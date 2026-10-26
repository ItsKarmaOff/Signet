// Signet password vault: encrypted logins (safeStorage, hence the system keychain), on-demand filling
// in pages and a save prompt after a login.
import { app, BrowserWindow, clipboard, dialog, ipcMain, safeStorage, systemPreferences, type WebContents } from 'electron'
import { copyFileSync, readFileSync } from 'node:fs'
import { readFile, writeFile } from 'node:fs/promises'
import { randomUUID } from 'node:crypto'
import { join } from 'node:path'
import { PASSWORD_EXTENSIONS } from '@shared/types'
import type { PasswordImportResult, PasswordManager, PasswordsInfo, Rect, SavedLogin, TabInfo } from '@shared/types'
import type { Browser } from '../../browser/browser'
import { AtomicWriter } from '../../core/atomic'
import { t } from '../../app/i18n'
import { verifyUser } from '../../app/osauth'
import { siteOf } from '../../core/urls'

/** http(s) origin of a tab address, or null. */
function webOriginOf(url: string): string | null {
  try {
    const u = new URL(url)
    return u.protocol === 'https:' || u.protocol === 'http:' ? u.origin : null
  } catch {
    return null
  }
}

/** Login as written on disk: the password is encrypted (base64). */
interface StoredLogin extends SavedLogin {
  password: string
}

/** Entered login awaiting an answer (in memory only). */
interface Pending {
  /** Tab where the login was entered: its key button reopens the prompt. */
  tabId: string
  origin: string
  username: string
  password: string
  /** Existing login to update. */
  existingId?: string
  at: number
}

const MANAGERS: PasswordManager[] = ['signet', 'off', ...PASSWORD_EXTENSIONS.map((m) => m.id)]
const MAX_FIELD = 1000
/** A prompt with no answer is forgotten after 5 minutes. */
/** A prompt with no answer (or postponed) stays available from the key button for 30 minutes. */
const PENDING_TTL = 30 * 60_000
/** Username entered alone (two-step login): kept 5 minutes for the password page. */
const IDENTIFIER_TTL = 5 * 60_000
/** After a successful authentication, later reveals do not ask again for 1 minute. */
const AUTH_GRACE = 60_000
/** A copied password is removed from the clipboard after 90 s (if it is still there). */
const CLIPBOARD_TTL = 90_000

export const isManager = (v: unknown): v is PasswordManager => typeof v === 'string' && (MANAGERS as string[]).includes(v)

/** http(s) origin of an input (`example.com`, `https://example.com/login`…), or null. */
export function normalizeOrigin(input: string): string | null {
  const text = input.trim()
  if (!text) return null
  try {
    const u = new URL(/^[a-z][a-z0-9+.-]*:\/\//i.test(text) ? text : `https://${text}`)
    return (u.protocol === 'https:' || u.protocol === 'http:') && u.hostname ? u.origin : null
  } catch {
    return null
  }
}

/** Reads a CSV (RFC 4180: quotes, doubled quotes, line breaks inside a field). */
export function parseCsv(text: string): string[][] {
  const rows: string[][] = []
  let row: string[] = []
  let field = ''
  let quoted = false
  const src = text.replace(/^\uFEFF/, '')
  for (let i = 0; i < src.length; i++) {
    const c = src[i]
    if (quoted) {
      if (c === '"' && src[i + 1] === '"') {
        field += '"'
        i++
      } else if (c === '"') quoted = false
      else field += c
    } else if (c === '"') quoted = true
    else if (c === ',') {
      row.push(field)
      field = ''
    } else if (c === '\n' || c === '\r') {
      if (c === '\r' && src[i + 1] === '\n') i++
      row.push(field)
      rows.push(row)
      row = []
      field = ''
    } else field += c
  }
  if (field || row.length) {
    row.push(field)
    rows.push(row)
  }
  return rows.filter((r) => r.some((f) => f.trim()))
}

const csvField = (s: string) => (/[",\r\n]/.test(s) ? `"${s.replace(/"/g, '""')}"` : s)

/** Columns of exports from Chrome, Safari, Firefox, Bitwarden, 1Password, LastPass, Proton Pass, Dashlane. */
const URL_COLUMNS = ['url', 'login_uri', 'website', 'origin', 'login url', 'web site', 'hostname']
const USER_COLUMNS = ['username', 'login_username', 'login', 'user', 'user name', 'email']
const PASSWORD_COLUMNS = ['password', 'login_password']

export class Passwords {
  private entries: StoredLogin[] = []
  private loaded = false
  /** Unreadable file: the existing file is not overwritten during the session. */
  private blocked = false
  /** Last prompt per tab + origin (anti-spam). */
  private lastPrompt = new Map<string, number>()
  private file = join(app.getPath('userData'), 'passwords.json')
  private writer = new AtomicWriter(this.file, () => (this.loaded && !this.blocked ? JSON.stringify({ version: 1, logins: this.entries }) : null))
  private pending = new Map<string, Pending>()
  /** Last username entered without a password, per tab. */
  private identifiers = new Map<string, { origin: string; username: string; at: number }>()
  private authUntil = 0
  /** Prompt shown in the main window's overlay. */
  private overlayToken: string | null = null

  constructor(private b: Browser) {}

  // ───────────────────────── Storage ─────────────────────────

  private load(): void {
    if (this.loaded) return
    this.loaded = true
    try {
      const raw = JSON.parse(readFileSync(this.file, 'utf8')) as { logins?: StoredLogin[] }
      this.entries = (raw.logins ?? []).filter((e) => e && typeof e.id === 'string' && typeof e.origin === 'string' && typeof e.password === 'string')
    } catch (err) {
      const code = (err as NodeJS.ErrnoException).code
      if (code === 'ENOENT') return // first launch
      if (err instanceof SyntaxError) {
        // Corrupted file: separate copy before the next save overwrites it.
        try {
          copyFileSync(this.file, `${this.file}.corrupt-${Date.now()}`)
        } catch {
          this.blocked = true
        }
      } else this.blocked = true
    }
  }

  private save(): void {
    this.writer.request()
    // Accounts added or removed: the tabs' key button follows.
    this.refreshAll()
  }

  flush(): void {
    if (this.loaded) this.writer.flushSync()
  }

  get encryption(): boolean {
    try {
      return safeStorage.isEncryptionAvailable()
    } catch {
      return false
    }
  }

  private encrypt(password: string): string {
    return safeStorage.encryptString(password).toString('base64')
  }

  private decrypt(e: StoredLogin): string | null {
    try {
      return safeStorage.decryptString(Buffer.from(e.password, 'base64'))
    } catch {
      return null
    }
  }

  private get manager(): PasswordManager {
    return this.b.store.state.settings.passwordManager ?? 'signet'
  }

  private find(id: unknown): StoredLogin | undefined {
    this.load()
    return typeof id === 'string' ? this.entries.find((e) => e.id === id) : undefined
  }

  // ───────────────────────── Settings ─────────────────────────

  list(): PasswordsInfo {
    this.load()
    const host = (o: string) => o.replace(/^\w+:\/\/(www\.)?/, '')
    const logins = this.entries
      .map(({ password: _password, ...rest }) => rest)
      .sort((x, y) => host(x.origin).localeCompare(host(y.origin)) || x.username.localeCompare(y.username))
    return { logins, encryption: this.encryption, touchId: this.touchId() }
  }

  private touchId(): boolean {
    try {
      return process.platform === 'darwin' && systemPreferences.canPromptTouchID()
    } catch {
      return false
    }
  }

  /** User authentication (system, otherwise confirmation), valid for one minute. Also used for the backup export. */
  async authenticate(reason: string): Promise<boolean> {
    if (Date.now() < this.authUntil) return true
    const res = await verifyUser(reason)
    if (res === 'denied') return false
    if (res === 'unavailable') {
      // Weaker fallback: simple confirmation, no proof of identity (Linux, Windows Hello absent).
      const { response } = await dialog.showMessageBox({
        type: 'warning',
        buttons: [t('passwords.confirm.allow'), t('passwords.confirm.cancel')],
        defaultId: 1,
        cancelId: 1,
        message: t('passwords.confirm.title'),
        detail: reason
      })
      if (response !== 0) return false
    }
    this.authUntil = Date.now() + AUTH_GRACE
    return true
  }

  async reveal(id: string): Promise<string | null> {
    const e = this.find(id)
    if (!e || !(await this.authenticate(t('passwords.touchIdReveal', { site: e.origin })))) return null
    return this.decrypt(e)
  }

  async copy(id: string): Promise<boolean> {
    const password = await this.reveal(id)
    if (password === null) return false
    await clipboard.writeText(password)
    setTimeout(() => {
      void (async () => {
        if ((await clipboard.readText()) === password) await clipboard.writeText('')
      })()
    }, CLIPBOARD_TTL)
    return true
  }

  /** Adds or edits a login; returns a translated error message, or null. */
  upsert(id: string | null, originInput: unknown, usernameInput: unknown, password: unknown): string | null {
    this.load()
    if (!this.encryption) return t('passwords.error.encryption')
    const origin = typeof originInput === 'string' ? normalizeOrigin(originInput) : null
    if (!origin) return t('passwords.error.site')
    const username = typeof usernameInput === 'string' ? usernameInput.trim().slice(0, MAX_FIELD) : ''
    const existing = id ? this.find(id) : undefined
    if (id && !existing) return t('passwords.error.missing')
    if (typeof password === 'string' && password.length > MAX_FIELD) return t('passwords.error.password')
    if (!existing && (typeof password !== 'string' || !password)) return t('passwords.error.password')
    if (this.entries.some((e) => e !== existing && e.origin === origin && e.username === username)) return t('passwords.error.duplicate')
    const now = Date.now()
    if (existing) {
      Object.assign(existing, { origin, username, updatedAt: now })
      if (typeof password === 'string' && password) existing.password = this.encrypt(password)
    } else {
      this.entries.push({ id: randomUUID(), origin, username, password: this.encrypt(password as string), createdAt: now, updatedAt: now })
    }
    this.save()
    return null
  }

  remove(id: string): void {
    this.load()
    const before = this.entries.length
    this.entries = this.entries.filter((e) => e.id !== id)
    if (this.entries.length !== before) this.save()
  }

  /** Imports a CSV exported from another browser or manager; logins already present are kept as is. */
  async importCsv(sender: WebContents): Promise<PasswordImportResult | null> {
    this.load()
    if (!this.encryption) return { added: 0, existing: 0, error: 'encryption' }
    const win = BrowserWindow.fromWebContents(sender)
    const options: Electron.OpenDialogOptions = { properties: ['openFile'], filters: [{ name: 'CSV', extensions: ['csv'] }] }
    const res = win ? await dialog.showOpenDialog(win, options) : await dialog.showOpenDialog(options)
    if (res.canceled || !res.filePaths[0]) return null
    let text: string
    try {
      text = await readFile(res.filePaths[0], 'utf8')
    } catch {
      return { added: 0, existing: 0, error: 'read' }
    }
    const rows = parseCsv(text)
    const header = (rows.shift() ?? []).map((h) => h.trim().toLowerCase())
    const col = (names: string[]) => names.map((n) => header.indexOf(n)).find((i) => i >= 0) ?? -1
    const ui = col(URL_COLUMNS)
    const pi = col(PASSWORD_COLUMNS)
    const user = USER_COLUMNS.map((n) => header.indexOf(n)).filter((i) => i >= 0)
    if (ui < 0 || pi < 0) return { added: 0, existing: 0, error: 'format' }
    return this.addMany(
      rows.map((row) => ({
        origin: row[ui] ?? '',
        // Proton Pass separates the username and the email: the first non-empty one wins.
        username: user.map((i) => (row[i] ?? '').trim()).find(Boolean) ?? '',
        password: row[pi] ?? ''
      }))
    )
  }

  /**
   * Adds logins coming from elsewhere (CSV, another browser); those already present (same site, same username)
   * are kept as is; for a duplicate inside the list, the first entry wins. `added`: number added; `existing`: those that were already saved before the call (duplicates
   * inside the list, common in browsers, do not count).
   */
  addMany(list: { origin: string; username: string; password: string; createdAt?: number }[]): { added: number; existing: number } {
    this.load()
    if (!this.encryption) return { added: 0, existing: 0 }
    const before = new Set(this.entries.map((e) => `${e.origin}\n${e.username}`))
    let added = 0
    let existing = 0
    const counted = new Set<string>()
    const now = Date.now()
    for (const l of list) {
      const origin = normalizeOrigin(l.origin)
      const username = l.username.trim().slice(0, MAX_FIELD)
      if (!origin || !l.password || l.password.length > MAX_FIELD) continue
      const key = `${origin}\n${username}`
      if (before.has(key)) {
        if (!counted.has(key)) existing++
        counted.add(key)
        continue
      }
      if (this.entries.some((e) => e.origin === origin && e.username === username)) continue
      const createdAt = l.createdAt && l.createdAt > 0 && l.createdAt <= now ? l.createdAt : now
      this.entries.push({ id: randomUUID(), origin, username, password: this.encrypt(l.password), createdAt, updatedAt: now })
      added++
    }
    if (added) this.save()
    return { added, existing }
  }

  /** Exports everything as CSV (Chrome format: name,url,username,password), after authentication. */
  async exportCsv(sender: WebContents): Promise<boolean> {
    this.load()
    if (!(await this.authenticate(t('passwords.touchIdExport')))) return false
    const win = BrowserWindow.fromWebContents(sender)
    const options: Electron.SaveDialogOptions = {
      defaultPath: join(app.getPath('documents'), t('passwords.exportFile')),
      filters: [{ name: 'CSV', extensions: ['csv'] }]
    }
    const res = win ? await dialog.showSaveDialog(win, options) : await dialog.showSaveDialog(options)
    if (res.canceled || !res.filePath) return false
    const lines = ['name,url,username,password']
    for (const e of this.entries) {
      const password = this.decrypt(e)
      if (password === null) continue
      lines.push([new URL(e.origin).hostname, e.origin, e.username, password].map(csvField).join(','))
    }
    await writeFile(res.filePath, `${lines.join('\n')}\n`, { mode: 0o600 })
    return true
  }

  // ───────────────────────── Pages ─────────────────────────

  /**
   * Channels of the page script (src/preload/passwords.ts). The origin always comes from the sending frame as seen by main,
   * never from the page, and only tabs (not the UI, nor extension pages) are served.
   */
  registerPageIpc(): void {
    ipcMain.handle('pw:accounts', (e) => {
      const origin = this.senderOrigin(e)
      if (!origin || this.manager !== 'signet') return []
      this.load()
      return this.entries.filter((x) => x.origin === origin).map((x) => ({ id: x.id, username: x.username }))
    })
    ipcMain.handle('pw:fill', (e, id: unknown) => {
      const origin = this.senderOrigin(e)
      const entry = this.find(id)
      if (!origin || !entry || entry.origin !== origin || this.manager !== 'signet') return null
      const password = this.decrypt(entry)
      if (password === null) return null
      if (!this.b.privates.owns(this.b.views.tabIdOfContents(e.sender.id) ?? '')) {
        entry.lastUsedAt = Date.now()
        this.save()
      }
      return { username: entry.username, password }
    })
    ipcMain.on('pw:submit', (e, username: unknown, password: unknown) => {
      const origin = this.senderOrigin(e)
      if (!origin || typeof username !== 'string' || typeof password !== 'string') return
      this.submitted(e.sender, origin, username.trim().slice(0, MAX_FIELD), password)
    })
  }

  /** http(s) origin of the sending frame, if it belongs to a tab; otherwise null. */
  private senderOrigin(e: Electron.IpcMainInvokeEvent | Electron.IpcMainEvent): string | null {
    if (!this.b.views.tabIdOfContents(e.sender.id)) return null
    const origin = e.senderFrame?.origin
    return origin && /^https?:\/\/[^/]+$/i.test(origin) ? origin : null
  }

  private submitted(sender: WebContents, origin: string, username: string, password: string): void {
    const tabId = this.b.views.tabIdOfContents(sender.id)
    if (!tabId || this.manager !== 'signet' || password.length > MAX_FIELD) return
    const now = Date.now()
    // "Username" step of a two-step login: kept for the password page.
    const isPrivate = this.b.privates.owns(tabId)
    if (!password) {
      if (username && !isPrivate) this.identifiers.set(tabId, { origin, username, at: now })
      return
    }
    if (!username) {
      const id = this.identifiers.get(tabId)
      if (id && id.origin === origin && now - id.at < IDENTIFIER_TTL) username = id.username
    }
    this.load()
    const same = this.entries.find((e) => e.origin === origin && e.username === username)
    if (same && this.decrypt(same) === password) {
      if (!isPrivate) {
        same.lastUsedAt = now
        this.save()
      }
      return
    }
    const { settings } = this.b.store.state
    if (settings.offerSavePasswords === false || (settings.neverSavePasswords ?? []).includes(origin) || !this.encryption) return
    if (isPrivate) return
    this.prunePending()
    // Anti-spam: one prompt per tab and origin every 3 s.
    const rateKey = `${tabId}\n${origin}`
    if (now - (this.lastPrompt.get(rateKey) ?? 0) < 3000) return
    this.lastPrompt.set(rateKey, now)
    if (this.lastPrompt.size > 200) for (const [k, at] of this.lastPrompt) if (now - at > 3000) this.lastPrompt.delete(k)
    // Form submission and button click often arrive together: a single prompt.
    if ([...this.pending.values()].some((p) => p.origin === origin && p.username === username && p.password === password)) return
    // Only one pending prompt per tab: the last entry wins.
    for (const [token, p] of this.pending) if (p.tabId === tabId) this.pending.delete(token)
    const token = randomUUID()
    this.pending.set(token, { tabId, origin, username, password, existingId: same?.id, at: now })
    this.refreshKey(tabId)
    if (this.b.overlay.openPassword(tabId, { token, origin, username, update: !!same })) this.overlayToken = token
    else if (this.b.model.activeTab()?.id === tabId && tabId !== this.b.peek.tabId) void this.askInDialog(sender, token)
    // Background tab: the prompt waits, available from the key button.
  }

  /** Fallback outside the main window (Little Signet, Peek, overlay already open): native dialog box. */
  private async askInDialog(sender: WebContents, token: string): Promise<void> {
    const p = this.pending.get(token)
    if (!p) return
    const host = p.origin.replace(/^\w+:\/\//, '')
    const win = BrowserWindow.getAllWindows().find((w) => w.contentView.children.some((v) => 'webContents' in v && v.webContents === sender)) ?? this.b.win
    const options: Electron.MessageBoxOptions = {
      type: 'question',
      buttons: [p.existingId ? t('passwords.prompt.update') : t('passwords.prompt.save'), t('passwords.prompt.never'), t('passwords.prompt.notNow')],
      defaultId: 0,
      cancelId: 2,
      message: p.existingId ? t('passwords.prompt.updateTitle', { site: host }) : t('passwords.prompt.saveTitle', { site: host }),
      detail: p.username || t('passwords.prompt.noUsername')
    }
    const { response } = win && !win.isDestroyed() ? await dialog.showMessageBox(win, options) : await dialog.showMessageBox(options)
    this.answer(token, response === 0 ? 'save' : response === 1 ? 'never' : 'dismiss')
  }

  /** Stores the shown prompt (delay elapsed): it stays available from the key button. */
  dismissPrompt(): void {
    if (this.overlayToken) this.answer(this.overlayToken, 'dismiss')
  }

  answer(token: string, answer: 'save' | 'never' | 'dismiss'): void {
    const p = this.pending.get(token)
    // "Not now" (or click outside): the prompt stays available from the key button.
    if (answer !== 'dismiss') this.pending.delete(token)
    if (p && answer === 'save') {
      this.load()
      const existing = p.existingId ? this.find(p.existingId) : this.entries.find((e) => e.origin === p.origin && e.username === p.username)
      const now = Date.now()
      if (existing) Object.assign(existing, { password: this.encrypt(p.password), updatedAt: now, lastUsedAt: now })
      else
        this.entries.push({
          id: randomUUID(),
          origin: p.origin,
          username: p.username,
          password: this.encrypt(p.password),
          createdAt: now,
          updatedAt: now,
          lastUsedAt: now
        })
      this.save()
    } else if (p && answer === 'never') {
      const { settings } = this.b.store.state
      settings.neverSavePasswords = [...new Set([...(settings.neverSavePasswords ?? []), p.origin])]
      this.b.broadcast.changed()
    }
    if (p) this.refreshKey(p.tabId)
    if (token === this.overlayToken) {
      this.overlayToken = null
      if (this.b.overlay.open) this.b.overlay.close()
    }
  }

  // ───────────────────────── Address bar key button ─────────────────────────

  /** Forgets prompts that are too old and those of closed tabs. */
  private prunePending(): void {
    const now = Date.now()
    for (const [token, p] of this.pending) if (now - p.at > PENDING_TTL || !this.b.store.state.tabs[p.tabId]) this.pending.delete(token)
  }

  /** Pending prompt for the tab, if it concerns the site it displays. */
  private pendingFor(tabId: string, origin: string): [string, Pending] | null {
    this.prunePending()
    for (const entry of this.pending) if (entry[1].tabId === tabId && siteOf(entry[1].origin) === siteOf(origin)) return entry
    return null
  }

  /** Key button state of a tab: pending prompt, accounts saved for its origin, or nothing. */
  private keyState(tabId: string): TabInfo['passwordKey'] {
    const tab = this.b.store.state.tabs[tabId]
    if (!tab || this.manager !== 'signet' || this.b.privates.owns(tabId)) return undefined
    const origin = webOriginOf(tab.url)
    if (!origin) return undefined
    if (this.pendingFor(tabId, origin)) return 'pending'
    this.load()
    return this.entries.some((e) => e.origin === origin) ? 'saved' : undefined
  }

  refreshKey(tabId: string, soft = false): void {
    const tab = this.b.store.state.tabs[tabId]
    if (tab) this.b.broadcast.patchTab(tabId, { passwordKey: this.keyState(tabId) }, { soft })
  }

  /** All tabs (accounts added or removed, manager changed). */
  refreshAll(): void {
    for (const id of Object.keys(this.b.store.state.tabs)) this.refreshKey(id)
  }

  /** Tab navigation: a prompt from another site is forgotten, then the button follows the new page. */
  onNavigate(tabId: string, url: string): void {
    const site = siteOf(url)
    for (const [token, p] of this.pending) {
      if (p.tabId !== tabId || siteOf(p.origin) === site) continue
      this.pending.delete(token)
      // The displayed card is about another site: put it away.
      if (token === this.overlayToken) {
        this.overlayToken = null
        if (this.b.overlay.open) this.b.overlay.close()
      }
    }
    this.refreshKey(tabId, true)
  }

  /** Click on the key button: reopens the pending prompt, otherwise the site's account list, under `anchor`. */
  openKey(tabId: string, anchor: Rect): void {
    const tab = this.b.store.state.tabs[tabId]
    const origin = tab ? webOriginOf(tab.url) : null
    if (!origin || this.manager !== 'signet' || this.b.privates.owns(tabId)) return
    const pending = this.pendingFor(tabId, origin)
    if (pending) {
      const [token, p] = pending
      if (this.b.overlay.openPassword(tabId, { token, origin: p.origin, username: p.username, update: !!p.existingId, anchor })) this.overlayToken = token
      return
    }
    this.load()
    const logins = this.entries.filter((e) => e.origin === origin).map((e) => ({ id: e.id, username: e.username }))
    if (logins.length) this.b.overlay.openPasswordList(tabId, { origin, logins, anchor })
  }
}

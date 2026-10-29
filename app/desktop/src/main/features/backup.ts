// Backup and restore of the configuration: a `.signetbackup` file (compressed JSON) whose parts can each be ticked.
// Passwords, optional, are encrypted in it with a backup password (scrypt + AES-256-GCM).
// Import works on the profile files then restarts the app: nothing in memory can overwrite it.
import { app, BrowserWindow, dialog, safeStorage, shell, type WebContents } from 'electron'
import { createCipheriv, createDecipheriv, randomBytes, randomUUID, scryptSync } from 'node:crypto'
import { copyFileSync, existsSync, mkdirSync, readdirSync, readFileSync, renameSync, statSync, unlinkSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { gunzipSync, gzipSync } from 'node:zlib'
import type { BackupInfo, BackupMode, BackupSection, BackupSummary, Bookmark, Item, Settings, Space, SpaceTheme, TabInfo } from '@shared/types'
import { BACKUP_SECTIONS } from '@shared/types'
import type { Browser } from '../browser/browser'
import { applyCleaned, cleanSettings, cleanSpaceDraft, newTab } from '../core/store'
import { writeFileSyncDurable } from '../core/atomic'
import { t } from '../app/i18n'

const FORMAT = 'signetbackup'
const VERSION = 1
/** Safety copies kept (made just before each import). */
const SAFETY_KEEP = 3
/** Maximum size of a backup file read (decompressed). */
const MAX_BYTES = 512 * 1024 * 1024
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i

/** Tab as saved: its content, not its identifier (new tabs are created on import). */
interface TabData {
  url: string
  title: string
  favicon?: string
  pinnedUrl?: string
  customTitle?: string
  customIcon?: string
  muted?: boolean
}

/** Sidebar item with its tabs in clear. */
type ItemData =
  | { kind: 'tab'; tab: TabData; group?: string }
  | { kind: 'split'; tabs: TabData[]; layout?: string; sizes?: number[]; group?: string }
  | { kind: 'folder'; name: string; icon?: string; open: boolean; hue?: number; live?: unknown; items: ItemData[] }

/** Marker for a space (today tabs and bookmarks of a space absent from the backup or the profile). */
interface SpaceShell {
  id: string
  name: string
  icon: string
  hue: number
}

interface SpaceData extends SpaceShell {
  theme?: unknown
  profile?: 'shared' | 'isolated'
  pinned: ItemData[]
}

interface Sealed {
  kdf: 'scrypt'
  salt: string
  iv: string
  tag: string
  data: string
}

interface PlainLogin {
  origin: string
  username: string
  password: string
  createdAt?: number
  updatedAt?: number
}

interface BackupFile {
  format: typeof FORMAT
  version: number
  createdAt: number
  appVersion: string
  /** Safety copies only: passwords file as is, always encrypted by the system keychain (so readable on this computer only). */
  keychain?: Record<string, unknown>[]
  sections: Partial<{
    settings: Record<string, unknown>
    favorites: TabData[]
    spaces: SpaceData[]
    today: { space: SpaceShell; items: ItemData[]; groups?: unknown }[]
    bookmarks: { space: SpaceShell; bookmarks: Bookmark[] }[]
    history: Record<string, unknown>[]
    archive: Record<string, unknown>[]
    boosts: Record<string, unknown>
    permissions: Record<string, unknown>
    mods: { disabled?: boolean; mods: Record<string, unknown>[] }
    notes: Record<string, unknown>[]
    passwords: Sealed
    extensions: { id: string; name: string }[]
  }>
}

/** Profile files read or written by the backup. */
const P = {
  dir: () => app.getPath('userData'),
  state: () => join(P.dir(), 'state.json'),
  bookmarks: () => join(P.dir(), 'bookmarks.json'),
  history: () => join(P.dir(), 'history.json'),
  passwords: () => join(P.dir(), 'passwords.json'),
  mods: () => join(P.dir(), 'mods.json'),
  notes: () => join(P.dir(), 'notes'),
  safety: () => join(P.dir(), 'backups'),
  pendingExtensions: () => join(P.dir(), 'pending-extensions.json')
}

const isObj = (v: unknown): v is Record<string, unknown> => !!v && typeof v === 'object' && !Array.isArray(v)
const str = (v: unknown, max = 4000): string => (typeof v === 'string' ? v.slice(0, max) : '')
const isWebish = (url: string) => /^(https?|file|signet):/i.test(url)

function readJson<T>(path: string, fallback: T): T {
  try {
    return JSON.parse(readFileSync(path, 'utf8')) as T
  } catch {
    return fallback
  }
}

/** Synchronous atomic write (unique tmp + fsync + rename): never crosses an `AtomicWriter` write. */
function writeAtomic(path: string, data: string | Buffer, mode?: number): void {
  const tmp = `${path}.restore-${process.pid}-${Date.now()}.tmp`
  try {
    writeFileSyncDurable(tmp, data, mode)
    renameSync(tmp, path)
  } catch (error) {
    try {
      unlinkSync(tmp)
    } catch {
      // already absent
    }
    throw error
  }
}

function writeJson(path: string, value: unknown): void {
  writeAtomic(path, JSON.stringify(value))
}

// ───────────────────────── Password encryption ─────────────────────────

export function seal(value: unknown, password: string): Sealed {
  const salt = randomBytes(16)
  const iv = randomBytes(12)
  const key = scryptSync(password, salt, 32, { N: 1 << 15, r: 8, p: 1, maxmem: 64 * 1024 * 1024 })
  const cipher = createCipheriv('aes-256-gcm', key, iv)
  const data = Buffer.concat([cipher.update(JSON.stringify(value), 'utf8'), cipher.final()])
  return { kdf: 'scrypt', salt: salt.toString('base64'), iv: iv.toString('base64'), tag: cipher.getAuthTag().toString('base64'), data: data.toString('base64') }
}

/** Decrypts; null if the password is wrong or the block is damaged. */
export function unseal(sealed: Sealed, password: string): unknown {
  try {
    const key = scryptSync(password, Buffer.from(sealed.salt, 'base64'), 32, { N: 1 << 15, r: 8, p: 1, maxmem: 64 * 1024 * 1024 })
    const decipher = createDecipheriv('aes-256-gcm', key, Buffer.from(sealed.iv, 'base64'))
    decipher.setAuthTag(Buffer.from(sealed.tag, 'base64'))
    const plain = Buffer.concat([decipher.update(Buffer.from(sealed.data, 'base64')), decipher.final()])
    return JSON.parse(plain.toString('utf8'))
  } catch {
    return null
  }
}

// ───────────────────────── Tabs and items ─────────────────────────

export function tabData(tab: TabInfo | undefined): TabData | null {
  if (!tab || !tab.url) return null
  const d: TabData = { url: tab.url, title: tab.title ?? '' }
  if (tab.favicon) d.favicon = tab.favicon
  if (tab.pinnedUrl) d.pinnedUrl = tab.pinnedUrl
  if (tab.customTitle) d.customTitle = tab.customTitle
  if (tab.customIcon) d.customIcon = tab.customIcon
  if (tab.muted) d.muted = true
  return d
}

export function itemsData(items: Item[], tabs: Record<string, TabInfo>): ItemData[] {
  const out: ItemData[] = []
  for (const item of items) {
    if (item.kind === 'tab') {
      const tab = tabData(tabs[item.tabId])
      if (tab) out.push({ kind: 'tab', tab, ...(item.group && { group: item.group }) })
    } else if (item.kind === 'split') {
      const list = item.tabIds.map((id) => tabData(tabs[id])).filter((x): x is TabData => !!x)
      if (list.length >= 2) out.push({ kind: 'split', tabs: list, layout: item.layout, sizes: item.sizes, ...(item.group && { group: item.group }) })
    } else {
      out.push({
        kind: 'folder',
        name: item.name,
        icon: item.icon,
        open: item.open,
        hue: item.hue,
        live: (item as { live?: unknown }).live,
        items: itemsData(item.items, tabs)
      })
    }
  }
  return out
}

/** Validated saved tab, or null (missing address or unexpected scheme). */
export function cleanTab(v: unknown): TabData | null {
  if (!isObj(v)) return null
  const url = str(v.url, 8000)
  if (!url || !isWebish(url)) return null
  return {
    url,
    title: str(v.title, 1000),
    favicon: str(v.favicon, 200_000) || undefined,
    pinnedUrl: str(v.pinnedUrl, 8000) && isWebish(str(v.pinnedUrl, 8000)) ? str(v.pinnedUrl, 8000) : undefined,
    customTitle: str(v.customTitle, 200) || undefined,
    customIcon: str(v.customIcon, 16) || undefined,
    muted: v.muted === true || undefined
  }
}

/** Creates the tab in the state (new identifier) and returns its id. */
function addTab(tabs: Record<string, TabInfo>, data: TabData): string {
  const tab = newTab(data.url, data.title)
  Object.assign(tab, {
    favicon: data.favicon,
    pinnedUrl: data.pinnedUrl,
    customTitle: data.customTitle,
    customIcon: data.customIcon,
    muted: !!data.muted
  })
  tabs[tab.id] = tab
  return tab.id
}

/** Rebuilds items (new tabs); `pinned`: tabs take their "home" address. */
export function buildItems(list: unknown, tabs: Record<string, TabInfo>, pinned: boolean, depth = 0): Item[] {
  if (!Array.isArray(list) || depth > 8) return []
  const out: Item[] = []
  for (const raw of list.slice(0, 2000)) {
    if (!isObj(raw)) continue
    const group = pinned ? undefined : str(raw.group, 64) || undefined
    if (raw.kind === 'tab') {
      const data = cleanTab(raw.tab)
      if (!data) continue
      if (pinned && !data.pinnedUrl) data.pinnedUrl = data.url
      out.push({ kind: 'tab', tabId: addTab(tabs, data), ...(group && { group }) })
    } else if (raw.kind === 'split' && Array.isArray(raw.tabs)) {
      const ids = raw.tabs
        .map(cleanTab)
        .filter((x): x is TabData => !!x)
        .slice(0, 4)
        .map((d) => addTab(tabs, d))
      if (ids.length < 2) continue
      const layout = raw.layout === 'rows' || raw.layout === 'grid' ? raw.layout : undefined
      out.push({ kind: 'split', id: randomUUID(), tabIds: ids, ...(layout && { layout }), ...(group && { group }) })
    } else if (raw.kind === 'folder' && pinned) {
      const folder: Extract<Item, { kind: 'folder' }> = {
        kind: 'folder',
        id: randomUUID(),
        name: str(raw.name, 200) || t('defaults.folder'),
        open: raw.open !== false,
        items: buildItems(raw.items, tabs, true, depth + 1)
      }
      if (str(raw.icon, 16)) folder.icon = str(raw.icon, 16)
      if (typeof raw.hue === 'number' && Number.isFinite(raw.hue)) folder.hue = Math.round(raw.hue) % 360
      // Dynamic folder: the source is revalidated when the state loads (tree.ts).
      if (isObj(raw.live)) (folder as { live?: unknown }).live = { source: raw.live.source }
      out.push(folder)
    }
  }
  return out
}

/** Addresses of the tabs in a list of items (to avoid importing the same page twice in "add" mode). */
export function urlsOf(items: Item[], tabs: Record<string, TabInfo>, into = new Set<string>()): Set<string> {
  for (const item of items) {
    if (item.kind === 'tab') into.add(tabs[item.tabId]?.url ?? '')
    else if (item.kind === 'split') item.tabIds.forEach((id) => into.add(tabs[id]?.url ?? ''))
    else urlsOf(item.items, tabs, into)
  }
  return into
}

const firstUrl = (item: Item, tabs: Record<string, TabInfo>): string =>
  item.kind === 'tab' ? (tabs[item.tabId]?.url ?? '') : item.kind === 'split' ? (tabs[item.tabIds[0]]?.url ?? '') : `folder:${item.name}`

function referenced(state: StateFile): Set<string> {
  const ids = new Set<string>(state.favorites)
  const walk = (items: Item[]) => {
    for (const i of items) {
      if (i.kind === 'tab') ids.add(i.tabId)
      else if (i.kind === 'split') i.tabIds.forEach((x) => ids.add(x))
      else walk(i.items)
    }
  }
  for (const s of state.spaces) {
    walk(s.pinned)
    walk(s.today)
  }
  return ids
}

/** state.json as it is on disk (only what the backup handles). */
interface StateFile {
  tabs: Record<string, TabInfo>
  favorites: string[]
  spaces: Space[]
  activeSpaceId: string
  settings: Record<string, unknown>
  archive: Record<string, unknown>[]
  boosts: Record<string, unknown>
  sitePermissions: Record<string, unknown>
  [key: string]: unknown
}

// ───────────────────────── Module ─────────────────────────

export class Backup {
  /** Backup read and awaiting import (the file never goes back through the interface). */
  private pending: { token: string; file: BackupFile } | null = null

  constructor(private b: Browser) {}

  /** Writes to disk everything pending in memory, to read up-to-date files. */
  private flushAll(): void {
    this.b.store.flush()
    this.b.history.flush()
    this.b.passwords.flush()
    this.b.mods.flush()
    this.b.notes.flush()
  }

  private readState(): StateFile {
    return readJson<StateFile>(P.state(), {} as StateFile)
  }

  /** Number of items in each part, for the checkbox labels. */
  info(): BackupInfo {
    this.flushAll()
    const counts = this.counts(this.collect(new Set(BACKUP_SECTIONS), null))
    // Passwords are only encrypted at export: here we count the keychain credentials.
    const logins = readJson<{ logins?: unknown[] }>(P.passwords(), {}).logins
    counts.passwords = Array.isArray(logins) ? logins.length : 0
    return { counts, safety: this.safetyList() }
  }

  private counts(file: BackupFile): Partial<Record<BackupSection, number>> {
    const s = file.sections
    const countItems = (items: unknown): number =>
      Array.isArray(items)
        ? items.reduce<number>(
            (n, i) => n + (isObj(i) ? (i.kind === 'tab' ? 1 : i.kind === 'split' ? (Array.isArray(i.tabs) ? i.tabs.length : 0) : countItems(i.items)) : 0),
            0
          )
        : 0
    const out: Partial<Record<BackupSection, number>> = {}
    if (s.settings) out.settings = 1
    if (s.favorites) out.favorites = s.favorites.length
    if (s.spaces) out.spaces = s.spaces.length
    if (s.today) out.today = s.today.reduce((n, x) => n + countItems(x.items), 0)
    if (s.bookmarks) {
      const walk = (list: unknown): number =>
        Array.isArray(list) ? list.reduce<number>((n, x) => n + (isObj(x) ? (x.kind === 'folder' ? walk(x.items) : 1) : 0), 0) : 0
      out.bookmarks = s.bookmarks.reduce((n, x) => n + walk(x.bookmarks), 0)
    }
    if (s.history) out.history = s.history.length
    if (s.archive) out.archive = s.archive.length
    if (s.boosts) out.boosts = Object.keys(s.boosts).length
    if (s.permissions) out.permissions = Object.keys(s.permissions).length
    if (s.mods) out.mods = s.mods.mods.length
    if (s.notes) out.notes = s.notes.length
    if (s.passwords) out.passwords = -1 // unknown count until the file is decrypted
    if (Array.isArray(file.keychain)) out.passwords = file.keychain.length
    if (s.extensions) out.extensions = s.extensions.length
    return out
  }

  /** Builds the backup of the requested parts from the profile files. */
  private collect(wanted: Set<BackupSection>, password: string | null): BackupFile {
    const state = this.readState()
    const tabs = isObj(state.tabs) ? (state.tabs as Record<string, TabInfo>) : {}
    const spaces = Array.isArray(state.spaces) ? state.spaces : []
    const shell = (s: Space): SpaceShell => ({ id: s.id, name: s.name, icon: s.icon, hue: s.hue })
    const sections: BackupFile['sections'] = {}
    if (wanted.has('settings') && isObj(state.settings)) sections.settings = state.settings
    if (wanted.has('favorites')) sections.favorites = (state.favorites ?? []).map((id) => tabData(tabs[id])).filter((x): x is TabData => !!x)
    if (wanted.has('spaces'))
      sections.spaces = spaces.map((s) => ({
        ...shell(s),
        ...(s.theme && { theme: s.theme }),
        ...(s.profile && { profile: s.profile }),
        pinned: itemsData(s.pinned ?? [], tabs)
      }))
    if (wanted.has('today')) sections.today = spaces.map((s) => ({ space: shell(s), items: itemsData(s.today ?? [], tabs), groups: s.groups }))
    if (wanted.has('bookmarks')) {
      const saved = readJson<Record<string, unknown>>(P.bookmarks(), {})
      sections.bookmarks = spaces.map((s) => ({
        space: shell(s),
        bookmarks: (Array.isArray(saved[s.id]) ? saved[s.id] : (s.bookmarks ?? [])) as Bookmark[]
      }))
    }
    if (wanted.has('history')) sections.history = readJson<Record<string, unknown>[]>(P.history(), []).filter(isObj)
    if (wanted.has('archive')) sections.archive = Array.isArray(state.archive) ? state.archive : []
    if (wanted.has('boosts')) sections.boosts = isObj(state.boosts) ? state.boosts : {}
    if (wanted.has('permissions')) sections.permissions = isObj(state.sitePermissions) ? state.sitePermissions : {}
    if (wanted.has('mods')) {
      const m = readJson<{ disabled?: boolean; mods?: unknown[] }>(P.mods(), {})
      sections.mods = { disabled: m.disabled === true, mods: (Array.isArray(m.mods) ? m.mods : []).filter(isObj) }
    }
    if (wanted.has('notes')) sections.notes = this.readNotes()
    if (wanted.has('extensions')) sections.extensions = this.b.extensions.list().map((e) => ({ id: e.id, name: e.name }))
    if (wanted.has('passwords') && password) sections.passwords = seal(this.readLogins(), password)
    return { format: FORMAT, version: VERSION, createdAt: Date.now(), appVersion: app.getVersion(), sections }
  }

  private readNotes(): Record<string, unknown>[] {
    const dir = P.notes()
    if (!existsSync(dir)) return []
    const out: Record<string, unknown>[] = []
    for (const name of readdirSync(dir)) {
      if (!/^[0-9a-f-]{36}\.json$/i.test(name)) continue
      const doc = readJson<unknown>(join(dir, name), null)
      if (isObj(doc) && typeof doc.id === 'string' && UUID.test(doc.id)) out.push(doc)
    }
    return out
  }

  /** Keychain credentials, decrypted (to be re-encrypted with the backup password). */
  private readLogins(): PlainLogin[] {
    const raw = readJson<{ logins?: unknown[] }>(P.passwords(), {})
    const out: PlainLogin[] = []
    for (const e of Array.isArray(raw.logins) ? raw.logins : []) {
      if (!isObj(e) || typeof e.password !== 'string') continue
      try {
        const password = safeStorage.decryptString(Buffer.from(e.password, 'base64'))
        out.push({
          origin: str(e.origin),
          username: str(e.username, 1000),
          password,
          createdAt: Number(e.createdAt) || undefined,
          updatedAt: Number(e.updatedAt) || undefined
        })
      } catch {
        // unreadable on this machine: ignored
      }
    }
    return out
  }

  /** Same check as for revealing a password (Touch ID, account password, Windows Hello, otherwise confirmation). */
  private touchId(reason: string): Promise<boolean> {
    return this.b.passwords.authenticate(reason)
  }

  // ───────────────────────── Export ─────────────────────────

  /** Exports the ticked parts; `password` is required with passwords. Returns an error message, or null. */
  async exportTo(sender: WebContents, sections: BackupSection[], password: string | null): Promise<string | null> {
    const wanted = new Set(sections.filter((s) => (BACKUP_SECTIONS as readonly string[]).includes(s)))
    if (wanted.size === 0) return t('backup.error.nothing')
    if (wanted.has('passwords')) {
      if (!password || password.length < 6) return t('backup.error.passwordShort')
      if (!(await this.touchId(t('backup.touchIdExport')))) return t('backup.error.auth')
    }
    const win = BrowserWindow.fromWebContents(sender)
    const d = new Date()
    const pad = (n: number) => String(n).padStart(2, '0')
    const options: Electron.SaveDialogOptions = {
      title: t('backup.exportTitle'),
      defaultPath: join(app.getPath('documents'), `${t('backup.fileName')} ${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}.signetbackup`),
      filters: [{ name: t('backup.fileType'), extensions: ['signetbackup'] }]
    }
    const res = win ? await dialog.showSaveDialog(win, options) : await dialog.showSaveDialog(options)
    if (res.canceled || !res.filePath) return null
    this.flushAll()
    try {
      // Tmp then rename: a failure along the way does not destroy the previous backup.
      writeAtomic(res.filePath, gzipSync(JSON.stringify(this.collect(wanted, password))), 0o600)
      return null
    } catch (error) {
      console.error('Sauvegarde :', error)
      return t('backup.error.write')
    }
  }

  // ───────────────────────── Import: reading ─────────────────────────

  private parse(path: string): BackupFile | null {
    try {
      const buf = readFileSync(path)
      if (buf.length > MAX_BYTES) return null
      const text = (buf[0] === 0x1f && buf[1] === 0x8b ? gunzipSync(buf, { maxOutputLength: MAX_BYTES }) : buf).toString('utf8')
      const file = JSON.parse(text) as BackupFile
      return file && file.format === FORMAT && typeof file.version === 'number' && file.version <= VERSION && isObj(file.sections) ? file : null
    } catch {
      return null
    }
  }

  private summary(file: BackupFile): BackupSummary {
    const token = randomUUID()
    this.pending = { token, file }
    return {
      token,
      createdAt: Number(file.createdAt) || 0,
      appVersion: str(file.appVersion, 40),
      counts: this.counts(file),
      ...(Array.isArray(file.keychain) && { systemSealed: true })
    }
  }

  /** Picks a file and returns its summary (parts present), or an error message, or null if cancelled. */
  async open(sender: WebContents): Promise<BackupSummary | string | null> {
    const win = BrowserWindow.fromWebContents(sender)
    const options: Electron.OpenDialogOptions = { properties: ['openFile'], filters: [{ name: t('backup.fileType'), extensions: ['signetbackup'] }] }
    const res = win ? await dialog.showOpenDialog(win, options) : await dialog.showOpenDialog(options)
    if (res.canceled || !res.filePaths[0]) return null
    const file = this.parse(res.filePaths[0])
    return file ? this.summary(file) : t('backup.error.invalid')
  }

  /** Safety copy chosen from the list (by its file name). */
  openSafety(name: string): BackupSummary | string {
    if (!/^safety-\d+\.signetbackup$/.test(name)) return t('backup.error.invalid')
    const file = this.parse(join(P.safety(), name))
    return file ? this.summary(file) : t('backup.error.invalid')
  }

  private safetyList(): { name: string; createdAt: number }[] {
    const dir = P.safety()
    if (!existsSync(dir)) return []
    return readdirSync(dir)
      .filter((n) => /^safety-\d+\.signetbackup$/.test(n))
      .map((name) => ({ name, createdAt: Number(/\d+/.exec(name)?.[0]) || statSync(join(dir, name)).mtimeMs }))
      .sort((a, b) => b.createdAt - a.createdAt)
  }

  revealSafety(): void {
    mkdirSync(P.safety(), { recursive: true })
    void shell.openPath(P.safety())
  }

  // ───────────────────────── Import : application ─────────────────────────

  /**
   * Imports the ticked parts of the backup read (`token`), adding or replacing, then restarts the app.
   * Returns an error message (wrong password…), or null if the user gave up.
   */
  async apply(sender: WebContents, token: string, sections: BackupSection[], mode: BackupMode, password: string | null): Promise<string | null> {
    const pending = this.pending
    if (!pending || pending.token !== token) return t('backup.error.invalid')
    const file = pending.file
    const wanted = new Set(
      sections.filter((s) => (BACKUP_SECTIONS as readonly string[]).includes(s) && file.sections[s as keyof BackupFile['sections']] !== undefined)
    )
    if (wanted.size === 0) return t('backup.error.nothing')
    let logins: PlainLogin[] | null = null
    // Safety copy: passwords already encrypted by the system, restored as is (without a backup password).
    let keychain: Record<string, unknown>[] | null = null
    if (wanted.has('passwords') && Array.isArray(file.keychain)) {
      keychain = file.keychain.filter((l) => isObj(l) && typeof l.origin === 'string' && typeof l.password === 'string')
    } else if (wanted.has('passwords') && file.sections.passwords) {
      const plain = unseal(file.sections.passwords, password ?? '')
      if (!Array.isArray(plain)) return t('backup.error.wrongPassword')
      logins = plain.filter(isObj).map((l) => ({
        origin: str(l.origin),
        username: str(l.username, 1000),
        password: str(l.password, 1000),
        createdAt: Number(l.createdAt) || undefined,
        updatedAt: Number(l.updatedAt) || undefined
      }))
      if (!safeStorage.isEncryptionAvailable()) return t('backup.error.encryption')
    }
    const win = BrowserWindow.fromWebContents(sender)
    const confirm: Electron.MessageBoxOptions = {
      type: mode === 'replace' ? 'warning' : 'question',
      message: t(mode === 'replace' ? 'backup.confirm.replace' : 'backup.confirm.add'),
      detail: t('backup.confirm.detail'),
      buttons: [t('backup.confirm.ok'), t('dialog.cancel')],
      // Replacing erases data: the default button is "Cancel".
      defaultId: mode === 'replace' ? 1 : 0,
      cancelId: 1
    }
    const { response } = win ? await dialog.showMessageBox(win, confirm) : await dialog.showMessageBox(confirm)
    if (response !== 0) return null

    // Safety copy of the current state (passwords included, encrypted by the system), then import on the files, then restart.
    this.flushAll()
    try {
      this.writeSafety()
    } catch (error) {
      console.error('Backup: could not make the safety copy', error)
      return t('backup.error.safety')
    }
    try {
      this.applySections(file, wanted, mode, logins, keychain)
    } catch (error) {
      console.error('Sauvegarde : import impossible', error)
      return t('backup.error.import')
    }
    this.pending = null
    app.relaunch()
    // Immediate exit: shutdown saves would rewrite the old state over the import.
    app.exit(0)
    return null
  }

  private writeSafety(): void {
    const dir = P.safety()
    mkdirSync(dir, { recursive: true })
    const all = new Set<BackupSection>(BACKUP_SECTIONS.filter((s) => s !== 'passwords'))
    const file = this.collect(all, null)
    const saved = readJson<{ logins?: unknown[] }>(P.passwords(), {}).logins
    if (Array.isArray(saved) && saved.length) file.keychain = saved.filter(isObj)
    const stamp = Date.now()
    writeFileSync(join(dir, `safety-${stamp}.signetbackup`), gzipSync(JSON.stringify(file)), { mode: 0o600 })
    // Current keychain (already encrypted by the system), copied as is before any overwrite.
    if (existsSync(P.passwords())) {
      copyFileSync(P.passwords(), join(dir, `passwords-${stamp}.json`))
      const olds = readdirSync(dir)
        .filter((n) => /^passwords-\d+\.json$/.test(n))
        .sort((a, b) => Number(/\d+/.exec(b)?.[0]) - Number(/\d+/.exec(a)?.[0]))
      for (const old of olds.slice(SAFETY_KEEP)) {
        try {
          unlinkSync(join(dir, old))
        } catch {
          // best-effort
        }
      }
    }
    for (const old of this.safetyList().slice(SAFETY_KEEP)) {
      try {
        unlinkSync(join(dir, old.name))
      } catch {
        // best-effort
      }
    }
  }

  private applySections(
    file: BackupFile,
    wanted: Set<BackupSection>,
    mode: BackupMode,
    logins: PlainLogin[] | null,
    keychain: Record<string, unknown>[] | null
  ): void {
    const s = file.sections
    const replace = mode === 'replace'
    const state = this.readState()
    if (!isObj(state.tabs)) state.tabs = {}
    if (!Array.isArray(state.spaces)) state.spaces = []
    if (!Array.isArray(state.favorites)) state.favorites = []
    const tabs = state.tabs

    // Settings: always replaced (a mix of two sets of settings makes no sense).
    // Validated field by field; unpacked extension folders never come from a backup.
    if (wanted.has('settings') && isObj(s.settings)) {
      const current = (isObj(state.settings) ? state.settings : {}) as unknown as Settings
      const incoming = { ...s.settings } as unknown as Settings
      delete incoming.unpackedExtensions
      if (current.unpackedExtensions) incoming.unpackedExtensions = current.unpackedExtensions
      applyCleaned(incoming, cleanSettings(incoming, current))
      state.settings = incoming as unknown as Record<string, unknown>
    }

    if (wanted.has('favorites') && Array.isArray(s.favorites)) {
      const have = new Set(replace ? [] : state.favorites.map((id) => tabs[id]?.url))
      const added = s.favorites
        .map(cleanTab)
        .filter((d): d is TabData => !!d && !have.has(d.url))
        .map((d) => addTab(tabs, { ...d, pinnedUrl: d.pinnedUrl ?? d.url }))
      state.favorites = replace ? added : [...state.favorites, ...added]
    }

    if (wanted.has('spaces') && Array.isArray(s.spaces)) {
      const incoming = s.spaces.filter(isObj)
      const byId = new Map(state.spaces.map((sp) => [sp.id, sp]))
      const next: Space[] = replace ? [] : [...state.spaces]
      for (const raw of incoming) {
        const id = typeof raw.id === 'string' && UUID.test(raw.id) ? raw.id : randomUUID()
        const existing = byId.get(id)
        const pinned = buildItems(raw.pinned, tabs, true)
        if (existing && !replace) {
          // Same space: we add the pinned items that are missing.
          const have = urlsOf(existing.pinned, tabs)
          existing.pinned.push(...pinned.filter((i) => !have.has(firstUrl(i, tabs))))
          continue
        }
        const space: Space = {
          ...(existing ?? {}),
          id,
          name: str(raw.name, 40) || t('defaults.space'),
          icon: str(raw.icon, 16) || '✨',
          hue: typeof raw.hue === 'number' && Number.isFinite(raw.hue) ? Math.round(raw.hue) % 360 : 250,
          pinned,
          today: existing?.today ?? [],
          activeTabId: null
        }
        // Gradient validated as in the space form; otherwise solid color.
        const clean = cleanSpaceDraft({
          name: space.name,
          icon: space.icon,
          hue: space.hue,
          theme: isObj(raw.theme) ? (raw.theme as unknown as SpaceTheme) : null
        })
        if (clean.theme) space.theme = clean.theme
        else delete space.theme
        if (raw.profile === 'isolated') space.profile = 'isolated'
        else delete space.profile
        next.push(space)
      }
      if (next.length) state.spaces = next
      if (!state.spaces.some((sp) => sp.id === state.activeSpaceId)) state.activeSpaceId = state.spaces[0]?.id
    }

    /** Profile space that receives the today tabs or the bookmarks of a backup space. */
    const target = (shell: unknown): Space | undefined => {
      const sh = isObj(shell) ? shell : {}
      return (
        state.spaces.find((sp) => sp.id === sh.id) ??
        state.spaces.find((sp) => sp.name === sh.name) ??
        state.spaces.find((sp) => sp.id === state.activeSpaceId) ??
        state.spaces[0]
      )
    }

    if (wanted.has('today') && Array.isArray(s.today)) {
      const cleared = new Set<Space>()
      for (const entry of s.today.filter(isObj)) {
        const space = target(entry.space)
        if (!space) continue
        if (replace && !cleared.has(space)) {
          space.today = []
          cleared.add(space)
        }
        const have = urlsOf(space.today, tabs)
        space.today.push(...buildItems(entry.items, tabs, false).filter((i) => !have.has(firstUrl(i, tabs))))
        if (isObj(entry.groups)) space.groups = { ...(space.groups ?? {}), ...(entry.groups as Space['groups']) }
      }
    }

    // Tabs that nothing references anymore (replaced): removed from the state.
    const used = referenced(state)
    for (const id of Object.keys(tabs)) if (!used.has(id)) delete tabs[id]
    for (const sp of state.spaces) if (sp.activeTabId && !used.has(sp.activeTabId)) sp.activeTabId = null

    if (wanted.has('archive') && Array.isArray(s.archive)) {
      const incoming = s.archive.filter((a) => isObj(a) && typeof a.url === 'string' && isWebish(a.url))
      const current = replace ? [] : (state.archive ?? []).filter(isObj)
      const key = (a: Record<string, unknown>) => (typeof a.id === 'string' ? a.id : `${a.archivedAt}:${a.url}`)
      const have = new Set(current.map(key))
      state.archive = [...current, ...incoming.filter((a) => !have.has(key(a)))]
        .sort((a, b) => (Number(b.archivedAt) || 0) - (Number(a.archivedAt) || 0))
        .slice(0, 500)
    }
    if (wanted.has('boosts') && isObj(s.boosts)) state.boosts = replace ? s.boosts : { ...s.boosts, ...(isObj(state.boosts) ? state.boosts : {}) }
    if (wanted.has('permissions') && isObj(s.permissions))
      state.sitePermissions = replace ? s.permissions : { ...s.permissions, ...(isObj(state.sitePermissions) ? state.sitePermissions : {}) }

    // Bookmarks: bookmarks.json, per space.
    if (wanted.has('bookmarks') && Array.isArray(s.bookmarks)) {
      const saved = readJson<Record<string, unknown>>(P.bookmarks(), {})
      const cleared = new Set<string>()
      for (const entry of s.bookmarks.filter(isObj)) {
        const space = target(entry.space)
        if (!space || !Array.isArray(entry.bookmarks)) continue
        const current = replace && !cleared.has(space.id) ? [] : Array.isArray(saved[space.id]) ? (saved[space.id] as Record<string, unknown>[]) : []
        cleared.add(space.id)
        const keyOf = (bm: unknown) => (isObj(bm) ? (bm.kind === 'folder' ? `folder:${bm.name}` : str(bm.url, 8000)) : '')
        const have = new Set(current.map(keyOf))
        saved[space.id] = [...current, ...entry.bookmarks.filter((bm) => isObj(bm) && !have.has(keyOf(bm)))]
        delete space.bookmarks // bookmarks.json is authoritative
      }
      writeJson(P.bookmarks(), saved)
    }

    writeJson(P.state(), state)

    if (wanted.has('history') && Array.isArray(s.history)) {
      const incoming = s.history.filter((h) => isObj(h) && typeof h.url === 'string' && isWebish(h.url))
      const current = replace ? [] : readJson<Record<string, unknown>[]>(P.history(), []).filter(isObj)
      const byUrl = new Map(current.map((h) => [h.url as string, h]))
      for (const h of incoming) {
        const entry = {
          url: h.url as string,
          title: str(h.title, 1000),
          favicon: str(h.favicon, 200_000) || undefined,
          visits: Math.max(1, Number(h.visits) || 1),
          ...(Number(h.typed) > 0 ? { typed: Math.floor(Number(h.typed)) } : {}),
          lastVisit: Number(h.lastVisit) || 0
        }
        const mine = byUrl.get(entry.url)
        // Same address: visits summed, most recent last visit.
        if (mine)
          byUrl.set(entry.url, {
            ...mine,
            visits: (Number(mine.visits) || 0) + entry.visits,
            ...(Number(mine.typed) > 0 || entry.typed ? { typed: (Number(mine.typed) || 0) + (entry.typed ?? 0) } : {}),
            lastVisit: Math.max(Number(mine.lastVisit) || 0, entry.lastVisit)
          })
        else byUrl.set(entry.url, entry)
      }
      writeJson(P.history(), [...byUrl.values()].sort((a, b) => (Number(b.lastVisit) || 0) - (Number(a.lastVisit) || 0)).slice(0, 5000))
    }

    if (wanted.has('mods') && s.mods && Array.isArray(s.mods.mods)) {
      const current = replace
        ? { disabled: false, mods: [] as Record<string, unknown>[] }
        : readJson<{ disabled?: boolean; mods?: Record<string, unknown>[] }>(P.mods(), {})
      const list = Array.isArray(current.mods) ? current.mods : []
      const key = (m: Record<string, unknown>) => `${m.name}\n${m.css}`
      const have = new Set(list.map(key))
      const added = s.mods.mods
        .filter((m) => isObj(m) && typeof m.css === 'string' && !have.has(key(m)))
        .map((m) => ({ id: randomUUID(), name: str(m.name, 200) || 'Mod', css: str(m.css, 500_000), enabled: m.enabled !== false }))
      writeJson(P.mods(), { version: 1, disabled: replace ? s.mods.disabled === true : current.disabled === true, mods: [...list, ...added] })
    }

    if (wanted.has('notes') && Array.isArray(s.notes)) {
      const dir = P.notes()
      mkdirSync(dir, { recursive: true })
      for (const doc of s.notes) {
        if (!isObj(doc) || typeof doc.id !== 'string' || !UUID.test(doc.id) || (doc.kind !== 'note' && doc.kind !== 'easel')) continue
        const path = join(dir, `${doc.id.toLowerCase()}.json`)
        if (!replace && existsSync(path)) continue
        writeJson(path, doc)
      }
    }

    if (logins) {
      const raw = replace ? { logins: [] as Record<string, unknown>[] } : readJson<{ logins?: Record<string, unknown>[] }>(P.passwords(), {})
      const list = Array.isArray(raw.logins) ? raw.logins.filter(isObj) : []
      const key = (o: unknown, u: unknown) => `${o}\n${u}`
      const have = new Set(list.map((l) => key(l.origin, l.username)))
      const now = Date.now()
      for (const l of logins) {
        if (!/^https?:\/\/[^/]+$/i.test(l.origin) || !l.password || have.has(key(l.origin, l.username))) continue
        have.add(key(l.origin, l.username))
        list.push({
          id: randomUUID(),
          origin: l.origin,
          username: l.username,
          password: safeStorage.encryptString(l.password).toString('base64'),
          createdAt: l.createdAt ?? now,
          updatedAt: l.updatedAt ?? now
        })
      }
      writeJson(P.passwords(), { version: 1, logins: list })
    }

    if (keychain) {
      const current = replace ? [] : (readJson<{ logins?: unknown[] }>(P.passwords(), {}).logins ?? []).filter(isObj)
      const key = (l: Record<string, unknown>) => `${l.origin}\n${l.username}`
      const have = new Set(current.map(key))
      const added = keychain.filter((l) => !have.has(key(l)))
      writeJson(P.passwords(), { version: 1, logins: [...current, ...added] })
    }

    // Extensions: installed at the next launch (network), from the store.
    if (wanted.has('extensions') && Array.isArray(s.extensions)) {
      const ids = s.extensions.map((e) => (isObj(e) ? str(e.id, 64) : '')).filter((id) => /^[a-p]{32}$/.test(id))
      if (ids.length) writeJson(P.pendingExtensions(), ids)
    }
  }

  /** At launch: installs the extensions left by an import (those missing), then forgets the list. */
  async installPendingExtensions(): Promise<void> {
    const ids = readJson<unknown>(P.pendingExtensions(), null)
    if (!Array.isArray(ids)) return
    try {
      unlinkSync(P.pendingExtensions())
    } catch {
      // best-effort
    }
    const have = new Set(this.b.extensions.list().map((e) => e.id))
    for (const id of ids) if (typeof id === 'string' && !have.has(id)) await this.b.extensions.installById(id)
  }
}

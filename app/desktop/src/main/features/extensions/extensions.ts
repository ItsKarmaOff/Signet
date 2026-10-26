// Only file (with the preload) that knows the extension libraries; the interface stays neutral.
import { BrowserWindow, dialog, nativeImage, session, type ContextMenuParams, type MenuItem, type Session, type WebContents } from 'electron'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { ElectronChromeExtensions } from 'electron-chrome-extensions'
import { installChromeWebStore, installExtension, uninstallExtension } from 'electron-chrome-web-store'
import { PASSWORD_EXTENSIONS } from '@shared/types'
import type { ExtensionInfo } from '@shared/types'
import type { Browser } from '../../browser/browser'
import { peekSession } from '../../web/profiles'
import { t } from '../../app/i18n'
import { buildMenu, menuIcon } from '../../app/menuicons'

const STORE_URL = 'https://chromewebstore.google.com/'

/** Maximum wait for extensions before the first page of an isolated session. */
const READY_TIMEOUT = 3000

/** Permission lines shown at most in the install confirmation. */
const MAX_LINES = 8

/** Manifest permission → catalog sentence (keys written out in full). */
const PERMISSION_TEXT: Record<string, () => string> = {
  tabs: () => t('dialog.extension.perm.tabs'),
  history: () => t('dialog.extension.perm.history'),
  topSites: () => t('dialog.extension.perm.topSites'),
  bookmarks: () => t('dialog.extension.perm.bookmarks'),
  cookies: () => t('dialog.extension.perm.cookies'),
  webRequest: () => t('dialog.extension.perm.webRequest'),
  webRequestBlocking: () => t('dialog.extension.perm.webRequest'),
  declarativeNetRequest: () => t('dialog.extension.perm.declarativeNetRequest'),
  declarativeNetRequestWithHostAccess: () => t('dialog.extension.perm.declarativeNetRequest'),
  downloads: () => t('dialog.extension.perm.downloads'),
  clipboardRead: () => t('dialog.extension.perm.clipboardRead'),
  clipboardWrite: () => t('dialog.extension.perm.clipboardWrite'),
  notifications: () => t('dialog.extension.perm.notifications'),
  nativeMessaging: () => t('dialog.extension.perm.nativeMessaging'),
  geolocation: () => t('dialog.extension.perm.geolocation'),
  management: () => t('dialog.extension.perm.management'),
  proxy: () => t('dialog.extension.perm.proxy'),
  debugger: () => t('dialog.extension.perm.debugger'),
  privacy: () => t('dialog.extension.perm.privacy'),
  identity: () => t('dialog.extension.perm.identity'),
  desktopCapture: () => t('dialog.extension.perm.desktopCapture'),
  tabCapture: () => t('dialog.extension.perm.desktopCapture'),
  pageCapture: () => t('dialog.extension.perm.pageCapture'),
  browsingData: () => t('dialog.extension.perm.browsingData')
}

/** Permissions with no particular risk: not shown. */
const QUIET_PERMISSIONS = new Set([
  'storage',
  'unlimitedStorage',
  'activeTab',
  'alarms',
  'contextMenus',
  'scripting',
  'idle',
  'offscreen',
  'sidePanel',
  'webNavigation',
  'fontSettings',
  'search',
  'tts',
  'ttsEngine',
  'power',
  'system.display',
  'system.cpu',
  'system.memory',
  'favicon',
  'readingList',
  'sessions',
  'declarativeContent',
  'webAuthenticationProxy',
  'gcm',
  'background',
  'action',
  'commands',
  'i18n'
])

const ALL_SITES = new Set(['<all_urls>', '*://*/*', 'http://*/*', 'https://*/*', '*://*/', 'http://*/', 'https://*/'])

/** Readable sentences for what an extension asks: site access first, then known permissions, then unknown names as is. */
function describePermissions(manifest: chrome.runtime.Manifest): string[] {
  const m = manifest as chrome.runtime.Manifest & { content_scripts?: { matches?: string[] }[] }
  const names = (m.permissions ?? []).map(String)
  const patterns = new Set<string>([...(m.host_permissions ?? []).map(String), ...names.filter((p) => p.includes('://') || p === '<all_urls>')])
  for (const cs of m.content_scripts ?? []) for (const p of cs.matches ?? []) patterns.add(String(p))
  const out: string[] = []
  if ([...patterns].some((p) => ALL_SITES.has(p))) out.push(t('dialog.extension.perm.allSites'))
  else if (patterns.size) {
    const hosts = [
      ...new Set(
        [...patterns]
          .map((p) =>
            p
              .replace(/^[^:]+:\/\//, '')
              .replace(/\/.*$/, '')
              .replace(/^\*\./, '')
          )
          .filter(Boolean)
      )
    ]
    const list = hosts.slice(0, 3).join(', ')
    out.push(
      hosts.length > 3 ? t('dialog.extension.perm.hostsMore', { hosts: list, count: hosts.length - 3 }) : t('dialog.extension.perm.hosts', { hosts: list })
    )
  }
  const seen = new Set<string>()
  const unknown: string[] = []
  for (const name of names) {
    if (name.includes('://') || name === '<all_urls>' || QUIET_PERMISSIONS.has(name)) continue
    const text = PERMISSION_TEXT[name]
    if (!text) unknown.push(name)
    else if (!seen.has(text())) {
      seen.add(text())
      out.push(text())
    }
  }
  return [...out, ...unknown]
}

/** Session extensions: one library instance per session, a single shared install folder. */
export class Extensions {
  private attached = new Set<Session>()
  private loading: Promise<unknown>[] = []
  /** Sessions whose installed extensions are being loaded. */
  private pending = new Map<Session, Promise<unknown>>()
  /** False until the initial load is finished: afterwards, an addition or removal is propagated to the other sessions. */
  private mirroring = false
  /** True during our own calls to the library: its callbacks must not change the active tab. */
  private syncing = false

  constructor(private b: Browser) {}

  /** Enables extensions and the store in a session (called once per session). */
  attach(ses: Session): void {
    if (this.attached.has(ses)) return
    this.attached.add(ses)
    if (!ElectronChromeExtensions.fromSession(ses)) {
      new ElectronChromeExtensions({
        license: 'GPL-3.0',
        session: ses,
        createTab: (details) => this.createTab(ses, details),
        selectTab: (wc) => this.selectCallback(wc),
        removeTab: (wc) => this.removeCallback(wc),
        createWindow: (details) => this.createWindow(details),
        removeWindow: (win) => this.removeWindow(win)
      })
    }
    // Button icons go through this protocol, served to the interface session.
    if (ses === session.defaultSession) ElectronChromeExtensions.handleCRXProtocol(ses)

    const exts = ses.extensions
    exts.on('extension-loaded', (_e, ext) => this.mirror(ses, ext, true))
    exts.on('extension-unloaded', (_e, ext) => this.mirror(ses, ext, false))

    // Hiding of the store's banners is in the shared page script (src/preload/swipe.ts), registered on these same sessions.
    const loaded = installChromeWebStore({
      session: ses,
      beforeInstall: (details) => this.confirm(details)
    }).catch((e) => console.error('Extensions : boutique indisponible', e))
    this.loading.push(loaded)
    this.pending.set(ses, loaded)
    // Unpacked extensions: also loaded in a session opened after startup.
    void loaded.then(() => this.mirroring && this.b.unpacked.loadInto(ses))
    void loaded.finally(() => this.pending.delete(ses))
  }

  /**
   * Promise to await before the first navigation of a session whose extensions are still loading
   * (isolated partition attached on demand), or null if they are ready. Bounded so it never blocks the page.
   */
  whenReady(ses: Session): Promise<unknown> | null {
    const loaded = this.pending.get(ses)
    return loaded ? Promise.race([loaded, new Promise((r) => setTimeout(r, READY_TIMEOUT))]) : null
  }

  /**
   * Waits for the default session's extensions to load. Isolated partitions are attached
   * on demand (first `sessionFor`): the store reloads the installed extensions there, with no needless copy.
   */
  async init(): Promise<void> {
    this.attach(session.defaultSession)
    await Promise.all(this.loading)
    this.mirroring = true
    this.b.broadcast.changed({ layout: false, persist: false })
  }

  // ───────────────────────── Tabs ─────────────────────────

  /** Declares a tab's view to the library (main window, or the Little Signet's). */
  track(wc: WebContents, win: BrowserWindow | null): void {
    if (!win || win.isDestroyed() || wc.isDestroyed()) return
    this.run(() => ElectronChromeExtensions.fromSession(wc.session)?.addTab(wc, win))
  }

  /** Signals the displayed tab. */
  select(wc: WebContents | undefined): void {
    if (!wc || wc.isDestroyed()) return
    this.run(() => ElectronChromeExtensions.fromSession(wc.session)?.selectTab(wc))
  }

  /** Context menu entries added by extensions. */
  menuItems(wc: WebContents, params: ContextMenuParams): MenuItem[] {
    try {
      return ElectronChromeExtensions.fromSession(wc.session)?.getContextMenuItems(wc, params) ?? []
    } catch {
      return []
    }
  }

  private run(fn: () => void): void {
    const was = this.syncing
    this.syncing = true
    try {
      fn()
    } catch (e) {
      console.error('Extensions :', e)
    } finally {
      this.syncing = was
    }
  }

  // ───────────────────────── Library callbacks ─────────────────────────

  /** Space in which to open a tab requested by an extension of this session. */
  private spaceFor(ses: Session) {
    const { spaces } = this.b.store.state
    const active = this.b.space
    if (peekSession(active) === ses) return active
    return spaces.find((s) => peekSession(s) === ses) ?? active
  }

  private async createTab(ses: Session, details: chrome.tabs.CreateProperties): Promise<[WebContents, BrowserWindow]> {
    this.b.window.show()
    const space = this.spaceFor(ses)
    if (space.id !== this.b.space.id) this.b.spaces.switchTo(space.id)
    const id = this.b.tabs.open(details.url ?? '', { activate: details.active !== false })
    const wc = this.b.views.ensureView(id).webContents
    return [wc, this.b.win!]
  }

  private selectCallback(wc: WebContents): void {
    if (this.syncing) return
    const id = this.b.views.tabIdOfContents(wc.id)
    if (id) this.b.tabs.activate(id)
  }

  private removeCallback(wc: WebContents): void {
    if (this.syncing) return
    const id = this.b.views.tabIdOfContents(wc.id)
    // Sleep in progress: the page closes but the tab stays in the bar (reloaded when displayed).
    if (id && !this.b.views.discarding.has(id)) this.b.tabs.close(id)
  }

  /** Windows requested by an extension: a Little Signet for a popup window, otherwise a tab. */
  private async createWindow(details: chrome.windows.CreateData): Promise<BrowserWindow> {
    const url = Array.isArray(details.url) ? details.url[0] : details.url
    if (details.type === 'popup' && url) return this.b.little.open(url)
    this.b.window.show()
    if (url) this.b.tabs.open(url, { activate: true })
    return this.b.win!
  }

  private removeWindow(win: Electron.BaseWindow): void {
    if (win !== this.b.win && !win.isDestroyed()) win.close()
  }

  // ───────────────────────── Store ─────────────────────────

  /** Asks for confirmation before installing from the store. */
  private async confirm(details: {
    localizedName: string
    manifest: chrome.runtime.Manifest
    browserWindow?: BrowserWindow
  }): Promise<{ action: 'allow' | 'deny' }> {
    const lines = describePermissions(details.manifest)
    const shown = lines.length > MAX_LINES ? [...lines.slice(0, MAX_LINES - 1), t('dialog.extension.more', { count: lines.length - MAX_LINES + 1 })] : lines
    const options: Electron.MessageBoxOptions = {
      type: 'question',
      buttons: [t('dialog.extension.add'), t('dialog.cancel')],
      defaultId: 0,
      cancelId: 1,
      message: t('dialog.extension.message', { name: details.localizedName }),
      detail: lines.length ? t('dialog.extension.detail', { permissions: shown.map((l) => `• ${l}`).join('\n') }) : t('dialog.extension.detailNone')
    }
    const parent = details.browserWindow ?? this.b.win ?? undefined
    const { response } = parent ? await dialog.showMessageBox(parent, options) : await dialog.showMessageBox(options)
    return { action: response === 0 ? 'allow' : 'deny' }
  }

  openStore(): void {
    this.b.window.show()
    this.b.tabs.open(STORE_URL, { activate: true })
  }

  /** Propagates an addition or removal to the other sessions (the install folder is shared). */
  private mirror(from: Session, ext: Electron.Extension, loaded: boolean): void {
    if (!this.mirroring) return
    for (const ses of this.attached) {
      if (ses === from) continue
      const exts = ses.extensions
      const present = !!exts.getExtension(ext.id)
      if (loaded && !present) exts.loadExtension(ext.path, { allowFileAccess: true }).catch(() => {})
      else if (!loaded && present) exts.removeExtension(ext.id)
    }
    this.b.broadcast.changed({ layout: false, persist: false })
  }

  /** Installed extensions, for Settings. */
  list(): ExtensionInfo[] {
    const loaded = session.defaultSession.extensions.getAllExtensions().map((ext): ExtensionInfo => {
      const manifest = ext.manifest as chrome.runtime.Manifest
      const path = this.b.unpacked.pathOf(ext.id)
      return {
        id: ext.id,
        name: ext.name,
        version: ext.version,
        icon: iconOf(ext),
        hasAction: !!(manifest.action || manifest.browser_action),
        ...(path && { unpacked: this.b.unpacked.stateOf(path) })
      }
    })
    // Unpacked extensions that could not be loaded: shown with their error.
    const failed = this.b.unpacked
      .failed()
      .map((f): ExtensionInfo => ({ id: '', name: f.name, version: '', hasAction: false, unpacked: this.b.unpacked.stateOf(f.path) }))
    return [...loaded, ...failed].sort((a, b) => a.name.localeCompare(b.name, 'fr'))
  }

  /** Native right-click menu on an extension button (replaces the library's). */
  async showMenu(id: string): Promise<void> {
    if (!/^[a-p]{32}$/.test(id)) return
    const ext = session.defaultSession.extensions.getExtension(id) ?? [...this.attached].map((s) => s.extensions.getExtension(id)).find(Boolean)
    if (!ext) return
    const manifest = ext.manifest as chrome.runtime.Manifest
    const page = manifest.options_page ?? manifest.options_ui?.page
    const optionsUrl = page ? new URL(page, `chrome-extension://${id}/`).href : undefined
    const menu = buildMenu([
      { label: ext.name, enabled: false },
      { type: 'separator' },
      { label: t('menu.extension.options'), enabled: !!optionsUrl, click: () => this.openOptions(optionsUrl) },
      { label: t('menu.extension.hide'), click: () => this.setPinned(id, false) },
      { label: t('menu.extension.manage'), click: () => this.manage() },
      { type: 'separator' },
      { label: t('menu.extension.remove'), icon: menuIcon('delete'), click: () => void this.confirmRemove(id, ext.name) }
    ])
    menu.popup(this.b.win && !this.b.win.isDestroyed() ? { window: this.b.win } : undefined)
  }

  /** Pins or unpins an extension's button in the toolbar (it stays active in either case). */
  private setPinned(id: string, on: boolean): void {
    const { settings } = this.b.store.state
    const pinned = (settings.pinnedExtensions ?? []).filter((x) => x !== id)
    settings.pinnedExtensions = on ? [...pinned, id] : pinned
    this.b.broadcast.changed()
  }

  /**
   * Extensions button menu (puzzle piece): each extension with a button (click = open it under the button, through the
   * calling toolbar), then the pin submenu and management. Returns the extension to open, or null.
   */
  showListMenu(): Promise<string | null> {
    const { settings } = this.b.store.state
    const pinned = settings.pinnedExtensions ?? []
    const list = this.list().filter((e) => e.id && e.hasAction)
    const icon = (e: ExtensionInfo) => {
      if (!e.icon) return undefined
      const image = nativeImage.createFromDataURL(e.icon)
      return image.isEmpty() ? undefined : image.resize({ width: 16, height: 16, quality: 'best' })
    }
    return new Promise((resolve) => {
      let done = false
      const finish = (id: string | null) => {
        if (done) return
        done = true
        resolve(id)
      }
      const menu = buildMenu([
        ...(list.length
          ? list.map((e): Electron.MenuItemConstructorOptions => ({ label: e.name, icon: icon(e), click: () => finish(e.id) }))
          : [{ label: t('menu.extension.none'), enabled: false }]),
        { type: 'separator' },
        {
          label: t('menu.extension.pinned'),
          enabled: list.length > 0,
          submenu: list.map((e): Electron.MenuItemConstructorOptions => ({
            label: e.name,
            type: 'checkbox',
            checked: pinned.includes(e.id),
            click: () => this.setPinned(e.id, !pinned.includes(e.id))
          }))
        },
        { label: t('menu.extension.manage'), click: () => this.manage() }
      ])
      menu.popup({
        ...(this.b.win && !this.b.win.isDestroyed() ? { window: this.b.win } : {}),
        // Closed without a choice: an item's click sometimes arrives after closing, hence the short delay.
        callback: () => setTimeout(() => finish(null), 100)
      })
    })
  }

  private openOptions(url: string | undefined): void {
    if (!url) return
    this.b.window.show()
    this.b.tabs.open(url, { activate: true })
  }

  /** Settings › Extensions. */
  private manage(): void {
    this.b.settings.open('extensions')
  }

  private async confirmRemove(id: string, name: string): Promise<void> {
    const options: Electron.MessageBoxOptions = {
      type: 'warning',
      buttons: [t('dialog.removeExtension.confirm'), t('dialog.cancel')],
      defaultId: 1,
      cancelId: 1,
      message: t('dialog.removeExtension.message', { name }),
      detail: t('dialog.removeExtension.detail')
    }
    const { response } = this.b.win ? await dialog.showMessageBox(this.b.win, options) : await dialog.showMessageBox(options)
    if (response === 0) await this.remove(id)
  }

  async remove(id: string): Promise<void> {
    if (!/^[a-p]{32}$/.test(id)) return
    // Unpacked: removed without going through the store, which would erase its folder.
    const path = this.b.unpacked.pathOf(id)
    if (path) return this.b.unpacked.remove(path)
    for (const ses of this.attached) {
      try {
        await uninstallExtension(id, { session: ses })
      } catch (e) {
        console.error('Extensions : retrait impossible', e)
      }
    }
    this.b.broadcast.changed()
  }

  /** Installs a password manager's extension (Settings › Passwords), in all sessions. */
  /** Installs a store extension by its identifier (backup restore); failures are only logged. */
  async installById(id: string): Promise<void> {
    if (!/^[a-p]{32}$/.test(id) || session.defaultSession.extensions.getExtension(id)) return
    await Promise.all(this.loading)
    try {
      await installExtension(id, { session: session.defaultSession })
    } catch (e) {
      console.error('Extensions : installation impossible', id, e)
    }
    this.b.broadcast.changed()
  }

  async installPasswordManager(manager: string): Promise<void> {
    const entry = PASSWORD_EXTENSIONS.find((m) => m.id === manager)
    if (!entry || session.defaultSession.extensions.getExtension(entry.extensionId)) return
    await Promise.all(this.loading)
    try {
      await installExtension(entry.extensionId, { session: session.defaultSession })
    } catch (e) {
      console.error('Extensions : installation impossible', e)
      const options: Electron.MessageBoxOptions = { type: 'warning', message: t('dialog.passwordExtension.failed', { name: entry.name }) }
      if (this.b.win) await dialog.showMessageBox(this.b.win, options)
      else await dialog.showMessageBox(options)
    }
    this.b.broadcast.changed()
  }

  // ───────────────────────── Development ─────────────────────────

  /** Installs an extension by its identifier in all sessions (captures and checks only). */
  async devInstall(id: string): Promise<void> {
    await Promise.all(this.loading)
    const ses = session.defaultSession
    await installExtension(id, { session: ses })
    // Propagation to the other sessions is asynchronous: we wait until they have it.
    for (let i = 0; i < 50 && [...this.attached].some((s) => !s.extensions.getExtension(id)); i++) await new Promise((r) => setTimeout(r, 100))
    this.b.broadcast.changed()
  }

  /** One line per session: loaded extensions (development check). */
  report(): string {
    return [...this.attached]
      .map(
        (s, i) =>
          `${i === 0 ? 'default' : `isolated#${i}`}=[${s.extensions
            .getAllExtensions()
            .map((e) => `${e.name} ${e.id}`)
            .join('; ')}]`
      )
      .join(' ')
  }
}

/** Largest icon declared by the manifest, as a data URL. */
const icons = new Map<string, string | undefined>()

function iconOf(ext: Electron.Extension): string | undefined {
  const key = `${ext.id}@${ext.version}`
  if (!icons.has(key)) icons.set(key, readIcon(ext))
  return icons.get(key)
}

function readIcon(ext: Electron.Extension): string | undefined {
  const icons = (ext.manifest as chrome.runtime.Manifest).icons as Record<string, string> | undefined
  if (!icons) return undefined
  const best = Object.entries(icons).sort((a, b) => Number(b[0]) - Number(a[0]))[0]
  if (!best) return undefined
  try {
    const file = join(ext.path, best[1])
    const type = /\.svg$/i.test(file) ? 'image/svg+xml' : /\.jpe?g$/i.test(file) ? 'image/jpeg' : 'image/png'
    return `data:${type};base64,${readFileSync(file).toString('base64')}`
  } catch {
    return undefined
  }
}

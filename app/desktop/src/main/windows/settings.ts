import { app, BrowserWindow, dialog } from 'electron'
import { setDefaultBrowser, windowChrome } from '../app/platform'
import { APP_ICONS, IMPORT_KINDS } from '@shared/types'
import type { BrowsingDataKind, Settings, SettingsInfo } from '@shared/types'
import { iconPreview, isAppIcon, setAppIcon } from '../app/appicon'
import type { Browser } from '../browser/browser'
import { IMPORT_SOURCES, availableSources, runImport } from '../features/importers'
import { allSessions, reapplyLanguages, reapplySpellcheck } from '../web/profiles'
import { normalizeLanguages } from '../web/languages'
import type { ImportSource } from '../features/importers'
import { loadRoute, preloadPath, setSearchEngine } from '../core/urls'
import { setUiLanguage, t } from '../app/i18n'
import { applyTheme, broadcastAppearance } from '../app/appearance'
import { isManager } from '../features/passwords/passwords'
import { setShortcutRecording } from '../app/menu'
import { applyCleaned, cleanSettings } from '../core/store'
import { syncQuitHud } from '../app/quit'
import { pushUi } from '../ipc/push'
import { downloadDir } from '../web/downloads'
import { applySecureDns, hasProxySettings, isDnsMode } from '../features/engine'
import { isDohTemplate } from '@shared/dns'

const SLOW_TTL = 2000
const KINDS: BrowsingDataKind[] = ['history', 'archive', 'downloads', 'cache', 'cookies', 'permissions']

/** Settings window: a single instance, reused as long as it is open. */
export class SettingsWindow {
  private win: BrowserWindow | null = null
  private ready = false
  private last = ''
  /** Expensive parts of `info()` (disk, icons, extensions): re-read at most every `SLOW_TTL` ms. */
  private slow: Pick<SettingsInfo, 'defaultBrowser' | 'appIcons' | 'importSources' | 'extensions'> | null = null
  private slowAt = 0
  private slowTimer: NodeJS.Timeout | null = null

  constructor(private b: Browser) {}

  /** Opens the window, on a section if requested (simulated click on the sidebar entry). */
  open(section?: string): void {
    const goTo = (win: BrowserWindow) => {
      if (!section || !/^[a-z]+$/.test(section)) return
      const click = `document.querySelector('[data-section="${section}"]')?.click()`
      for (const delay of [0, 400, 900])
        setTimeout(() => {
          if (!win.isDestroyed()) win.webContents.executeJavaScript(click).catch(() => {})
        }, delay)
    }
    if (this.win && !this.win.isDestroyed()) {
      if (this.win.isMinimized()) this.win.restore()
      this.win.show()
      this.win.focus()
      goTo(this.win)
      return
    }
    const win = new BrowserWindow({
      width: 780,
      height: 600,
      minWidth: 640,
      minHeight: 460,
      title: t('window.settings'),
      ...windowChrome({ lights: { x: 16, y: 18 }, vibrancy: true }),
      webPreferences: { preload: preloadPath, sandbox: true, contextIsolation: true, nodeIntegration: false }
    })
    this.win = win
    this.ready = false
    this.last = ''
    this.slow = null
    // Links (repository, for example) open in a tab of the main window.
    win.webContents.setWindowOpenHandler(({ url }) => {
      if (/^https?:\/\//i.test(url)) {
        this.b.window.show()
        this.b.tabs.open(url, { activate: true })
      }
      return { action: 'deny' }
    })
    win.webContents.on('did-finish-load', () => {
      this.ready = true
      this.push(true)
      setTimeout(() => this.push(true), 150) // gives React time to subscribe
    })
    // The default browser choice happens in a system window: we re-read on return.
    win.on('focus', () => this.push(false, true))
    // Shortcut capture interrupted (other window, closing): the menu shortcuts come back.
    win.on('blur', () => setShortcutRecording(this.b, false))
    win.on('closed', () => {
      setShortcutRecording(this.b, false)
      this.win = null
      this.slow = null
      if (this.slowTimer) clearTimeout(this.slowTimer)
      this.slowTimer = null
    })
    loadRoute(win.webContents, '#settings')
    goTo(win)
  }

  /** Window title in the current language. */
  retitle(): void {
    if (this.win && !this.win.isDestroyed()) this.win.setTitle(t('window.settings'))
  }

  private readSlow(): NonNullable<SettingsWindow['slow']> {
    const available = new Map(availableSources().map((s) => [s.id, s.kinds]))
    return {
      defaultBrowser: app.isDefaultProtocolClient('http') && app.isDefaultProtocolClient('https'),
      appIcons: APP_ICONS.map((i) => ({ id: i.id, name: t(`appIcon.${i.id}`), preview: iconPreview(i.id) })),
      importSources: IMPORT_SOURCES.map((s) => ({ id: s.id, title: s.name, available: available.has(s.id), kinds: available.get(s.id) ?? [] })),
      extensions: this.b.extensions.list()
    }
  }

  /** Expensive parts, cached; `fresh` (focus, forced send) re-reads them, otherwise at most every `SLOW_TTL` ms (deferred final re-read). */
  private slowInfo(fresh: boolean): NonNullable<SettingsWindow['slow']> {
    const now = Date.now()
    if (!this.slow || fresh) {
      this.slow = this.readSlow()
      this.slowAt = now
    } else if (now - this.slowAt >= SLOW_TTL) {
      this.slow = this.readSlow()
      this.slowAt = now
    } else if (!this.slowTimer) {
      // A change (extension installed…) that happened during the cache window is picked up right after.
      this.slowTimer = setTimeout(
        () => {
          this.slowTimer = null
          this.push()
        },
        SLOW_TTL - (now - this.slowAt) + 20
      )
    }
    return this.slow
  }

  info(fresh = false): SettingsInfo {
    const { state } = this.b.store
    return {
      settings: state.settings,
      version: app.getVersion(),
      packaged: app.isPackaged,
      ...this.slowInfo(fresh),
      boosts: Object.values(state.boosts).sort((x, y) => y.updatedAt - x.updatedAt),
      counts: {
        history: this.b.history.size,
        archive: state.archive.length,
        downloads: state.downloads.length,
        permissions: this.b.permissions.listAll().length
      },
      hue: this.b.space.hue,
      downloadDir: downloadDir(state.settings),
      hardwareAccelerationActive: app.isHardwareAccelerationEnabled(),
      proxySettings: hasProxySettings()
    }
  }

  /** "Choose a folder" dialog, opened on the current folder; the choice becomes the downloads folder. */
  async chooseDownloadDir(): Promise<void> {
    const options: Electron.OpenDialogOptions = {
      title: t('downloads.chooseDir'),
      buttonLabel: t('downloads.chooseDirButton'),
      defaultPath: downloadDir(this.b.store.state.settings),
      properties: ['openDirectory', 'createDirectory']
    }
    const win = this.win && !this.win.isDestroyed() ? this.win : null
    const res = win ? await dialog.showOpenDialog(win, options) : await dialog.showOpenDialog(options)
    const dir = res.filePaths[0]
    if (res.canceled || !dir) return
    this.update({ downloadDir: dir })
  }

  /** Sends the state to the window; with no change since the last send, does nothing. */
  push(force = false, fresh = force): void {
    const wc = this.win && !this.win.isDestroyed() ? this.win.webContents : null
    if (!wc || wc.isDestroyed() || !this.ready) return
    const info = this.info(fresh)
    const json = JSON.stringify(info)
    if (!force && json === this.last) return
    this.last = json
    pushUi(wc, 'settings:info', info)
  }

  update(patch: Partial<Settings>): void {
    const { settings } = this.b.store.state
    const fonts = JSON.stringify([settings.fontSize, settings.minimumFontSize])
    const zoom = settings.defaultZoom
    // Fields to validate (durations, local names, shortcuts, extensions): see `cleanSettings`.
    applyCleaned(settings, cleanSettings(patch, settings))
    // The search engine (value already validated by `cleanSettings`) is used right away for the next inputs.
    setSearchEngine(settings.searchEngine)
    // Font sizes only reach new views: hidden tabs go to sleep and come back with them; displayed ones keep theirs until reopened.
    if (JSON.stringify([settings.fontSize, settings.minimumFontSize]) !== fonts) this.b.sleep.discardHidden()
    // Default zoom: open pages follow now (except the sites the user zoomed).
    if (settings.defaultZoom !== zoom) this.b.pageZoom.applyAll()
    // HTTPS-only mode applies from the next navigation.
    if (typeof patch.httpsOnly === 'boolean') {
      if (patch.httpsOnly) settings.httpsOnly = true
      else delete settings.httpsOnly
    }
    if (typeof patch.peekExternalLinks === 'boolean') settings.peekExternalLinks = patch.peekExternalLinks
    if (typeof patch.autoPip === 'boolean') settings.autoPip = patch.autoPip
    if (typeof patch.askDownloadPath === 'boolean') settings.askDownloadPath = patch.askDownloadPath
    if (patch.newTabPosition === 'top' || patch.newTabPosition === 'bottom') settings.newTabPosition = patch.newTabPosition
    if (typeof patch.confirmQuit === 'boolean') {
      settings.confirmQuit = patch.confirmQuit
      syncQuitHud()
    }
    if (typeof patch.showBookmarksBar === 'boolean') settings.showBookmarksBar = patch.showBookmarksBar
    if (typeof patch.swipeNavigation === 'boolean') settings.swipeNavigation = patch.swipeNavigation
    if (typeof patch.tabPreview === 'boolean') {
      settings.tabPreview = patch.tabPreview
      if (!patch.tabPreview) this.b.previews.release()
    }
    if (typeof patch.spellcheck === 'boolean' && patch.spellcheck !== (settings.spellcheck !== false)) {
      settings.spellcheck = patch.spellcheck
      reapplySpellcheck(patch.spellcheck)
    }
    if (isManager(patch.passwordManager) && patch.passwordManager !== (settings.passwordManager ?? 'signet')) {
      settings.passwordManager = patch.passwordManager
      this.b.passwords.refreshAll() // the key button only exists with Signet's password vault
    }
    if (typeof patch.offerSavePasswords === 'boolean') settings.offerSavePasswords = patch.offerSavePasswords
    if (typeof patch.devMode === 'boolean' && patch.devMode !== (settings.devMode === true)) {
      settings.devMode = patch.devMode
      if (!patch.devMode) this.b.devMode.clearAll()
      this.b.devMode.syncNetworkWatch()
    }
    if (typeof patch.devKeepOutOfHistory === 'boolean') settings.devKeepOutOfHistory = patch.devKeepOutOfHistory
    if (typeof patch.jsonViewer === 'boolean') settings.jsonViewer = patch.jsonViewer
    // Graphics acceleration: saved here, applied at the next launch (index.ts, before `ready`).
    if (typeof patch.hardwareAcceleration === 'boolean') {
      if (patch.hardwareAcceleration) delete settings.hardwareAcceleration
      else settings.hardwareAcceleration = false
    }
    // Secure DNS: applied right away, for the whole app.
    const dns = JSON.stringify([settings.secureDns, settings.secureDnsServer])
    if (isDnsMode(patch.secureDns)) settings.secureDns = patch.secureDns
    if (isDohTemplate(patch.secureDnsServer)) settings.secureDnsServer = patch.secureDnsServer
    if (JSON.stringify([settings.secureDns, settings.secureDnsServer]) !== dns) applySecureDns(settings)
    if (typeof patch.checkUpdates === 'boolean' && patch.checkUpdates !== (settings.checkUpdates !== false)) {
      settings.checkUpdates = patch.checkUpdates
      this.b.updates.sync()
    }
    // List of never-saved sites: they can only be removed from Settings (adding goes through the prompt).
    if (Array.isArray(patch.neverSavePasswords)) {
      const kept = new Set(patch.neverSavePasswords.filter((o): o is string => typeof o === 'string'))
      settings.neverSavePasswords = (settings.neverSavePasswords ?? []).filter((o) => kept.has(o))
    }
    if (typeof patch.compactMode === 'boolean') this.b.shell.setCompact(patch.compactMode)
    if (patch.tabLayout === 'vertical' || patch.tabLayout === 'horizontal') this.b.shell.setTabLayout(patch.tabLayout)
    if (patch.sidebarSide === 'left' || patch.sidebarSide === 'right') settings.sidebarSide = patch.sidebarSide
    if (patch.theme === 'system' || patch.theme === 'light' || patch.theme === 'dark') {
      settings.theme = patch.theme
      applyTheme(settings)
    }
    // Views appearance: only pushes if a value changes.
    const look = JSON.stringify([settings.tint, settings.density, settings.textSize, settings.transparency])
    if (patch.tint === 'subtle' || patch.tint === 'normal' || patch.tint === 'strong') settings.tint = patch.tint
    if (patch.density === 'compact' || patch.density === 'normal' || patch.density === 'roomy') settings.density = patch.density
    if (patch.textSize === 'small' || patch.textSize === 'normal' || patch.textSize === 'large') settings.textSize = patch.textSize
    if (typeof patch.transparency === 'boolean') settings.transparency = patch.transparency
    if (JSON.stringify([settings.tint, settings.density, settings.textSize, settings.transparency]) !== look) broadcastAppearance(settings)
    if (typeof patch.adblock === 'boolean' && patch.adblock !== settings.adblock) {
      settings.adblock = patch.adblock
      void this.b.adblock.sync()
    }
    if (typeof patch.appIcon === 'string' && isAppIcon(patch.appIcon) && patch.appIcon !== settings.appIcon) {
      setAppIcon(this.b, patch.appIcon)
    }
    if (patch.uiLanguage === 'system' || patch.uiLanguage === 'en' || patch.uiLanguage === 'fr') {
      if (patch.uiLanguage === 'system') delete settings.uiLanguage
      else settings.uiLanguage = patch.uiLanguage
      setUiLanguage(settings.uiLanguage)
    }
    // Empty list = go back to the system languages.
    if (Array.isArray(patch.languages)) {
      const next = normalizeLanguages(patch.languages)
      if (next.join() !== settings.languages.join()) {
        settings.languages = next
        reapplyLanguages(next)
        this.b.shell.reloadVisible()
      }
    }
    this.b.broadcast.changed()
  }

  setDefaultBrowser(): void {
    setDefaultBrowser()
    this.b.broadcast.changed()
  }

  /** Command bar: opens Settings › Import with the choice window of this source (simulated click on its button). */
  openImport(id: string): void {
    if (!/^[a-z]+$/.test(id)) return
    this.open('import')
    // One click per request, even if several attempts go through (the page may still be loading).
    const token = `${id}-${Date.now()}`
    const click = `(() => { if (window.__signetImport === '${token}') return; const b = document.querySelector('[data-import="${id}"]'); if (b) { window.__signetImport = '${token}'; b.click() } })()`
    for (const delay of [500, 1000, 1500])
      setTimeout(() => {
        if (this.win && !this.win.isDestroyed()) this.win.webContents.executeJavaScript(click).catch(() => {})
      }, delay)
  }

  runImport(id: string, kinds: unknown): Promise<void> {
    const picked = Array.isArray(kinds) ? IMPORT_KINDS.filter((k) => kinds.includes(k)) : []
    return IMPORT_SOURCES.some((s) => s.id === id) && picked.length ? runImport(this.b, id as ImportSource, picked) : Promise.resolve()
  }

  async clearData(kinds: BrowsingDataKind[]): Promise<void> {
    const wanted = new Set(kinds.filter((k) => KINDS.includes(k)))
    const { state } = this.b.store
    if (wanted.has('history')) this.b.history.clear()
    if (wanted.has('archive')) state.archive = []
    if (wanted.has('downloads')) this.b.downloads.clear()
    if (wanted.has('cache')) for (const ses of allSessions()) await ses.clearCache()
    if (wanted.has('cookies')) for (const ses of allSessions()) await ses.clearStorageData()
    if (wanted.has('permissions')) this.b.permissions.reset()
    this.b.store.schedule()
    this.b.broadcast.changed()
  }
}

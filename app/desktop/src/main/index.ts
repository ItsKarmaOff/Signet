import { app, BrowserWindow } from 'electron'
import { Browser } from './browser/browser'
import { History } from './core/history'
import { applyAppIcon, warmIconPreviews } from './app/appicon'
import { buildAppMenu } from './app/menu'
import { Store } from './core/store'
import { broadcastLocale, onLocaleChange } from './app/i18n'
import { applyTheme } from './app/appearance'
import { startupLanguage } from './web/languages'
import { registerInternalScheme } from './web/internal'
import { initQuit, isQuitHud } from './app/quit'
import { guardUiViews } from './web/guard'
import { LAUNCH_ARG } from './features/webapps'
import { IS_MAC } from './app/platform'
import { reducedUserAgent } from './web/identity'
import { registerIpc } from './ipc'
import { setSearchEngine } from './core/urls'

app.setName('Signet')
// Google refuses sign-in to browsers that identify as Electron.
// Reduced version (`Chrome/156.0.0.0`), like Chrome and CEF (identity.ts).
app.userAgentFallback = reducedUserAgent(app.userAgentFallback)
// Main language (navigator.language): set before `ready`, effective at next launch if it changes.
app.commandLine.appendSwitch('lang', startupLanguage())

// The `signet` scheme must be declared before `ready`.
registerInternalScheme()
// UI views: navigation locked to the app's UI.
guardUiViews()

let browser: Browser | null = null

// Development: an excess listener on a webContents is reported with the view concerned and where it was added
// (Node otherwise only prints the message).
if (!app.isPackaged) {
  process.on('warning', (w) => {
    if (w.name !== 'MaxListenersExceededWarning') return
    const emitter = (w as Error & { emitter?: { getType?: () => string; getURL?: () => string; isDestroyed?: () => boolean } }).emitter
    const where = emitter?.getType && !emitter.isDestroyed?.() ? ` — ${emitter.getType()} ${emitter.getURL?.() ?? ''}` : ''
    console.warn(`[listeners]${where}\n${w.stack ?? w.message}`)
  })
}
// Quit confirmation: listens to the keys of all views as soon as they are created.
initQuit(() => browser?.store.state.settings.confirmQuit !== false)
const pendingUrls: string[] = []
/** Installed app launches received before the browser is ready. */
const pendingApps: string[] = []

if (!app.requestSingleInstanceLock()) {
  app.quit()
} else {
  start()
}

function isWebUrl(arg: string): boolean {
  return /^https?:\/\//i.test(arg)
}

/** Installed app launcher: `signet://webapp/<id>` (packaged version) or `--signet-webapp=<id>` (development). */
function isAppLaunch(arg: string): boolean {
  return /^signet:\/\/webapp\//i.test(arg) || arg.startsWith(LAUNCH_ARG)
}

function launchApp(arg: string): void {
  if (browser) browser.webApps.launchFrom(arg)
  else pendingApps.push(arg)
}

function openExternal(url: string): void {
  if (browser) browser.tabs.openExternal(url)
  else pendingUrls.push(url)
}

/** Writes everything pending to disk. */
function flushAll(): void {
  browser?.views.saveAllHistories()
  browser?.store.flush()
  browser?.history.flush()
  browser?.passwords.flush()
  browser?.notes.flush()
  browser?.mods.flush()
  browser?.webApps.flush()
  browser?.link.flush()
}

function start(): void {
  app.on('second-instance', (_e, argv) => {
    const launch = argv.find(isAppLaunch)
    const url = argv.find(isWebUrl)
    if (launch) launchApp(launch)
    else if (url) openExternal(url)
    else browser?.window.show()
  })
  app.on('open-url', (event, url) => {
    event.preventDefault()
    if (isAppLaunch(url)) launchApp(url)
    else if (isWebUrl(url)) openExternal(url)
  })
  // Final quit: the mDNS announcement is withdrawn and the link port closed.
  app.on('will-quit', () => void browser?.link.stop())
  app.on('before-quit', () => {
    if (browser) browser.pip.quitting = true // otherwise the mini player cancels the quit
    flushAll()
  })
  // Windows: on shutdown or logoff, before-quit does not fire.
  app.on('browser-window-created', (_e, win) => win.on('session-end', flushAll))
  app.on('window-all-closed', () => {})
  // Linux and Windows: no Dock to reopen the app, it quits when its last visible window closes. Otherwise
  // it would run without a window, and relaunching the app (even a newer version) would only wake it up.
  // A minimized window counts; hidden windows kept ready (quit bubble…) do not.
  if (!IS_MAC) {
    app.on('browser-window-created', (_e, win) =>
      win.once('closed', () =>
        setImmediate(() => {
          if (!BrowserWindow.getAllWindows().some((w) => !w.isDestroyed() && !isQuitHud(w) && (w.isVisible() || w.isMinimized()))) app.quit()
        })
      )
    )
  }
  app.on('activate', () => browser?.window.open())

  void app.whenReady().then(async () => {
    handleShutdownSignals()
    // Developer tools: loaded on demand, never in the packaged app.
    const dev = app.isPackaged ? null : await import('./dev')
    browser = new Browser(new Store(), new History())
    // End-to-end tests (Playwright): exposes the browser and cuts unneeded network. Never in the packaged app.
    if (process.env.SIGNET_E2E === '1' && !app.isPackaged) {
      ;(globalThis as { __signet?: Browser }).__signet = browser
      browser.store.state.settings.adblock = false
    }
    applyTheme(browser.store.state.settings)
    setSearchEngine(browser.store.state.settings.searchEngine)
    registerIpc(browser, !app.isPackaged)
    browser.link.start()
    // The menu is built by Browser's first apply() (triggered by openWindow, on the next tick).
    // Language change: menu, window titles, then all UI views.
    onLocaleChange(() => {
      if (!browser) return
      buildAppMenu(browser)
      browser.settings.retitle()
      browser.welcome.retitle()
      browser.passkeys.retitle()
      browser.library.retitle()
      browser.boosts.retitle()
      broadcastLocale()
    })
    dev?.setup(browser.store)
    // Extensions left by a backup import: installed once extensions are loaded.
    void browser.extensions
      .init()
      .then(() => browser?.backup.installPendingExtensions())
      .then(() => browser?.unpacked.init())
    // Launched by an app's launcher: only its window opens (like Chrome); the Dock or ⌘N bring back the main window.
    const launches = [...pendingApps, ...process.argv.filter(isAppLaunch)]
    pendingApps.length = 0
    if (launches.length) {
      buildAppMenu(browser)
      for (const arg of launches) browser.webApps.launchFrom(arg)
    } else {
      browser.window.open()
      // First launch: the intro opens on the main window (end-to-end tests turn it off with SIGNET_NO_WELCOME).
      if (browser.welcome.pending && !(process.env.SIGNET_NO_WELCOME === '1' && !app.isPackaged)) browser.welcome.open()
    }
    browser.liveFolders.start()
    browser.updates.sync()
    // History read after the window opens, before the palette's first search.
    setTimeout(() => browser?.history.preload(), 300)
    // Dock icon: already the bundle's when it is the default icon in the packaged version; otherwise applied after the window opens.
    // Linux: always (window icon, AppImage shortcut).
    const appIcon = browser.store.state.settings.appIcon
    if (!app.isPackaged || appIcon !== 'aurore' || process.platform === 'linux') setImmediate(() => applyAppIcon(appIcon))
    setTimeout(() => void warmIconPreviews(), 3000)
    void browser.adblock.sync()
    dev?.start(browser)
    for (const url of [...pendingUrls, ...process.argv.filter(isWebUrl)]) browser.tabs.openExternal(url)
    pendingUrls.length = 0
  })
}

/**
 * Ctrl+C in the terminal (or `kill`): normal exit. Electron receives the signal several times (the terminal, then
 * electron-vite and npm which relay it) and its own handler cancels the quit in progress on the second one, with code 1.
 * Set once the app is ready: before that, Chromium installs its own on top. A signal repeated more than a second after
 * the first forces the exit if the quit blocks.
 */
function handleShutdownSignals(): void {
  let first = 0
  for (const signal of ['SIGINT', 'SIGTERM'] as const) {
    process.on(signal, () => {
      first ||= Date.now()
      if (Date.now() - first > 1000) app.exit(0)
      else app.quit()
    })
  }
}

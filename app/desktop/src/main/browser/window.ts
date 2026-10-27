import { BrowserWindow, screen } from 'electron'
import { windowChrome } from '../app/platform'
import { logUiError } from '../app/uilog'
import { loadRoute, preloadPath } from '../core/urls'
import type { Store } from '../core/store'
import type { Develop } from '../devtools/develop'
import { denyWindowOpen } from '../web/guard'
import { captureNativeHints } from '../web/identity'
import { emulateLanguages } from '../web/languages'
import { SLOW_PERSIST_MS, type Changed } from './broadcast'
import type { Compact } from './compact'
import type { FindBar } from './find'
import type { HtmlFullscreen } from './fullscreen'
import type { Floating } from './floating'
import type { Layout } from './layout'
import type { Media } from './media'
import type { Overlay } from './overlay'
import type { Peek } from './peek'
import type { TabPreviews } from './preview'
import type { TabSleep } from './sleep'
import type { Undo } from './undo'
import type { Views } from './views'

/** Idle warm-up of the find bar after the window opens. */
const FIND_WARM_MS = 4000

/** Saved size and position, brought back onto a connected screen if the former one is gone. */
function onScreen<T extends { x?: number; y?: number; width: number; height: number }>(saved: T | null | undefined): T | undefined {
  if (!saved) return undefined
  if (typeof saved.x !== 'number' || typeof saved.y !== 'number') return saved
  const r = { x: saved.x, y: saved.y, width: saved.width, height: saved.height }
  const area = screen.getDisplayMatching(r).workArea
  const width = Math.min(r.width, area.width)
  const height = Math.min(r.height, area.height)
  const visible = r.x < area.x + area.width - 80 && r.x + width > area.x + 80 && r.y >= area.y - 10 && r.y + height <= area.y + area.height + 10
  if (visible) return { ...saved, width, height }
  return { ...saved, width, height, x: area.x + Math.round((area.width - width) / 2), y: area.y + Math.round((area.height - height) / 2) }
}

export interface MainWindowDeps {
  store: Store
  /** Carries the main window (`win`), read by all the other parts. */
  host: { win: BrowserWindow | null }
  layout: Pick<Layout, 'trafficLights' | 'onOpen' | 'onClosed'>
  overlay: Pick<Overlay, 'create' | 'ensureLoaded' | 'reset' | 'destroyView'>
  compact: Pick<Compact, 'create' | 'destroy'>
  find: Pick<FindBar, 'create' | 'destroy'>
  fullscreen: Pick<HtmlFullscreen, 'onWindowLeft' | 'reset'>
  floating: Pick<Floating, 'reset'>
  sleep: Pick<TabSleep, 'start' | 'stop'>
  views: Pick<Views, 'all' | 'destroyView'>
  peek: Pick<Peek, 'reset'>
  undo: Pick<Undo, 'reset'>
  previews: Pick<TabPreviews, 'destroy'>
  media: Pick<Media, 'onWindowBlur' | 'onWindowFocus' | 'reset'>
  develop: Pick<Develop, 'reapplyAll'>
  changed: Changed
  /** Three-finger gesture: neighboring space (+1: next). */
  stepSpace: (step: number) => void
}

/** Main window: creation, listeners, recovery after a UI crash, and teardown of everything that depends on it. */
export class MainWindow {
  private warmTimers: NodeJS.Timeout[] = []

  constructor(private readonly d: MainWindowDeps) {}

  open(): void {
    const { store, host } = this.d
    if (host.win) return this.show()
    const bounds = onScreen(store.state.bounds)
    const win = new BrowserWindow({
      width: bounds?.width ?? 1280,
      height: bounds?.height ?? 820,
      x: bounds?.x,
      y: bounds?.y,
      minWidth: 700,
      minHeight: 480,
      ...windowChrome({ lights: this.d.layout.trafficLights(), vibrancy: true }),
      webPreferences: { preload: preloadPath, sandbox: true, contextIsolation: true, nodeIntegration: false }
    })
    host.win = win
    this.d.layout.onOpen()

    const overlay = this.d.overlay.create()
    // Hidden UI views: created on demand or warmed up when idle (each loads the bundle).
    if (store.state.settings.compactMode) this.d.compact.create()
    this.warmTimers.push(setTimeout(() => host.win && this.d.find.create(), FIND_WARM_MS))
    this.d.sleep.start()

    const remember = () => {
      if (!win.isFullScreen() && !win.isMaximized()) store.state.bounds = win.getBounds()
    }
    win.on('resize', () => {
      remember()
      this.d.changed({ slow: true })
    })
    // Moving the window changes neither the layout nor the displayed state: we only keep the position.
    win.on('move', () => {
      remember()
      store.schedule(SLOW_PERSIST_MS)
    })
    // The user leaves system fullscreen (green button, shortcut) while a page is fullscreen: the page leaves it too.
    win.on('leave-full-screen', () => this.d.fullscreen.onWindowLeft())
    win.on('closed', () => this.onClosed())
    // Leaving the window (other app, other desktop, other window) with a video playing: automatic mini player.
    win.on('blur', () => this.d.media.onWindowBlur())
    win.on('minimize', () => this.d.media.onWindowBlur())
    win.on('focus', () => this.d.media.onWindowFocus())
    // Three-finger gesture (if enabled in macOS): neighboring space, leftward = next.
    win.on('swipe', (_e, direction) => {
      if (direction === 'left') this.d.stepSpace(1)
      else if (direction === 'right') this.d.stepSpace(-1)
    })

    denyWindowOpen(win.webContents)
    denyWindowOpen(overlay.webContents)
    // Window UI gone (process crash, failed load): we reload it rather than leave the
    // window empty around the pages, which stay displayed.
    win.webContents.on('render-process-gone', (_e, details) => {
      logUiError('main', `UI process gone: ${details.reason}`)
      if (details.reason !== 'clean-exit') setTimeout(() => !win.isDestroyed() && loadRoute(win.webContents, ''), 500)
    })
    win.webContents.on('unresponsive', () => logUiError('main', 'UI is unresponsive'))
    let loadRetries = 0
    win.webContents.on('did-fail-load', (_e, code, desc, _url, isMainFrame) => {
      if (!isMainFrame || code === -3) return
      logUiError('main', `UI load failed: ${code} ${desc}`)
      if (loadRetries++ < 5) setTimeout(() => !win.isDestroyed() && loadRoute(win.webContents, ''), 1000)
    })
    win.webContents.on('did-finish-load', () => (loadRetries = 0))
    loadRoute(win.webContents, '')
    // The overlay (palette, popovers) only loads its bundle once the main window is shown.
    win.webContents.once('did-finish-load', () => this.d.overlay.ensureLoaded())
    // Native client hints collected in the UI, then applied to already-open tabs (see identity.ts).
    win.webContents.once('did-finish-load', () => {
      void captureNativeHints(win.webContents).then((known) => {
        if (!known) return
        const { languages } = store.state.settings
        for (const v of this.d.views.all()) emulateLanguages(v.webContents, languages)
        this.d.develop.reapplyAll()
      })
    })
    this.d.changed()
  }

  show(): void {
    const win = this.d.host.win
    if (!win) return this.open()
    if (win.isMinimized()) win.restore()
    win.show()
    win.focus()
  }

  /** Reloads the main window's UI (View menu): pages and state do not move. */
  reloadInterface(): void {
    const wc = this.d.host.win?.webContents
    if (!wc || wc.isDestroyed()) return
    logUiError('main', 'UI reload requested')
    wc.reload()
  }

  private onClosed(): void {
    const { store, host, views } = this.d
    host.win = null
    this.d.layout.onClosed()
    this.d.fullscreen.reset()
    this.d.sleep.stop()
    for (const t of this.warmTimers) clearTimeout(t)
    this.warmTimers = []
    this.d.peek.reset()
    this.d.overlay.reset()
    for (const id of Object.keys(store.state.tabs)) {
      if (!store.detached.has(id)) views.destroyView(id)
    }
    this.d.undo.reset()
    this.d.previews.destroy()
    this.d.overlay.destroyView()
    this.d.find.destroy()
    this.d.floating.reset()
    this.d.compact.destroy()
    this.d.media.reset()
    store.flush()
  }
}

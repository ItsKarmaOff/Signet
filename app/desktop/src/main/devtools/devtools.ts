import { BrowserWindow, nativeTheme, screen, WebContentsView, type WebContents } from 'electron'
import { windowChrome } from '../app/platform'
import type { DevToolsDock, DevToolsPrefs, DevToolsWindowState, Rect } from '@shared/types'
import type { Browser } from '../browser/browser'
import { GAP } from '@signet/shared/split'
import { t } from '../app/i18n'
import { buildMenu } from '../app/menuicons'
import { denyWindowOpen } from '../web/guard'
import { loadRoute, preloadPath } from '../core/urls'
import { pushUi } from '../ipc/push'

/** Height of the tools card header (same value as the renderer's `DEVTOOLS_BAR`). */
export const DEVTOOLS_BAR = 36
const MIN_TOOLS_W = 320
const MIN_TOOLS_H = 160
const MIN_PAGE_W = 320
const MIN_PAGE_H = 200
const DEFAULT_PREFS: DevToolsPrefs = { dock: 'right', width: 480, height: 320 }

interface Entry {
  view: WebContentsView
  page: WebContents
  dock: DevToolsDock
  width: number
  height: number
  /** Detached tools window. */
  win: BrowserWindow | null
  /** Panel card at the last computation (to bound the size during a drag). */
  card: Rect | null
  /** Give the tools focus at the next layout computation. */
  focus: boolean
  /** Tools interface loaded (`devtools-opened`); before that, a requested panel waits in `panel`. */
  loaded: boolean
  panel: string | null
  off: () => void
}

/** Part of a panel card taken by the built-in tools. */
export interface DockedTools {
  view: WebContentsView
  dock: 'right' | 'bottom'
  card: Rect
  rect: Rect
  divider: Rect
}

/** Tools size bounded by the available room: each side keeps its minimum, otherwise half and half. */
function clampSize(size: number, total: number, minTools: number, minPage: number): number {
  const room = total - GAP
  if (room < minTools + minPage) return Math.round(room / 2)
  return Math.round(Math.min(Math.max(size, minTools), room - minPage))
}

/**
 * Developer tools of each tab, in a view that Signet creates and places itself
 * (`setDevToolsWebContents`): a card to the right of or below the page, or a separate window. The same view moves
 * from one location to the other without reopening the tools.
 */
export class DevTools {
  private entries = new Map<string, Entry>()

  constructor(private b: Browser) {}

  private get prefs(): DevToolsPrefs {
    return { ...DEFAULT_PREFS, ...this.b.store.state.settings.devtools }
  }

  private savePrefs(patch: Partial<DevToolsPrefs>): void {
    this.b.store.state.settings.devtools = { ...this.prefs, ...patch }
    this.b.store.schedule()
  }

  /** Tools view of a tab (development captures). */
  viewOf(tabId: string): WebContentsView | undefined {
    return this.entries.get(tabId)?.view
  }

  isOpen(tabId: string): boolean {
    return this.entries.has(tabId)
  }

  /** Tab inspected by a detached tools window. */
  tabIdOf(win: BrowserWindow | null): string | null {
    if (!win) return null
    for (const [id, e] of this.entries) if (e.win === win) return id
    return null
  }

  /** Built-in tools only go in the main window (not Little Signet, app, private window, Peek). */
  private canDock(tabId: string): boolean {
    return !this.b.store.detached.has(tabId) && tabId !== this.b.peek.tabId
  }

  toggle(tabId: string | null): void {
    if (!tabId) return
    const e = this.entries.get(tabId)
    if (!e) return void this.open(tabId)
    // Built-in tools while the tab has moved to another window: we show them instead of closing them where nobody can see.
    if (e.dock !== 'window' && !this.canDock(tabId)) return this.setDock(tabId, 'window', true)
    this.close(tabId)
  }

  open(tabId: string, dock?: DevToolsDock): Entry | null {
    const existing = this.entries.get(tabId)
    if (existing) return existing
    const page = this.b.views.viewOf(tabId)?.webContents
    if (!page || page.isDestroyed()) return null
    const prefs = this.prefs
    const want = dock ?? prefs.dock
    const view = new WebContentsView()
    view.setBackgroundColor(nativeTheme.shouldUseDarkColors ? '#282828' : '#ffffff')
    page.setDevToolsWebContents(view.webContents)
    page.openDevTools({ mode: 'detach', activate: false })

    const openUrl = (_e: Electron.Event, url: string) => void this.b.tabs.open(url)
    const retitle = () => this.refreshWindow(tabId, entry)
    const gone = () => {
      if (this.entries.get(tabId) === entry) this.close(tabId)
    }
    const opened = () => {
      entry.loaded = true
      const panel = entry.panel
      entry.panel = null
      if (panel && this.entries.get(tabId) === entry) void this.showPanel(entry, panel)
    }
    const entry: Entry = {
      view,
      page,
      dock: want !== 'window' && !this.canDock(tabId) ? 'window' : want,
      width: prefs.width,
      height: prefs.height,
      win: null,
      card: null,
      focus: true,
      loaded: false,
      panel: null,
      off: () => {
        if (page.isDestroyed()) return
        page.removeListener('devtools-open-url', openUrl)
        page.removeListener('devtools-closed', gone)
        page.removeListener('devtools-opened', opened)
        page.removeListener('destroyed', gone)
        page.removeListener('page-title-updated', retitle)
      }
    }
    page.on('devtools-open-url', openUrl)
    page.once('devtools-closed', gone)
    page.once('destroyed', gone)
    // The tools restore their last panel when loading: a panel requested earlier is only shown afterwards.
    page.once('devtools-opened', opened)
    page.on('page-title-updated', retitle)
    this.entries.set(tabId, entry)
    if (entry.dock === 'window') this.showWindow(tabId, entry)
    this.b.broadcast.patchTab(tabId, { devtools: entry.dock })
    return entry
  }

  close(tabId: string): void {
    const e = this.entries.get(tabId)
    if (!e) return
    this.entries.delete(tabId)
    e.off()
    const hadFocus = !e.view.webContents.isDestroyed() && e.view.webContents.isFocused()
    if (!e.page.isDestroyed()) e.page.closeDevTools()
    this.b.layout.detachFromMain(e.view)
    this.dropWindow(e)
    if (!e.view.webContents.isDestroyed()) e.view.webContents.close()
    this.b.broadcast.patchTab(tabId, { devtools: undefined })
    // Focus returns to the page if it was in the built-in tools.
    if (hadFocus && !e.page.isDestroyed() && this.b.layout.shownTabs.includes(tabId)) e.page.focus()
  }

  /** Closes everything (main window or app closing). */
  closeAll(): void {
    for (const id of [...this.entries.keys()]) this.close(id)
  }

  /** `forced`: fallback to a window imposed (tab outside the main window), not to be remembered as a preference. */
  setDock(tabId: string, dock: DevToolsDock, forced = false): void {
    const e = this.entries.get(tabId)
    const next = dock !== 'window' && !this.canDock(tabId) ? 'window' : dock
    if (!forced) this.savePrefs({ dock: next })
    if (!e) return void this.open(tabId, next)
    if (e.dock === next) {
      if (e.win && !e.win.isDestroyed()) e.win.focus()
      return
    }
    if (next === 'window') {
      this.b.layout.detachFromMain(e.view)
      e.dock = next
      this.showWindow(tabId, e)
    } else {
      this.dropWindow(e)
      e.dock = next
      e.focus = true
    }
    this.b.broadcast.patchTab(tabId, { devtools: next })
  }

  /** Size of the built-in tools (px, width on the right or height at the bottom); 0 = original size. `done`: end of drag, remember it. */
  setSize(tabId: string, size: number, done: boolean): void {
    const e = this.entries.get(tabId)
    if (!e || e.dock === 'window' || !Number.isFinite(size)) return
    const right = e.dock === 'right'
    const card = e.card
    let next = size > 0 ? size : right ? DEFAULT_PREFS.width : DEFAULT_PREFS.height
    if (card) next = right ? clampSize(next, card.width, MIN_TOOLS_W, MIN_PAGE_W) : clampSize(next, card.height, MIN_TOOLS_H, MIN_PAGE_H)
    if (right) e.width = next
    else e.height = next
    if (done || size <= 0) this.savePrefs(right ? { width: next } : { height: next })
    this.b.broadcast.changed({ persist: false })
  }

  /** Inspect the element under the pointer: opens the tools if needed, then selects the element in them. */
  inspect(tabId: string, x: number, y: number): void {
    let e = this.entries.get(tabId)
    if (e && e.dock !== 'window' && !this.canDock(tabId)) this.setDock(tabId, 'window', true)
    e ??= this.open(tabId) ?? undefined
    if (!e) return
    e.page.inspectElement(x, y)
    this.focusTools(e)
  }

  /** Focus on the tools: the detached window directly, otherwise at the next layout (built-in). */
  private focusTools(e: Entry): void {
    if (e.dock === 'window') {
      if (e.win && !e.win.isDestroyed()) {
        e.win.focus()
        if (!e.view.webContents.isDestroyed()) e.view.webContents.focus()
      }
      return
    }
    e.focus = true
    this.b.broadcast.changed({ persist: false })
  }

  /** Opens the tools (or shows them if open) on the console (⌥⌘J, address bar error counter). */
  showConsole(tabId: string | null): void {
    if (!tabId) return
    let e = this.entries.get(tabId)
    if (e && e.dock !== 'window' && !this.canDock(tabId)) this.setDock(tabId, 'window', true)
    e ??= this.open(tabId) ?? undefined
    if (!e) return
    this.focusTools(e)
    void this.showPanel(e, 'console')
  }

  /** Shows a tools panel, right away or as soon as their interface has finished loading. */
  private async showPanel(e: Entry, panel: string): Promise<void> {
    const wc = e.view.webContents
    if (wc.isDestroyed()) return
    if (!e.loaded) {
      e.panel = panel
      return
    }
    await wc.executeJavaScript(`window.DevToolsAPI && DevToolsAPI.showPanel(${JSON.stringify(panel)})`).catch(() => {})
  }

  /** Menu of the tools button in the address bar: location and closing. */
  showMenu(tabId: string): void {
    const e = this.entries.get(tabId)
    if (!e) return
    const dockable = this.canDock(tabId)
    const item = (dock: DevToolsDock, label: string) => ({
      label,
      type: 'checkbox' as const,
      checked: e.dock === dock,
      enabled: dock === 'window' || dockable,
      click: () => this.setDock(tabId, dock)
    })
    buildMenu([
      item('right', t('devtools.dockRight')),
      item('bottom', t('devtools.dockBottom')),
      item('window', t('devtools.dockWindow')),
      { type: 'separator' },
      { label: t('devtools.close'), click: () => this.close(tabId) }
    ]).popup(this.b.win && !this.b.win.isDestroyed() ? { window: this.b.win } : undefined)
  }

  /** Splits a panel's card: the page, and the tools on the right or at the bottom (separated by the split view gap). */
  carve(tabId: string, card: Rect): { page: Rect; tools?: DockedTools } {
    const e = this.entries.get(tabId)
    if (!e || e.dock === 'window' || e.page.isDestroyed()) return { page: card }
    e.card = card
    let page: Rect
    let tools: Rect
    let divider: Rect
    if (e.dock === 'right') {
      const w = clampSize(e.width, card.width, MIN_TOOLS_W, MIN_PAGE_W)
      page = { ...card, width: card.width - w - GAP }
      tools = { x: page.x + page.width + GAP, y: card.y, width: w, height: card.height }
      divider = { x: page.x + page.width, y: card.y, width: GAP, height: card.height }
    } else {
      const h = clampSize(e.height, card.height, MIN_TOOLS_H, MIN_PAGE_H)
      page = { ...card, height: card.height - h - GAP }
      tools = { x: card.x, y: page.y + page.height + GAP, width: card.width, height: h }
      divider = { x: card.x, y: page.y + page.height, width: card.width, height: GAP }
    }
    const rect = { x: tools.x, y: tools.y + DEVTOOLS_BAR, width: tools.width, height: Math.max(0, tools.height - DEVTOOLS_BAR) }
    return { page, tools: { view: e.view, dock: e.dock, card: tools, rect, divider } }
  }

  /** After the layout computation: focus on the tools that have just opened or changed location. */
  afterLayout(shown: readonly string[]): void {
    for (const [id, e] of this.entries) {
      if (!e.focus || e.view.webContents.isDestroyed()) continue
      if (e.dock !== 'window' && !shown.includes(id)) continue
      e.focus = false
      e.view.webContents.focus()
    }
  }

  private hostOf(page: WebContents): string {
    try {
      return new URL(page.getURL()).host
    } catch {
      return '' // address without host (internal page)
    }
  }

  private windowTitle(page: WebContents): string {
    return t('devtools.windowTitle', { site: this.hostOf(page) || page.getTitle() })
  }

  /** State of a detached tools window's header (`#devtools` route), from its `webContents`. */
  windowState(sender: WebContents): DevToolsWindowState | null {
    for (const [tabId, e] of this.entries) if (e.win && !e.win.isDestroyed() && e.win.webContents === sender) return this.stateOf(tabId, e)
    return null
  }

  private stateOf(tabId: string, e: Entry): DevToolsWindowState {
    return { tabId, site: e.page.isDestroyed() ? '' : this.hostOf(e.page) || e.page.getTitle(), dockable: this.canDock(tabId) }
  }

  /** Updates the detached window's title and header (navigation, tab moved). */
  private refreshWindow(tabId: string, e: Entry): void {
    const win = e.win
    if (!win || win.isDestroyed() || e.page.isDestroyed()) return
    win.setTitle(this.windowTitle(e.page))
    pushUi(win.webContents, 'devtools:window-state', this.stateOf(tabId, e))
  }

  /**
   * Detached tools window: hidden title bar, interface header (`#devtools` route, traffic lights
   * on the left, buttons to reattach the tools to the page) and the tools view below.
   */
  private showWindow(tabId: string, e: Entry): void {
    const saved = this.prefs.bounds
    const visible =
      saved &&
      screen.getAllDisplays().some((d) => {
        const a = d.workArea
        return saved.x < a.x + a.width && saved.x + saved.width > a.x && saved.y < a.y + a.height && saved.y + saved.height > a.y
      })
    const win = new BrowserWindow({
      ...(visible ? saved : { width: 960, height: 680 }),
      minWidth: 420,
      minHeight: 280,
      title: this.windowTitle(e.page),
      ...windowChrome({ lights: { x: 14, y: Math.round((DEVTOOLS_BAR - 14) / 2) }, background: nativeTheme.shouldUseDarkColors ? '#282828' : '#ffffff' }),
      show: false,
      webPreferences: { preload: preloadPath, sandbox: true, contextIsolation: true, nodeIntegration: false }
    })
    // The title (Window menu, Mission Control) is the tools', not the route's.
    win.on('page-title-updated', (ev) => ev.preventDefault())
    denyWindowOpen(win.webContents)
    loadRoute(win.webContents, '#devtools')
    e.win = win
    e.view.setBorderRadius(0)
    win.contentView.addChildView(e.view)
    const fit = () => {
      const { width, height } = win.getContentBounds()
      e.view.setBounds({ x: 0, y: DEVTOOLS_BAR, width, height: Math.max(0, height - DEVTOOLS_BAR) })
    }
    fit()
    win.on('resize', fit)
    const remember = () => this.savePrefs({ bounds: win.getBounds() })
    win.on('moved', remember)
    win.on('resized', remember)
    // The tab may have changed window since opening: the header says whether the tools can be reattached.
    win.on('focus', () => this.refreshWindow(tabId, e))
    // Closing the window closes the tools (the window finishes closing by itself).
    win.on('close', () => {
      if (e.win !== win) return
      e.win = null
      if (this.entries.get(tabId) === e) this.close(tabId)
    })
    win.show()
    e.view.webContents.focus()
  }

  /** Removes the view from the detached window and closes it without closing the tools. */
  private dropWindow(e: Entry): void {
    const win = e.win
    e.win = null
    if (!win || win.isDestroyed()) return
    win.contentView.removeChildView(e.view)
    win.destroy()
  }
}

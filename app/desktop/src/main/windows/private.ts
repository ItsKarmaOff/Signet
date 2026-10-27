import { BrowserWindow, session, type Rectangle, type Session, type WebContents, type WebContentsView } from 'electron'
import { IS_MAC, windowChrome } from '../app/platform'
import type { Rect } from '@shared/types'
import { randomUUID } from 'node:crypto'
import type { PrivateState } from '@shared/types'
import type { Browser } from '../browser/browser'
import { fitRect } from '../browser/fullscreen'
import { configurePrivateSession, releasePrivateSession } from '../web/profiles'
import { newTab } from '../core/store'
import { denyWindowOpen } from '../web/guard'
import { loadRoute, preloadPath } from '../core/urls'
import { t } from '../app/i18n'
import { pushUi } from '../ipc/push'

const CASCADE = 24
const PAD = 6
/** Tabs on top: tab row (40 px) and gap with the card. */
const TOP = 44
/** Address bar, at the top of the card. */
const BAR = 40

interface PrivateWin {
  win: BrowserWindow
  /** In-memory session, specific to the window: nothing is written to disk, everything disappears on close. */
  ses: Session
  tabs: string[]
  active: string | null
  /** Web view placed in the window. */
  shown: WebContentsView | null
  /** Give focus to the page on next display (after a typed address or a tab change). */
  focusPage: boolean
  lastSent: string
  /** Layout applied at the last placement (`horizontal` or `vertical:<width>`). */
  geometry: string
}

/**
 * Private browsing windows: several tabs, an address bar, an in-memory session per window.
 * No history, no saved tabs, no remembered permissions, no extensions.
 */
export class PrivateWindows {
  private windows = new Map<BrowserWindow, PrivateWin>()
  private owner = new Map<string, PrivateWin>()
  private last: Rectangle | null = null

  constructor(private b: Browser) {}

  open(url = ''): BrowserWindow {
    if (this.windows.size === 0) this.last = null
    const win = new BrowserWindow({
      width: 1100,
      height: 780,
      minWidth: 520,
      minHeight: 360,
      x: this.last ? this.last.x + CASCADE : undefined,
      y: this.last ? this.last.y + CASCADE : undefined,
      title: t('window.private'),
      ...windowChrome({ lights: this.lights(), vibrancy: true }),
      webPreferences: { preload: preloadPath, sandbox: true, contextIsolation: true, nodeIntegration: false }
    })
    this.last = win.getBounds()
    // The window title (Window menu, Mission Control) stays "Private Browsing".
    win.on('page-title-updated', (e) => e.preventDefault())

    const ses = session.fromPartition(`private-${randomUUID()}`) // without `persist:`: in memory only
    configurePrivateSession(ses)
    const pw: PrivateWin = { win, ses, tabs: [], active: null, shown: null, focusPage: true, lastSent: '', geometry: this.geometryKey() }
    this.windows.set(win, pw)
    this.add(pw, url, { activate: true })

    win.on('resize', () => this.place(pw))
    // The bar asks for its state at startup (`private:get`); a reload starts from zero.
    win.webContents.on('did-finish-load', () => (pw.lastSent = ''))
    win.on('closed', () => this.onClosed(pw))
    denyWindowOpen(win.webContents)
    loadRoute(win.webContents, '#private')
    return win
  }

  // ───────────────────────── Access ─────────────────────────

  /** True if the window is a private window. */
  has(win: BrowserWindow | null): boolean {
    return !!win && this.windows.has(win)
  }

  /** True if this session is the one of a still-open private window. */
  hasSession(ses: Session): boolean {
    for (const pw of this.windows.values()) if (pw.ses === ses) return true
    return false
  }

  /** True if the tab belongs to a private window. */
  owns(tabId: string): boolean {
    return this.owner.has(tabId)
  }

  /** True for a private tab that is not the one displayed in its window. */
  hidden(tabId: string): boolean {
    const pw = this.owner.get(tabId)
    return !!pw && pw.active !== tabId
  }

  /** Tab displayed in a private window (null if the window is not one, or if it is empty). */
  activeTabOf(win: BrowserWindow | null): string | null {
    return (win && this.windows.get(win)?.active) || null
  }

  /** Tabs of a private window, in the bar's order. */
  tabsOf(win: BrowserWindow | null): string[] {
    return [...((win && this.windows.get(win)?.tabs) ?? [])]
  }

  /** True if this `webContents` is a private window's bar. */
  ownsBar(wc: WebContents): boolean {
    return this.has(BrowserWindow.fromWebContents(wc))
  }

  tabIdForBar(wc: WebContents): string | null {
    return this.activeTabOf(BrowserWindow.fromWebContents(wc))
  }

  private fromBar(wc: WebContents): PrivateWin | undefined {
    const win = BrowserWindow.fromWebContents(wc)
    return win ? this.windows.get(win) : undefined
  }

  // ───────────────────────── Tabs ─────────────────────────

  private add(pw: PrivateWin, url: string, opts: { activate: boolean; after?: string }): string {
    const { store } = this.b
    const tab = newTab(url)
    store.state.tabs[tab.id] = tab
    store.detached.add(tab.id)
    this.b.views.inheritSession(tab.id, pw.ses)
    const after = opts.after ? pw.tabs.indexOf(opts.after) : -1
    pw.tabs.splice(after >= 0 ? after + 1 : pw.tabs.length, 0, tab.id)
    this.owner.set(tab.id, pw)
    if (opts.activate) {
      pw.active = tab.id
      pw.focusPage = true
    }
    this.b.broadcast.changed({ layout: false, persist: false })
    return tab.id
  }

  /** New empty tab (⌘T, + button): the address bar takes focus. */
  newTab(win: BrowserWindow | null): void {
    const pw = win && this.windows.get(win)
    if (pw) this.add(pw, '', { activate: true })
  }

  newTabFromBar(wc: WebContents): void {
    const pw = this.fromBar(wc)
    if (pw) this.add(pw, '', { activate: true })
  }

  /** Link opened from a private tab: new tab right after it, in the same window (same session). */
  openFrom(tabId: string, url: string, activate: boolean): void {
    const pw = this.owner.get(tabId)
    if (pw) this.add(pw, url, { activate, after: tabId })
  }

  activate(wc: WebContents, tabId: string): void {
    const pw = this.fromBar(wc)
    if (!pw || !pw.tabs.includes(tabId) || pw.active === tabId) return
    pw.active = tabId
    pw.focusPage = true
    this.b.broadcast.changed({ layout: false, persist: false })
  }

  closeFromBar(wc: WebContents, tabId: string): void {
    const pw = this.fromBar(wc)
    if (pw?.tabs.includes(tabId)) this.closeTab(tabId)
  }

  /** ⌘W: closes the displayed tab, and the window with its last tab. */
  closeActive(win: BrowserWindow | null): void {
    const pw = win && this.windows.get(win)
    if (!pw) return
    if (pw.active) this.closeTab(pw.active)
    else pw.win.close()
  }

  /** Closes a tab without leaving a trace (no ⇧⌘T); the window closes with its last tab. */
  closeTab(tabId: string): void {
    const pw = this.owner.get(tabId)
    if (!pw) return
    const i = pw.tabs.indexOf(tabId)
    pw.tabs.splice(i, 1)
    this.owner.delete(tabId)
    if (pw.shown && pw.shown === this.b.views.viewOf(tabId)) this.detachShown(pw)
    this.b.views.forget(tabId)
    if (pw.tabs.length === 0) {
      pw.win.close()
      return
    }
    if (pw.active === tabId) {
      pw.active = pw.tabs[Math.min(i, pw.tabs.length - 1)]
      pw.focusPage = true
    }
    this.b.broadcast.changed({ layout: false, persist: false })
  }

  /** Tab opened just for a download (never displayed): we close it again, unless it is the only one. */
  discardBlank(tabId: string): boolean {
    const pw = this.owner.get(tabId)
    if (!pw || pw.tabs.length < 2) return false
    this.closeTab(tabId)
    return true
  }

  /** ⌃Tab / ⌃⇧Tab in a private window. */
  cycle(win: BrowserWindow | null, step: number): void {
    const pw = win && this.windows.get(win)
    if (!pw || pw.tabs.length < 2 || !pw.active) return
    const i = pw.tabs.indexOf(pw.active)
    pw.active = pw.tabs[(i + step + pw.tabs.length) % pw.tabs.length]
    pw.focusPage = true
    this.b.broadcast.changed({ layout: false, persist: false })
  }

  /** ⌘1…⌘8: nth tab; ⌘9: the last. */
  goto(win: BrowserWindow | null, n: number): void {
    const pw = win && this.windows.get(win)
    if (!pw || pw.tabs.length === 0) return
    const id = n >= 9 ? pw.tabs[pw.tabs.length - 1] : pw.tabs[n - 1]
    if (!id || id === pw.active) return
    pw.active = id
    pw.focusPage = true
    this.b.broadcast.changed({ layout: false, persist: false })
  }

  /** Address typed in the bar: the displayed tab goes there, then the page takes focus. */
  navigate(wc: WebContents, input: string): void {
    const pw = this.fromBar(wc)
    if (!pw?.active || !input.trim()) return
    pw.focusPage = true
    this.b.tabs.navigate(pw.active, input.trim())
    // Tab already loaded: the view stays in place, focus returns to it right away.
    if (pw.shown && !pw.shown.webContents.isDestroyed()) {
      pw.shown.webContents.focus()
      pw.focusPage = false
    }
  }

  /** ⌘L: focus on the address bar. */
  focusAddress(win: BrowserWindow | null): void {
    const pw = win && this.windows.get(win)
    if (!pw || pw.win.webContents.isDestroyed()) return
    pw.win.webContents.focus()
    pushUi(pw.win.webContents, 'private:focus-address')
  }

  // ───────────────────────── Affichage ─────────────────────────

  /** Called on every `Browser.apply` cycle: places the displayed tab's view and pushes the state to the bars. */
  sync(): void {
    for (const pw of this.windows.values()) this.syncOne(pw)
  }

  // ───────────────────────── Disposition ─────────────────────────

  /** Same layout as the main window: tabs on top, or sidebar of the same width. */
  private get sidebarWidth(): number | null {
    return this.b.layout.horizontal ? null : this.b.store.state.sidebarWidth
  }

  private geometryKey(): string {
    const w = this.sidebarWidth
    return w === null ? 'horizontal' : `vertical:${w}`
  }

  /** Traffic lights centered in the tab row (on top) or in the sidebar's 44 px header. */
  private lights(): { x: number; y: number } {
    return this.b.layout.horizontal ? { x: 16, y: 13 } : { x: 16, y: 16 }
  }

  /** Card (address bar included) in a window of this size. */
  private card(width: number, height: number): Rect {
    const side = this.sidebarWidth
    return side === null
      ? { x: PAD, y: TOP, width: width - PAD * 2, height: height - TOP - PAD }
      : { x: side, y: PAD, width: width - side - PAD, height: height - PAD * 2 }
  }

  private syncOne(pw: PrivateWin): void {
    if (pw.win.isDestroyed()) return
    // Layout changed in the main window (⌃⇧S, bar width): the window follows.
    const geometry = this.geometryKey()
    if (geometry !== pw.geometry) {
      if (IS_MAC && geometry.split(':')[0] !== pw.geometry.split(':')[0]) pw.win.setWindowButtonPosition(this.lights())
      pw.geometry = geometry
      this.place(pw)
    }
    const id = pw.active
    const tab = id ? this.b.store.state.tabs[id] : undefined
    // An empty tab has no view: the window shows its welcome screen under the address bar.
    const view = id && tab?.url ? this.b.views.ensureView(id, pw.win) : null
    if (pw.shown !== view) {
      const changed = this.detachShown(pw)
      if (view) {
        pw.win.contentView.addChildView(view)
        pw.shown = view
        this.place(pw)
      }
      if (changed || view) this.b.permissions.onLayout()
    }
    if (pw.focusPage) {
      pw.focusPage = false
      if (view) view.webContents.focus()
      else pw.win.webContents.focus() // the bar then gives focus to the address field
    }
    this.push(pw)
  }

  /** Removes the placed view; true if there was one. */
  private detachShown(pw: PrivateWin): boolean {
    const view = pw.shown
    if (!view) return false
    pw.shown = null
    try {
      if (!pw.win.isDestroyed()) pw.win.contentView.removeChildView(view)
    } catch {
      // view already destroyed
    }
    return true
  }

  private place(pw: PrivateWin): void {
    if (!pw.shown || pw.win.isDestroyed()) return
    const { width, height } = pw.win.getContentBounds()
    const card = this.card(width, height)
    const full = !!pw.active && this.b.fullscreen.isOther(pw.active)
    pw.shown.setBorderRadius(full ? 0 : 12)
    pw.shown.setBounds(fitRect(full, { width, height }, { x: card.x, y: card.y + BAR, width: Math.max(0, card.width), height: Math.max(0, card.height - BAR) }))
  }

  /** A tab's page entered or left fullscreen: its window places the view again. */
  refit(tabId: string): void {
    const pw = this.owner.get(tabId)
    if (pw && pw.active === tabId) this.place(pw)
  }

  /** State requested by the bar at startup. */
  stateForBar(wc: WebContents): PrivateState | null {
    const pw = this.fromBar(wc)
    if (!pw) return null
    const state = this.state(pw)
    pw.lastSent = JSON.stringify(state)
    return state
  }

  private push(pw: PrivateWin): void {
    const { win } = pw
    if (win.isDestroyed() || win.webContents.isDestroyed()) return
    const state = this.state(pw)
    const json = JSON.stringify(state)
    if (json === pw.lastSent) return
    pw.lastSent = json
    pushUi(win.webContents, 'private:state', state)
  }

  private state(pw: PrivateWin): PrivateState {
    const tabs = this.b.store.state.tabs
    const active = pw.active ? tabs[pw.active] : undefined
    return {
      tabs: pw.tabs
        .map((id) => tabs[id])
        .filter((tab) => !!tab)
        .map((tab) => ({ id: tab.id, url: tab.url, title: tab.title, favicon: tab.favicon, loading: tab.loading, audible: tab.audible })),
      activeId: pw.active,
      sidebarWidth: this.sidebarWidth,
      url: active?.url ?? '',
      loading: active?.loading ?? false,
      canGoBack: active?.canGoBack ?? false,
      canGoForward: active?.canGoForward ?? false
    }
  }

  private onClosed(pw: PrivateWin): void {
    this.windows.delete(pw.win)
    pw.shown = null
    for (const id of pw.tabs) {
      this.owner.delete(id)
      this.b.views.forget(id)
    }
    pw.tabs = []
    this.b.downloads.dropPrivate(pw.ses)
    releasePrivateSession(pw.ses)
  }
}

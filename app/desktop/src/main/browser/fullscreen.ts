import type { BrowserWindow, WebContents } from 'electron'
import type { Changed } from './broadcast'

/**
 * A page asked for fullscreen (video player, presentation): the tab fills the window if it is displayed
 * in it and not in the mini player; otherwise the request is left to the page.
 */
export function canFill(tabId: string | null, shown: readonly string[], inMiniPlayer: boolean): tabId is string {
  return tabId !== null && !inMiniPlayer && shown.includes(tabId)
}

/** Rectangle of a page view: the whole content area while its page is fullscreen, otherwise `normal`. */
export function fitRect<R extends { x: number; y: number; width: number; height: number }>(
  full: boolean,
  size: { width: number; height: number },
  normal: R
): R | { x: number; y: number; width: number; height: number } {
  return full ? { x: 0, y: 0, width: size.width, height: size.height } : normal
}

export interface HtmlFullscreenDeps {
  window: { readonly win: BrowserWindow | null }
  /** Tabs displayed in the main window at the last layout. */
  shown: () => readonly string[]
  /** The page is in the mini player. */
  inMiniPlayer: (tabId: string) => boolean
  /** Web contents of a tab, if its view is alive. */
  contentsOf: (tabId: string) => WebContents | undefined
  changed: Changed
}

/**
 * Page fullscreen (`requestFullscreen`) in the main window. Electron puts the window in system fullscreen
 * (and takes it out) by itself, but a `WebContentsView` is not resized: the layout gives the whole window
 * to the tab's view (see `Layout`: every other layer is hidden meanwhile).
 */
export class HtmlFullscreen {
  /** Tab whose page is fullscreen. */
  tabId: string | null = null
  /** The window was already in system fullscreen when the page asked for it. */
  private windowWasFull = false
  /** Tabs fullscreen in another window (private, app, Little): those windows lay their view out themselves. */
  private others = new Set<string>()
  private otherListeners: ((tabId: string) => void)[] = []

  /** True if the tab's page is fullscreen in a window other than the main one. */
  isOther(tabId: string): boolean {
    return this.others.has(tabId)
  }

  /** Called with a tab when its page enters or leaves fullscreen outside the main window. */
  onOther(fn: (tabId: string) => void): void {
    this.otherListeners.push(fn)
  }

  private setOther(tabId: string, on: boolean): void {
    if (this.others.has(tabId) === on) return
    if (on) this.others.add(tabId)
    else this.others.delete(tabId)
    for (const fn of this.otherListeners) fn(tabId)
  }
  constructor(private readonly d: HtmlFullscreenDeps) {}

  /** Follows a tab's page; called when its view is created. */
  attach(tabId: string, wc: WebContents): void {
    wc.on('enter-html-full-screen', () => this.enter(tabId))
    wc.on('leave-html-full-screen', () => (this.leave(tabId), this.setOther(tabId, false)))
    wc.once('destroyed', () => {
      // The page vanished without telling the window: Electron does not leave system fullscreen then.
      const win = this.d.window.win
      const mine = this.tabId === tabId
      this.leave(tabId)
      this.others.delete(tabId)
      if (mine && !this.windowWasFull && win && !win.isDestroyed() && win.isFullScreen()) win.setFullScreen(false)
    })
  }

  private enter(tabId: string): void {
    const win = this.d.window.win
    if (!win || win.isDestroyed() || !canFill(tabId, this.d.shown(), this.d.inMiniPlayer(tabId))) return this.setOther(tabId, true)
    if (this.tabId && this.tabId !== tabId) this.exitPage(this.tabId)
    this.tabId = tabId
    this.windowWasFull = win.isFullScreen()
    this.d.changed({ persist: false, layout: true })
  }

  private leave(tabId: string): void {
    if (this.tabId !== tabId) return
    this.tabId = null
    this.d.changed({ persist: false, layout: true })
  }

  /** Asks the page to leave fullscreen (it answers with `leave-html-full-screen`). */
  private exitPage(tabId: string): void {
    const wc = this.d.contentsOf(tabId)
    if (wc && !wc.isDestroyed()) wc.executeJavaScript('document.fullscreenElement && document.exitFullscreen()').catch(() => {})
  }

  /** The tab is no longer displayed (other tab, other space, sleep): the page leaves fullscreen. */
  onLayout(shown: readonly string[]): void {
    if (this.tabId && !shown.includes(this.tabId)) this.exitPage(this.tabId)
    if (this.tabId && !this.d.contentsOf(this.tabId)) this.leave(this.tabId)
  }

  /** The window left system fullscreen (green button, shortcut) while a page is fullscreen. */
  onWindowLeft(): void {
    if (!this.tabId) return
    this.exitPage(this.tabId)
  }

  /** The main window is closed. */
  reset(): void {
    this.tabId = null
  }
}

import { BrowserWindow, type Rectangle, type WebContents, type WebContentsView } from 'electron'
import { windowChrome } from '../app/platform'
import type { LittleState } from '@shared/types'
import type { Browser } from '../browser/browser'
import { fitRect } from '../browser/fullscreen'
import { newTab } from '../core/store'
import { denyWindowOpen } from '../web/guard'
import { loadRoute, preloadPath } from '../core/urls'
import { pushUi } from '../ipc/push'

const CASCADE = 24
/** Margin around the card + the card's bar (like PANE_BAR). */
const PAD = 8
const BAR = PAD + 40

interface Entry {
  tabId: string
  view: WebContentsView
  /** Last state sent to the bar (JSON): avoids sending the same one again. */
  lastSent: string
  /** Places the page view (follows the window size and the page fullscreen). */
  place?: () => void
}

/** Small windows to quickly open a link: a bar (route #little) and a detached tab. */
export class LittleWindows {
  private windows = new Map<BrowserWindow, Entry>()
  private last: Rectangle | null = null

  constructor(private b: Browser) {}

  open(url: string): BrowserWindow {
    const { store } = this.b
    const tab = newTab(url)
    store.state.tabs[tab.id] = tab
    store.detached.add(tab.id)

    if (this.windows.size === 0) this.last = null
    const win = new BrowserWindow({
      width: 760,
      height: 820,
      x: this.last ? this.last.x + CASCADE : undefined,
      y: this.last ? this.last.y + CASCADE : undefined,
      ...windowChrome({ lights: { x: 14, y: 14 }, vibrancy: true }),
      webPreferences: { preload: preloadPath, sandbox: true, contextIsolation: true, nodeIntegration: false }
    })
    this.last = win.getBounds()

    const view = this.b.views.ensureView(tab.id, win)
    const entry: Entry = { tabId: tab.id, view, lastSent: '' }
    this.windows.set(win, entry)
    win.contentView.addChildView(view)
    const place = () => {
      const { width, height } = win.getContentBounds()
      const full = this.b.fullscreen.isOther(tab.id)
      view.setBorderRadius(full ? 0 : 12)
      view.setBounds(fitRect(full, { width, height }, { x: PAD, y: BAR, width: width - PAD * 2, height: height - BAR - PAD }))
    }
    entry.place = place
    place()
    win.on('resize', place)

    // The bar has no state getter: it signals `little:ready` once subscribed (see `ready`), then we push on every change.
    win.on('closed', () => {
      // Closing the window destroys the tab (unless it was just taken over by the main window).
      const entry = this.windows.get(win)
      if (!entry) return
      this.windows.delete(win)
      this.b.views.forget(entry.tabId)
    })
    denyWindowOpen(win.webContents)
    loadRoute(win.webContents, '#little')
    return win
  }

  /** The bar is subscribed (or reloaded): forget the last send and send the state again. */
  ready(barre: WebContents): void {
    const win = BrowserWindow.fromWebContents(barre)
    const entry = win && this.windows.get(win)
    if (!win || !entry || win.webContents !== barre) return
    entry.lastSent = ''
    this.push(win)
  }

  /** A tab's page entered or left fullscreen. */
  refit(tabId: string): void {
    for (const e of this.windows.values()) if (e.tabId === tabId) e.place?.()
  }

  tabIdOf(win: BrowserWindow | null): string | null {
    return (win && this.windows.get(win)?.tabId) || null
  }

  tabIdForBar(wc: WebContents): string | null {
    return this.tabIdOf(BrowserWindow.fromWebContents(wc))
  }

  /** Tabs of the open small windows. */
  tabIds(): string[] {
    return [...this.windows.values()].map((e) => e.tabId)
  }

  pushStates(): void {
    for (const win of this.windows.keys()) this.push(win)
  }

  private push(win: BrowserWindow): void {
    const entry = this.windows.get(win)
    const tab = entry && this.b.store.state.tabs[entry.tabId]
    if (!tab || win.isDestroyed() || win.webContents.isDestroyed()) return
    const state: LittleState = {
      tabId: tab.id,
      url: tab.url,
      title: tab.title,
      favicon: tab.favicon,
      loading: tab.loading,
      canGoBack: tab.canGoBack,
      canGoForward: tab.canGoForward,
      hue: this.b.space.hue,
      themeColor: tab.themeColor
    }
    const json = JSON.stringify(state)
    if (entry.lastSent === json) return
    entry.lastSent = json
    pushUi(win.webContents, 'little:state', state)
  }

  openInMain(barre: WebContents): void {
    const win = BrowserWindow.fromWebContents(barre)
    const entry = win && this.windows.get(win)
    if (!win || !entry) return
    this.windows.delete(win)
    win.contentView.removeChildView(entry.view)
    this.b.tabs.adopt(entry.tabId)
    this.b.window.show()
    win.close()
  }
}

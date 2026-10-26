import type { WebContents } from 'electron'
import type { FindState, Rect } from '@shared/types'
import type { Browser } from './browser'
import { UiView } from './uiview'

const WIDTH = 360
const HEIGHT = 48
const MARGIN = 12
/** Duration of the pill fade-out (= --t-fast on the CSS side). */
const EXIT_MS = 120

/** Find in the active tab's page: logic here, UI in the `#find` view. */
export class FindBar {
  private ui: UiView | null = null
  private tabId: string | null = null
  private target: WebContents | null = null
  private state: FindState = { session: 0, query: '', active: 0, matches: 0, hue: 0 }
  /** Last query per tab, restored on reopening. */
  private queries = new Map<string, string>()
  private session = 0
  /** Animated exit in progress: the view stays in place, the pill fades out. */
  private leaving = false
  private leaveTimer: NodeJS.Timeout | null = null

  constructor(private readonly b: Browser) {}

  create(): void {
    this.ui ??= new UiView('#find')
  }

  destroy(): void {
    this.close()
    this.clearLeave()
    this.ui?.destroy()
    this.ui = null
  }

  get view() {
    return this.ui?.view ?? null
  }

  get isOpen(): boolean {
    return this.tabId !== null || this.leaving
  }

  /** Opens the bar on the active tab, or gives it focus again if it is already open. */
  open(tabId: string): void {
    this.clearLeave()
    this.create() // on demand if the warm-up has not happened yet; the state is queued until loading
    if (this.tabId !== tabId) {
      this.close()
      const wc = this.b.views.viewOf(tabId)?.webContents
      if (!wc) return
      this.tabId = tabId
      this.target = wc
      this.state = { session: ++this.session, query: this.queries.get(tabId) ?? '', active: 0, matches: 0, hue: this.b.space.hue }
      wc.on('found-in-page', this.onFound)
      if (this.state.query) wc.findInPage(this.state.query)
    }
    this.push()
  }

  /** ⌘F: opens the find bar on the active tab (or gives it focus again). */
  showOnActive(): void {
    const { b } = this
    const tabId = b.space.activeTabId
    if (!b.win || b.overlay.open || !tabId || !b.views.has(tabId)) return
    this.open(tabId)
    b.layout.requestFocus(true)
    b.broadcast.changed({ persist: false })
  }

  /** ⌘G / ⇧⌘G: next or previous match; with no search in progress, opens the bar. */
  stepOrShow(forward: boolean): void {
    if (!this.step(forward)) this.showOnActive()
  }

  /** Closes the bar (Escape, cross) with its animation and gives focus back to the page. */
  dismiss(): void {
    this.close(true)
    this.b.layout.requestFocus()
    this.b.broadcast.changed({ persist: false })
  }

  /** Next or previous match; with no query, equivalent to opening the bar. */
  step(forward: boolean): boolean {
    if (!this.tabId || !this.state.query) return false
    this.search(this.state.query, { forward, findNext: true })
    return true
  }

  search(query: string, opts: { forward: boolean; findNext: boolean }): void {
    if (!this.tabId || !this.target || this.target.isDestroyed()) return
    if (!this.queries.has(this.tabId)) {
      const id = this.tabId
      this.target.once('destroyed', () => this.queries.delete(id)) // forget the query when the tab closes
    }
    this.queries.set(this.tabId, query)
    this.state = { ...this.state, query }
    if (query) {
      this.target.findInPage(query, opts)
    } else {
      this.target.stopFindInPage('clearSelection')
      this.state = { ...this.state, active: 0, matches: 0 }
      this.push()
    }
  }

  /** `animate`: intended close (Escape, cross), the pill fades out before the view is removed; otherwise immediate removal. */
  close(animate = false): void {
    if (!this.target) return
    if (!this.target.isDestroyed()) {
      this.target.off('found-in-page', this.onFound)
      this.target.stopFindInPage('clearSelection')
    }
    this.target = null
    this.tabId = null
    if (animate && this.ui) {
      this.leaving = true
      this.state = { ...this.state, closing: true }
      this.push()
      this.leaveTimer = setTimeout(() => {
        this.leaveTimer = null
        this.leaving = false
        this.b.broadcast.changed({ persist: false })
      }, EXIT_MS)
    } else this.clearLeave()
  }

  private clearLeave(): void {
    if (this.leaveTimer) clearTimeout(this.leaveTimer)
    this.leaveTimer = null
    this.leaving = false
  }

  /** The tab just navigated to another page. */
  onNavigate(tabId: string): void {
    if (this.tabId === tabId) this.close()
  }

  /** Top-right position of the panel of the searched tab. */
  bounds(rect: Rect): Rect {
    return { x: Math.max(rect.x, rect.x + rect.width - WIDTH - MARGIN), y: rect.y + MARGIN, width: WIDTH, height: HEIGHT }
  }

  /** Closes the bar if the active tab is no longer the searched one. */
  follow(activeTabId: string | null): void {
    if (this.tabId && this.tabId !== activeTabId) this.close()
  }

  private onFound = (_e: Electron.Event, result: Electron.Result): void => {
    if (!result.finalUpdate) return
    this.state = { ...this.state, active: result.activeMatchOrdinal, matches: result.matches }
    this.push()
  }

  private push(): void {
    this.ui?.send('find:state', this.state)
  }
}

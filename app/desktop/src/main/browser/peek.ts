import type { BrowserWindow, WebContentsView } from 'electron'
import type { Rect } from '@shared/types'
import { newTab } from '@signet/shared/model'
import type { Store } from '../core/store'
import { activeSpace } from '../model/locate'
import type { Changed } from './broadcast'
import type { Floating } from './floating'
import type { Layout, LayoutContext, PeekLayer } from './layout'
import { OVERLAY_EXIT_MS, type Overlay, type PeekHandle } from './overlay'
import { lerpRect, PeekMotion, PEEK_ORIGIN_SCALE, PEEK_SCALE, scaledRect } from './peekanim'
import type { Views } from './views'

/** Peek card: centered, 300 to 1100 px wide, under a 64 px margin. */
export function peekCard(width: number, height: number): Rect {
  const w = Math.max(300, Math.min(width - 160, 1100))
  return { x: Math.round((width - w) / 2), y: 64, width: w, height: Math.max(200, height - 96) }
}

export interface PeekDeps {
  store: Store
  window: { readonly win: BrowserWindow | null }
  views: Pick<Views, 'ensureView' | 'viewOf' | 'forget' | 'sessionOfTab' | 'inheritSession'>
  layout: Pick<Layout, 'put' | 'panes' | 'shownTabs' | 'requestFocus'>
  floating: Pick<Floating, 'hide'>
  overlay: Pick<Overlay, 'contents' | 'kind' | 'setState' | 'send' | 'hide'>
  /** Immediate full cycle (layout, state). */
  apply: () => void
  changed: Changed
  /** Gives a today's-tab slot to the Peek that becomes a tab, and activates it. */
  adopt: (tabId: string) => void
}

/** Floating preview of a page (Peek): a detached tab in an overlay card, which can become a real tab. */
export class Peek implements PeekLayer, PeekHandle {
  /** Detached tab of the open Peek. */
  tabId: string | null = null
  /** Peek entering or exiting: the web view is scaled down around the center of its card (see peekanim.ts). */
  private motion = new PeekMotion()
  private leaving: string | null = null
  /** Point (window coordinates) from which the Peek springs and where it returns; null: from the card center. */
  private origin: { x: number; y: number } | null = null
  /** Switch to a tab: the view slides from the Peek card to the tab's slot (`expandMotion` goes from 0 to 1). */
  private expanding: string | null = null
  private expandMotion = new PeekMotion()
  private leaveTimer: NodeJS.Timeout | null = null
  private lastCard = ''

  constructor(private readonly d: PeekDeps) {}

  get busy(): boolean {
    return this.tabId !== null || this.leaving !== null || this.expanding !== null
  }

  /** Forgets the last card sent (the overlay closes). */
  resetCard(): void {
    this.lastCard = ''
  }

  /** Main window closed. */
  reset(): void {
    this.motion.stop()
    this.finishLeave()
    this.expandMotion.set(0)
    this.expanding = null
    if (this.tabId) this.d.views.forget(this.tabId)
    this.tabId = null
  }

  /** Places the Peek's views (open, exiting or switching to a tab); returns its web view if it is open. */
  place({ W, H, panes, ids, desired }: LayoutContext): WebContentsView | undefined {
    const { views, overlay, layout } = this.d
    const peekView = this.tabId ? views.viewOf(this.tabId) : undefined
    if (overlay.kind === 'peek' && peekView) {
      layout.put(peekView, scaledRect(peekCard(W, H), this.motion.scale, this.origin), 12)
      desired.push(peekView)
      if (JSON.stringify(peekCard(W, H)) !== this.lastCard) this.send()
    }
    // Peek closing: its view stays in place, shrinking, for the duration of the overlay's exit.
    const leavingView = this.leaving ? views.viewOf(this.leaving) : undefined
    if (leavingView) {
      layout.put(leavingView, scaledRect(peekCard(W, H), this.motion.scale, this.origin), 12)
      desired.push(leavingView)
    }
    // Peek becoming a tab: the view slides to the tab's slot, above the fading overlay.
    const expandingView = this.expanding ? views.viewOf(this.expanding) : undefined
    const expandPane = panes.find((p) => p.tabId === this.expanding)
    if (expandingView && expandPane && ids.includes(expandPane.tabId)) {
      layout.put(expandingView, lerpRect(peekCard(W, H), expandPane.rect, this.expandMotion.scale))
      const i = desired.indexOf(expandingView)
      if (i >= 0) desired.splice(i, 1)
      desired.push(expandingView)
    } else if (this.expanding) {
      // The tab is no longer displayed (tab switch, closing): nothing left to slide.
      this.expandMotion.set(0)
      this.expanding = null
    }
    return peekView
  }

  open(url: string, fromTabId?: string, point?: { x: number; y: number }): void {
    const { store, window, views, overlay, layout, floating } = this.d
    if (!window.win || !overlay.contents) return
    if (this.tabId) this.close()
    this.finishExpand()
    // Clicked point (page) -> window: we add the position of the origin tab's view, if it is displayed.
    const from = fromTabId && layout.shownTabs.includes(fromTabId) ? views.viewOf(fromTabId) : undefined
    const fb = from?.getBounds()
    this.origin = point && fb ? { x: Math.round(fb.x + point.x), y: Math.round(fb.y + point.y) } : null
    const tab = newTab(url)
    store.state.tabs[tab.id] = tab
    store.detached.add(tab.id)
    views.inheritSession(tab.id, views.sessionOfTab(fromTabId ?? activeSpace(store.state).activeTabId ?? ''))
    this.tabId = tab.id
    views.ensureView(tab.id)
    floating.hide()
    overlay.setState({ kind: 'peek' })
    this.send()
    layout.requestFocus()
    // Entry: the web view grows from the link (or the center of its card), like the overlay card.
    this.motion.set(this.origin ? PEEK_ORIGIN_SCALE : PEEK_SCALE)
    this.d.apply()
    this.motion.glide(1, 0.3, () => this.placeView())
  }

  /** `animate`: closing requested by the user, the web view shrinks while the overlay fades out. */
  close(animate = false): void {
    const { views, overlay } = this.d
    const id = this.tabId
    this.tabId = null
    this.finishLeave()
    if (id && animate && overlay.kind === 'peek') {
      this.leaving = id
      this.motion.glide(this.origin ? PEEK_ORIGIN_SCALE : PEEK_SCALE, OVERLAY_EXIT_MS / 1000, () => this.placeView())
      this.leaveTimer = setTimeout(() => this.finishLeave(), OVERLAY_EXIT_MS)
      overlay.hide()
      return
    }
    this.motion.set(1)
    if (id) views.forget(id)
    overlay.hide(true)
  }

  /** End of the Peek's exit: the web view is destroyed. */
  private finishLeave(): void {
    if (this.leaveTimer) clearTimeout(this.leaveTimer)
    this.leaveTimer = null
    const id = this.leaving
    this.leaving = null
    if (!id) return
    this.motion.set(1)
    this.d.views.forget(id)
  }

  /** Places the Peek's view (open or exiting) at the current scale, without going back through the layout. */
  private placeView(): void {
    const { window, views, layout } = this.d
    const id = this.tabId ?? this.leaving
    const view = id ? views.viewOf(id) : undefined
    if (!view || !window.win || view.webContents.isDestroyed()) return
    const { width, height } = window.win.getContentBounds()
    layout.put(view, scaledRect(peekCard(width, height), this.motion.scale, this.origin))
  }

  /** Places the Peek view that becomes a tab, between its card and the tab's slot. */
  private placeExpand(): void {
    const { window, views, layout } = this.d
    const id = this.expanding
    const view = id ? views.viewOf(id) : undefined
    const pane = layout.panes.find((p) => p.tabId === id)
    if (!view || !pane || !window.win || view.webContents.isDestroyed()) return this.finishExpand()
    const { width, height } = window.win.getContentBounds()
    layout.put(view, lerpRect(peekCard(width, height), pane.rect, this.expandMotion.scale))
  }

  /** End (or abandonment) of the slide to the tab: the layout takes the view back to its place. */
  private finishExpand(): void {
    this.expandMotion.set(0)
    if (!this.expanding) return
    this.expanding = null
    this.d.changed({ persist: false })
  }

  private send(): void {
    const { store, window, overlay } = this.d
    const tab = this.tabId ? store.state.tabs[this.tabId] : undefined
    if (!tab || !window.win) return
    const { width, height } = window.win.getContentBounds()
    const card = peekCard(width, height)
    this.lastCard = JSON.stringify(card)
    overlay.send({ mode: 'peek', tabId: tab.id, url: tab.url, title: tab.title || tab.url, card, origin: this.origin ?? undefined })
  }

  /** The Peek's address or title changed: its card is sent again. */
  refresh(): void {
    if (this.d.overlay.kind === 'peek') this.send()
  }

  /** The Peek becomes a today's tab, the view sliding from its card to the tab's slot. */
  expand(): void {
    const { overlay } = this.d
    const id = this.tabId
    if (!id) return
    this.tabId = null
    this.motion.set(1)
    // The scrim fades out while the web view slides from the card to the tab's slot.
    this.expanding = id
    this.expandMotion.set(0)
    overlay.hide()
    this.d.adopt(id)
    // Layout right away: the tab's slot is known, the view stays on the card (q = 0) before sliding.
    this.d.apply()
    this.expandMotion.glide(
      1,
      0.35,
      () => this.placeExpand(),
      () => this.finishExpand()
    )
  }
}

import { powerMonitor, type Rectangle, type WebContents, type WebContentsView } from 'electron'
import type { Rect, TabPreviewPayload } from '@shared/types'
import type { Browser } from './browser'
import { UiView } from './uiview'

/** Thumbnails kept in memory (the oldest go first). */
const MAX_THUMBS = 60
const THUMB_W = 560
/** Delay before capturing a tab that was just displayed (time for it to paint). */
const SHOWN_DELAY = 900
/** A page already captured less than 60 s ago is not recaptured; displayed tabs are at most at this rate (scrolling page). */
const RECAPTURE_MS = 60_000
/** After a load finishes, the time for the page to repaint. */
const LOADED_DELAY = 600
/** Card: width, thumbnail height, text height; transparent margin for the shadow. */
const CARD_W = 280
const THUMB_H = 158
const TEXT_H = 64
const MARGIN = 14
const GAP = 6
/** Animated exit: the view stays in place for the duration of the fade. */
const LEAVE_MS = 120

/** Tab preview on hover (sidebar, tab row): small view placed next to the row, non-interactive. */
export class TabPreviews {
  private thumbs = new Map<string, { url: string; at: number; page: string }>()
  /** Tabs displayed at the last layout: only newcomers are scheduled. */
  private shown = new Set<string>()
  private timers = new Map<string, NodeJS.Timeout>()
  private ui: UiView | null = null
  private bounds: Rectangle | null = null
  private visible = false
  private leaveTimer: NodeJS.Timeout | null = null
  private refresh: NodeJS.Timeout | null = null

  constructor(private b: Browser) {}

  /** The preview view, if it exists (placed or not). */
  get viewIfAny(): WebContentsView | null {
    return this.ui?.view ?? null
  }

  /** The preview view and its position, if it must be placed on the window (called by the layout). */
  get placed(): { view: WebContentsView; bounds: Rectangle } | null {
    return this.ui && this.bounds && (this.visible || this.leaveTimer) ? { view: this.ui.view, bounds: this.bounds } : null
  }

  private get enabled(): boolean {
    return this.b.store.state.settings.tabPreview !== false
  }

  // ───────────────────────── Thumbnails ─────────────────────────

  /** Layout computed: tabs that have just appeared are captured a little later. */
  onShown(ids: string[]): void {
    for (const [id, timer] of this.timers) {
      if (ids.includes(id)) continue
      clearTimeout(timer)
      this.timers.delete(id)
    }
    const fresh = ids.filter((id) => !this.shown.has(id))
    this.shown = new Set(ids)
    for (const id of fresh) this.schedule(id, SHOWN_DELAY)
    // Slow refresh of displayed tabs, except on battery.
    if (!this.refresh && this.enabled) {
      this.refresh = setInterval(() => {
        if (powerMonitor.isOnBatteryPower() || !this.b.win?.isFocused()) return
        for (const id of this.b.layout.shownTabs) void this.capture(id)
      }, RECAPTURE_MS + 1000)
      this.refresh.unref()
    }
  }

  /** A displayed tab has just finished loading: new capture. */
  onLoaded(tabId: string): void {
    this.schedule(tabId, LOADED_DELAY)
  }

  private schedule(id: string, delay: number): void {
    if (!this.enabled) return
    const old = this.timers.get(id)
    if (old) clearTimeout(old)
    const timer = setTimeout(() => {
      this.timers.delete(id)
      void this.capture(id)
    }, delay)
    timer.unref()
    this.timers.set(id, timer)
  }

  /** Downscaled capture of a displayed tab (never a private window tab). */
  async capture(tabId: string): Promise<void> {
    if (!this.enabled || this.b.privates.owns(tabId)) return
    const wc = this.b.views.viewOf(tabId)?.webContents
    if (!wc || wc.isDestroyed() || wc.isLoading() || !this.b.win?.isVisible() || !/^(https?|file|signet):/i.test(wc.getURL())) return
    const page = wc.getURL()
    const last = this.thumbs.get(tabId)
    if (last && last.page === page && Date.now() - last.at < RECAPTURE_MS) return
    try {
      const img = await wc.capturePage()
      if (img.isEmpty()) return
      const small = img.resize({ width: THUMB_W, quality: 'good' })
      this.thumbs.delete(tabId)
      this.thumbs.set(tabId, { url: `data:image/jpeg;base64,${small.toJPEG(72).toString('base64')}`, at: Date.now(), page })
      // Closed tabs and the oldest captures go away.
      for (const id of this.thumbs.keys()) if (!this.b.store.state.tabs[id]) this.thumbs.delete(id)
      while (this.thumbs.size > MAX_THUMBS) this.thumbs.delete(this.thumbs.keys().next().value as string)
    } catch {
      // page gone in the meantime
    }
  }

  // ───────────────────────── Card ─────────────────────────

  /**
   * Shows the tab's preview next to `rect` (coordinates of the requesting view). `below`: under the element (top
   * tab row); otherwise next to the sidebar, on the page side.
   */
  show(sender: WebContents, tabId: string, rect: Rect, below: boolean): void {
    const tab = this.b.store.state.tabs[tabId]
    const win = this.b.win
    if (!this.enabled || !tab || !win || this.b.store.detached.has(tabId)) return this.hide()
    // Tab already on screen (the current tab, or a panel of the split view): the preview would teach nothing.
    if (this.b.layout.shownTabs.includes(tabId)) return this.hide()
    const origin = this.b.layout.uiOrigin(sender)
    const r = { x: rect.x + origin.x, y: rect.y + origin.y, width: rect.width, height: rect.height }
    const thumb = this.thumbs.get(tabId)
    const right = this.b.store.state.settings.sidebarSide === 'right'
    // No margin (hence no shadow) on the tab bar side: the view's transparent margin would cover it, and
    // the view would not let the bar's translucent effect show through (dark spot, clipped line corners).
    const ml = !below && !right ? 0 : MARGIN
    const mr = !below && right ? 0 : MARGIN
    const mt = below ? 0 : MARGIN
    const width = CARD_W + ml + mr
    const height = (thumb ? THUMB_H : 0) + TEXT_H + mt + MARGIN
    const { width: W, height: H } = win.getContentBounds()
    let x: number
    let y: number
    if (below) {
      x = r.x - ml
      y = r.y + r.height + GAP - mt
    } else {
      x = right ? r.x - width - GAP + mr : r.x + r.width + GAP - ml
      y = r.y - mt - 4
    }
    x = Math.round(Math.max(0, Math.min(x, W - width)))
    y = Math.round(Math.max(0, Math.min(y, H - height)))
    this.bounds = { x, y, width, height }
    this.ui ??= new UiView('#preview')
    if (this.leaveTimer) clearTimeout(this.leaveTimer)
    this.leaveTimer = null
    const fresh = !this.visible
    this.visible = true
    this.push(tabId, fresh, below ? 'below' : right ? 'left' : 'right')
    this.b.layout.placePreview()
  }

  /** Sends the card content (or refreshes it after a capture). */
  private push(tabId: string, fresh = false, from?: TabPreviewPayload['from']): void {
    const tab = this.b.store.state.tabs[tabId]
    if (!tab || !this.ui || !this.visible) return
    const payload: TabPreviewPayload = {
      tabId,
      title: tab.customTitle || tab.title || tab.url,
      url: tab.url,
      favicon: tab.favicon,
      thumb: this.thumbs.get(tabId)?.url,
      audible: tab.audible && !tab.muted,
      asleep: !tab.loaded,
      fresh,
      from: from ?? 'right'
    }
    this.ui.send('preview:show', payload)
  }

  hide(): void {
    if (!this.visible) return
    this.visible = false
    this.ui?.send('preview:hide')
    if (this.leaveTimer) clearTimeout(this.leaveTimer)
    this.leaveTimer = setTimeout(() => {
      this.leaveTimer = null
      this.b.layout.placePreview()
    }, LEAVE_MS)
  }

  /** Setting turned off: the preview view is destroyed rather than just hidden. */
  release(): void {
    this.visible = false
    this.clearLeave()
    this.b.layout.placePreview()
    this.ui?.destroy()
    this.ui = null
    this.bounds = null
    for (const timer of this.timers.values()) clearTimeout(timer)
    this.timers.clear()
    this.stopRefresh()
  }

  private stopRefresh(): void {
    if (this.refresh) clearInterval(this.refresh)
    this.refresh = null
  }

  private clearLeave(): void {
    if (this.leaveTimer) clearTimeout(this.leaveTimer)
    this.leaveTimer = null
  }

  destroy(): void {
    this.clearLeave()
    for (const timer of this.timers.values()) clearTimeout(timer)
    this.timers.clear()
    this.stopRefresh()
    this.shown.clear()
    this.ui?.destroy()
    this.ui = null
    this.visible = false
    this.bounds = null
  }
}

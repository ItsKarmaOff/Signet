import { BrowserWindow, type WebContents } from 'electron'
import type { Browser } from './browser'

const INTERVAL = 1500
/** After this many identical samples in a row, or this ceiling (animated header), the tab is only probed every `SLOW_INTERVAL` ms. */
const STABLE_AFTER = 4
const MAX_SAMPLES = 12
const SLOW_INTERVAL = 15_000
/** After a navigation, closely spaced samples (ms) to follow the page from its first renders. */
const BURST = [60, 200, 450, 900]
/** Sampled row (px): under the loading bars stuck to the top (YouTube, GitHub…), in the header. */
const SAMPLE_Y = 6

/** Most frequent color of the first row of a BGRA capture (4 bits per channel), `#rrggbb` or null. */
export function dominantColor(bitmap: Buffer, width: number): string | null {
  const buckets = new Map<number, { n: number; r: number; g: number; b: number }>()
  for (let i = 0; i < width && i * 4 + 3 < bitmap.length; i++) {
    const o = i * 4
    if (bitmap[o + 3] === 0) continue
    const [b, g, r] = [bitmap[o], bitmap[o + 1], bitmap[o + 2]]
    const key = ((r >> 4) << 8) | ((g >> 4) << 4) | (b >> 4)
    const e = buckets.get(key) ?? { n: 0, r: 0, g: 0, b: 0 }
    e.n++
    e.r += r
    e.g += g
    e.b += b
    buckets.set(key, e)
  }
  let best: { n: number; r: number; g: number; b: number } | null = null
  for (const e of buckets.values()) if (!best || e.n > best.n) best = e
  if (!best) return null
  const hex = (v: number) =>
    Math.round(v / best!.n)
      .toString(16)
      .padStart(2, '0')
  return `#${hex(best.r)}${hex(best.g)}${hex(best.b)}`
}

/** Samples the color at the top of the displayed pages (`TabInfo.themeColor`). */
export class SiteColor {
  private inflight = new Set<string>()
  /** Last `theme-color` declared by the page, used when there is no sample. */
  private meta = new Map<string, string>()
  private shown = new Set<string>()
  /** Per-tab tracking: last sampled color, identical samples in a row, total since the last navigation, date of the last one. */
  private stable = new Map<string, { color: string | null; same: number; n: number; at: number }>()
  private bursts = new Map<string, NodeJS.Timeout[]>()
  private timer: NodeJS.Timeout

  constructor(private b: Browser) {
    this.timer = setInterval(() => this.tick(), INTERVAL)
  }

  dispose(): void {
    clearInterval(this.timer)
  }

  /** Hooks up the events of a tab view. */
  attach(tabId: string, wc: WebContents): void {
    // Event (navigation, load): the page may have changed, we restart from a fast probe.
    wc.on('did-navigate', () => this.burst(tabId))
    wc.on('did-navigate-in-page', (_e, _url, isMainFrame) => isMainFrame && this.burst(tabId))
    wc.on('dom-ready', () => this.burst(tabId))
    wc.on('did-finish-load', () => this.burst(tabId))
    wc.on('did-change-theme-color', (_e, color) => {
      if (color) this.meta.set(tabId, color.slice(0, 7))
      else this.meta.delete(tabId)
    })
    wc.once('destroyed', () => {
      for (const t of this.bursts.get(tabId) ?? []) clearTimeout(t)
      this.bursts.delete(tabId)
      this.meta.delete(tabId)
      this.inflight.delete(tabId)
      this.stable.delete(tabId)
    })
  }

  /** Burst of samples after a page event, the previous one is canceled.
   * A tab neither displayed nor detached (Peek, Little Signet) is not sampled: `onShown` will do it when it returns. */
  private burst(tabId: string): void {
    for (const t of this.bursts.get(tabId) ?? []) clearTimeout(t)
    this.stable.delete(tabId)
    const visible = () => this.shown.has(tabId) || this.b.store.detached.has(tabId)
    this.bursts.set(
      tabId,
      BURST.map((ms) => setTimeout(() => visible() && void this.sample(tabId, true), ms))
    )
  }

  /** Called on every layout: a tab that has just been displayed is sampled right away. */
  onShown(ids: string[]): void {
    const next = new Set(ids)
    for (const id of next) if (!this.shown.has(id)) setTimeout(() => void this.sample(id, true), 50)
    this.shown = next
  }

  private tick(): void {
    // Only the foreground window is probed: the main one, or the active Little Signet.
    const focused = BrowserWindow.getFocusedWindow()
    if (!focused) return
    const win = this.b.win
    const ids =
      win && focused === win && !win.isDestroyed() && win.isVisible() && !win.isMinimized()
        ? this.b.model.displayedTabIds()
        : [this.b.little.tabIdOf(focused) ?? this.b.webApps.tabIdOf(focused)].filter((id): id is string => !!id)
    const now = Date.now()
    for (const id of ids) {
      const st = this.stable.get(id)
      if (st && (st.same >= STABLE_AFTER || st.n >= MAX_SAMPLES) && now - st.at < SLOW_INTERVAL) continue
      void this.sample(id)
    }
  }

  async sample(tabId: string, reset = false): Promise<void> {
    if (reset) this.stable.delete(tabId)
    if (this.inflight.has(tabId)) return
    const view = this.b.views.viewOf(tabId)
    const wc = view?.webContents
    if (!view || !wc || wc.isDestroyed() || !this.b.store.state.tabs[tabId]) return
    const { width: w, height: h } = view.getBounds()
    const width = Math.round(w)
    if (width <= 0) return
    const y = h > SAMPLE_Y + 2 ? SAMPLE_Y : 0
    this.inflight.add(tabId)
    let color: string | null = null
    try {
      const img = await wc.capturePage({ x: 0, y, width, height: 2 })
      // The tab may have been closed or its view replaced during the capture.
      if (wc.isDestroyed() || this.b.views.viewOf(tabId)?.webContents !== wc) return
      if (!img.isEmpty()) color = dominantColor(img.toBitmap(), img.getSize().width)
    } catch {
      color = null
    } finally {
      this.inflight.delete(tabId)
    }
    const tab = this.b.store.state.tabs[tabId]
    if (!tab) return
    const st = this.stable.get(tabId)
    this.stable.set(tabId, { color, same: st && st.color === color ? st.same + 1 : 0, n: (st?.n ?? 0) + 1, at: Date.now() })
    const next = color ?? this.meta.get(tabId)
    if (next && tab.themeColor !== next) this.b.broadcast.patchTab(tabId, { themeColor: next })
  }
}

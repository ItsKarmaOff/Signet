import type { WebContents } from 'electron'
import type { Browser } from './browser'

/** Minimum quiet period after a swipe navigation (the page script already filters out the trackpad's leftover momentum: this delay only avoids a double trigger). */
const COOLDOWN = 120

/** Main side of swipe-to-navigate: the page script (src/preload/swipe.ts) queries, then requests the navigation. */
export class Swipe {
  private last = new WeakMap<WebContents, number>()

  constructor(private b: Browser) {}

  /** History state of the emitting tab; `enabled` false outside a tab, with the setting off or while momentum is in progress. */
  can(sender: WebContents): { enabled: boolean; back: boolean; forward: boolean } {
    const off = { enabled: false, back: false, forward: false }
    if (this.b.store.state.settings.swipeNavigation === false) return off
    if (!this.b.views.tabIdOfContents(sender.id)) return off
    if (Date.now() - (this.last.get(sender) ?? 0) < COOLDOWN) return off
    const h = sender.navigationHistory
    return { enabled: true, back: h.canGoBack(), forward: h.canGoForward() }
  }

  navigate(sender: WebContents, direction: 'back' | 'forward'): void {
    const info = this.can(sender)
    if (!info.enabled || !info[direction]) return
    this.last.set(sender, Date.now())
    if (direction === 'back') sender.navigationHistory.goBack()
    else sender.navigationHistory.goForward()
  }
}

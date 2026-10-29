/**
 * Navigations started from the command bar, per tab. The tab actions mark a tab when the palette loads a
 * page in it; the first main-frame navigation that then starts for that address is "ours" and, once it
 * commits (after redirects), the page counts as typed in the history. Any other navigation that starts
 * first (link, back/forward, a script) drops the marker.
 */

const normal = (url: string): string => {
  try {
    return new URL(url).href
  } catch {
    return url
  }
}

export class TypedNavigations {
  private pending = new Map<string, { url: string; started: boolean }>()

  /** The palette is about to load `url` in the tab. */
  mark(tabId: string, url: string): void {
    this.pending.set(tabId, { url: normal(url), started: false })
  }

  /** The marker was replaced by an unrelated load. */
  clear(tabId: string): void {
    this.pending.delete(tabId)
  }

  has(tabId: string): boolean {
    return this.pending.has(tabId)
  }

  /** A main-frame, cross-document navigation starts: ours (first, same address) or another one (marker dropped). */
  onStart(tabId: string, url: string): void {
    const p = this.pending.get(tabId)
    if (!p) return
    if (!p.started && p.url === normal(url)) p.started = true
    else this.pending.delete(tabId)
  }

  /** A navigation committed: true once if it is the one the palette started. */
  take(tabId: string): boolean {
    const p = this.pending.get(tabId)
    if (!p?.started) return false
    this.pending.delete(tabId)
    return true
  }
}

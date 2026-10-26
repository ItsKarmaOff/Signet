import type { Session, WebContents } from 'electron'
import { isLocalDevUrl } from '@shared/devhosts'
import type { Browser } from '../browser/browser'

/** Batching of counter updates (a page can write hundreds of errors at once). */
const PUSH_MS = 150
/** Network failures that are not page errors: cancelled navigation, request blocked by the ad blocker. */
const IGNORED_NET_ERRORS = new Set(['net::ERR_ABORTED', 'net::ERR_BLOCKED_BY_CLIENT'])

/**
 * Developer mode (Settings › Development): on local pages, number of errors and warnings written
 * to the console since the page loaded, and an option to keep them out of the history.
 */
export class DevMode {
  private counts = new Map<string, { errors: number; warnings: number }>()
  private timers = new Map<string, NodeJS.Timeout>()
  /** Known sessions, and whether the network listeners are attached to them. */
  private sessions = new Map<Session, boolean>()

  constructor(private b: Browser) {}

  get on(): boolean {
    return this.b.store.state.settings.devMode === true
  }

  /** Local page not to record in the history (developer mode option). */
  keepOutOfHistory(url: string): boolean {
    const s = this.b.store.state.settings
    return s.devMode === true && s.devKeepOutOfHistory === true && isLocalDevUrl(url)
  }

  attach(tabId: string, wc: WebContents): void {
    wc.on('console-message', (e) => {
      if (!this.on || (e.level !== 'error' && e.level !== 'warning') || !isLocalDevUrl(wc.getURL())) return
      this.bump(tabId, e.level)
    })
    // New page (not an internal navigation of a single-page app): the counter restarts from zero, like the console.
    wc.on('did-navigate', () => this.reset(tabId))
    wc.once('destroyed', () => this.reset(tabId))
  }

  /**
   * Failed resources (status 400 or more, network error): Chromium's console shows them as errors,
   * but without going through `console-message`. Once per session (see profiles.ts). The listeners are
   * attached only if developer mode is active (see `syncNetworkWatch`).
   */
  watchSession(ses: Session): void {
    this.sessions.set(ses, false)
    this.syncNetworkWatch()
  }

  /** Attaches or detaches the network listeners of all sessions depending on developer mode. */
  syncNetworkWatch(): void {
    const on = this.on
    for (const [ses, attached] of this.sessions) {
      if (attached === on) continue
      if (on) {
        const filter = { urls: ['http://*/*', 'https://*/*'] }
        ses.webRequest.onCompleted(filter, (d) => {
          if (d.statusCode >= 400) this.failed(d.webContentsId)
        })
        ses.webRequest.onErrorOccurred(filter, (d) => {
          if (!IGNORED_NET_ERRORS.has(d.error)) this.failed(d.webContentsId)
        })
      } else {
        ses.webRequest.onCompleted(null)
        ses.webRequest.onErrorOccurred(null)
      }
      this.sessions.set(ses, on)
    }
  }

  private failed(webContentsId: number | undefined): void {
    if (!this.on || webContentsId === undefined) return
    const tabId = this.b.views.tabIdOfContents(webContentsId)
    const wc = tabId ? this.b.views.viewOf(tabId)?.webContents : undefined
    if (tabId && wc && !wc.isDestroyed() && isLocalDevUrl(wc.getURL())) this.bump(tabId, 'error')
  }

  private bump(tabId: string, level: 'error' | 'warning'): void {
    const c = this.counts.get(tabId) ?? { errors: 0, warnings: 0 }
    if (level === 'error') c.errors++
    else c.warnings++
    this.counts.set(tabId, c)
    this.schedule(tabId)
  }

  /** Developer mode off: no more counters. */
  clearAll(): void {
    for (const id of [...this.counts.keys()]) this.reset(id)
  }

  private schedule(tabId: string): void {
    if (this.timers.has(tabId)) return
    this.timers.set(
      tabId,
      setTimeout(() => {
        this.timers.delete(tabId)
        const c = this.counts.get(tabId)
        if (c) this.b.broadcast.patchTab(tabId, { consoleErrors: c.errors || undefined, consoleWarnings: c.warnings || undefined })
      }, PUSH_MS)
    )
  }

  private reset(tabId: string): void {
    clearTimeout(this.timers.get(tabId))
    this.timers.delete(tabId)
    if (!this.counts.delete(tabId)) return
    this.b.broadcast.patchTab(tabId, { consoleErrors: undefined, consoleWarnings: undefined })
  }
}

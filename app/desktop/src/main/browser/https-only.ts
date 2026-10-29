import { randomUUID } from 'node:crypto'
import type { Session } from 'electron'
import type { Store } from '../core/store'
import { httpsUpgrade, isInsecureSiteError, isUpgradeOf } from '../core/https-only'

/** Upgrades of the same host within this delay, beyond which the site is considered to redirect back to http (loop). */
const LOOP_WINDOW = 10_000
const LOOP_LIMIT = 3

interface Attempt {
  httpUrl: string
  at: number
  count: number
}

/** A tab showing the "no secure connection" warning: the token proves a click comes from that page. */
interface Warning {
  httpUrl: string
  token: string
}

/** Prefix of the title the warning page sets to ask for "Continue to site" (followed by the token). */
export const CONTINUE_TITLE = 'signet-https-continue:'

/**
 * HTTPS-only mode (Settings › Privacy). Decides in the navigation events (not `session.webRequest`, which the ad
 * blocker and extensions use) and remembers, per tab, the http:// address that was upgraded so that a failure of
 * the https:// load can show a warning instead of an error page.
 */
export class HttpsOnly {
  /** Hosts the user allowed through the warning, per session (a private window has its own session): in memory only. */
  private allowed = new WeakMap<Session, Set<string>>()
  private attempts = new Map<string, Attempt>()
  private warnings = new Map<string, Warning>()

  constructor(private readonly d: { store: Pick<Store, 'state'> }) {}

  get enabled(): boolean {
    return this.d.store.state.settings.httpsOnly === true
  }

  /**
   * The address to load: the https:// version of an http:// one (noted for the tab), otherwise `url` itself.
   * Always `url` when the mode is off.
   */
  resolve(tabId: string, ses: Session, url: string): string {
    if (!this.enabled) return url
    const { devAliases } = this.d.store.state.settings
    const https = httpsUpgrade(url, { allowedHosts: this.allowed.get(ses), aliasTargets: devAliases?.map((a) => a.target) })
    if (!https) return url
    const now = Date.now()
    const prev = this.attempts.get(tabId)
    const again = prev && now - prev.at < LOOP_WINDOW && isSameHost(prev.httpUrl, url)
    const count = again ? prev.count + 1 : 1
    // A site that sends back to http each time we go to https: stop upgrading, the page loads over http.
    if (count > LOOP_LIMIT) return url
    this.attempts.set(tabId, { httpUrl: url, at: now, count })
    return https
  }

  /** The tab's current navigation is a typed address promoted to https:// (its http:// version is the fallback). */
  noteTyped(tabId: string, httpsUrl: string): void {
    if (!this.enabled) return
    this.attempts.set(tabId, { httpUrl: httpsUrl.replace(/^https:/i, 'http:'), at: Date.now(), count: 1 })
  }

  /**
   * A main-frame load failed: the http:// address to offer on the warning page if it is the upgraded load that has
   * no working secure connection, otherwise null (normal error page).
   */
  insecureFailure(tabId: string, failedUrl: string, code: number): string | null {
    const attempt = this.attempts.get(tabId)
    if (!this.enabled || !attempt || !isInsecureSiteError(code) || !isUpgradeOf(attempt.httpUrl, failedUrl)) return null
    return attempt.httpUrl
  }

  /** Records the warning shown in the tab and returns the token its page must send back. */
  warn(tabId: string, httpUrl: string): string {
    const token = randomUUID()
    this.warnings.set(tabId, { httpUrl, token })
    return token
  }

  /** The warning page asked to continue (its title carries the token): the http:// address to load, after allowing its host. */
  continueFrom(tabId: string, ses: Session, title: string): string | null {
    const warning = this.warnings.get(tabId)
    if (!warning || title !== CONTINUE_TITLE + warning.token) return null
    this.warnings.delete(tabId)
    this.allow(ses, warning.httpUrl)
    this.attempts.delete(tabId)
    return warning.httpUrl
  }

  /** Allows the host of this address for the session (until the app quits). */
  allow(ses: Session, url: string): void {
    try {
      const host = new URL(url).hostname.toLowerCase()
      const set = this.allowed.get(ses) ?? new Set<string>()
      set.add(host)
      this.allowed.set(ses, set)
    } catch {
      // not an address: nothing to allow
    }
  }

  /** A page committed: once the upgraded https:// page is there, its upgrade attempt is over (a redirect back to http keeps counting). */
  onCommitted(tabId: string, url: string): void {
    if (/^https:/i.test(url)) {
      const attempt = this.attempts.get(tabId)
      if (attempt && isUpgradeOf(attempt.httpUrl, url)) this.attempts.delete(tabId)
    }
  }

  forget(tabId: string): void {
    this.attempts.delete(tabId)
    this.warnings.delete(tabId)
  }
}

function isSameHost(a: string, b: string): boolean {
  try {
    return new URL(a).hostname.toLowerCase() === new URL(b).hostname.toLowerCase()
  } catch {
    return false
  }
}

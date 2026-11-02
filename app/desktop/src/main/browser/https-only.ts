import { randomUUID } from 'node:crypto'
import type { Session } from 'electron'
import type { Store } from '../core/store'
import { NetworkOrder, networkHub } from '../web/network'
import { httpsUpgrade, isInsecureSiteError, isUpgradeOf, type UpgradeOptions } from '../core/https-only'

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

const NAME = 'https-only'

/**
 * HTTPS-only mode (Settings › Privacy). The upgrade happens on the request (a consumer of the network hub, only while
 * the mode is on): Chromium follows the redirect internally as a 307, so a form keeps its method and data. It remembers,
 * per tab, the http:// address that was upgraded so that a failure of the https:// load can show a warning instead of
 * an error page.
 */
export class HttpsOnly {
  /** Hosts the user allowed through the warning, per session (a private window has its own session): in memory only. */
  private allowed = new WeakMap<Session, Set<string>>()
  private attempts = new Map<string, Attempt>()
  private warnings = new Map<string, Warning>()
  private sessions = new Set<Session>()

  constructor(private readonly d: { store: Pick<Store, 'state'>; tabOf: (webContentsId: number) => string | null }) {}

  get enabled(): boolean {
    return this.d.store.state.settings.httpsOnly === true
  }

  /** Adds a session (regular, isolated or private); the upgrade is active on it while the mode is on. */
  attach(ses: Session): void {
    this.sessions.add(ses)
    this.sync()
  }

  /** Removes a session (private window closed). */
  detach(ses: Session): void {
    if (!this.sessions.delete(ses)) return
    networkHub(ses).release(NAME)
  }

  /** Registers or releases the request consumer on all sessions, depending on the setting. */
  sync(): void {
    for (const ses of this.sessions) {
      const hub = networkHub(ses)
      if (!this.enabled) {
        hub.release(NAME)
        continue
      }
      // Pages only, like Chrome: frames and subresources are left to Chromium (mixed content rules); a frame upgraded
      // to an https:// that does not exist would fail without the warning page.
      hub.on('onBeforeRequest', NAME, NetworkOrder.httpsOnly, { urls: ['http://*/*'], types: ['mainFrame'] }, (d, cb) => {
        const https = this.upgradeRequest(ses, d)
        cb(https ? { redirectURL: https } : {})
      })
    }
  }

  /** The https:// address to redirect a document request to, or null to let it through. */
  private upgradeRequest(ses: Session, d: { url: string; resourceType: string; webContentsId?: number }): string | null {
    if (d.webContentsId === undefined) return null
    const tabId = this.d.tabOf(d.webContentsId)
    return tabId ? this.resolveFor(tabId, ses, d.url) : null
  }

  private options(ses: Session): UpgradeOptions {
    return { allowedHosts: this.allowed.get(ses), aliasTargets: this.d.store.state.settings.devAliases?.map((a) => a.target) }
  }

  /**
   * The address to load: the https:// version of an http:// one (noted for the tab), otherwise `url` itself.
   * Always `url` when the mode is off.
   */
  resolve(tabId: string, ses: Session, url: string): string {
    return this.resolveFor(tabId, ses, url) ?? url
  }

  /** Same as `resolve`, null instead of `url` when it is not upgraded. */
  private resolveFor(tabId: string, ses: Session, url: string): string | null {
    if (!this.enabled) return null
    const https = httpsUpgrade(url, this.options(ses))
    if (!https) return null
    const now = Date.now()
    const prev = this.attempts.get(tabId)
    const again = prev && now - prev.at < LOOP_WINDOW && isSameHost(prev.httpUrl, url)
    const count = again ? prev.count + 1 : 1
    // A site that sends back to http each time we go to https: stop upgrading, the page loads over http.
    if (count > LOOP_LIMIT) return null
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
    if (!this.enabled || !attempt || !isInsecureSiteError(code)) return null
    // The failed address is the https:// one (the redirect is followed internally); the http:// one is accepted just in case.
    if (!isUpgradeOf(attempt.httpUrl, failedUrl) && !(/^http:/i.test(failedUrl) && isSameHost(attempt.httpUrl, failedUrl))) return null
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

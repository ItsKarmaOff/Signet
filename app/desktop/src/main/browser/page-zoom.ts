import type { Session, WebContents } from 'electron'
import type { Store } from '../core/store'
import { defaultZoomFactor, isZoomPercent, rememberSiteZoom, reportedZoom, zoomHost, type SiteZoom } from '@shared/pagesettings'
import type { Views } from './views'

/**
 * Default page zoom (Settings › Appearance). Chromium shares one zoom level per host in a session (new views also start at the default through `webPreferences.zoomFactor`, see `pagePreferences`; this is the fallback) and Electron has no
 * default zoom, so: the hosts whose zoom the user changed with the zoom shortcuts are noted (per session, in
 * memory), and every other web page gets the default when it commits and when the setting changes.
 */
export type ZoomScope = { kind: 'shared' } | { kind: 'private' } | { kind: 'space'; id: string }

export class PageZoom {
  /** Hosts zoomed by the user, per session (a private window has its own session). */
  private userHosts = new WeakMap<Session, Set<string>>()

  constructor(
    private readonly d: {
      store: Pick<Store, 'state' | 'schedule'>
      views: Pick<Views, 'all'>
      /** Whose zoom memory a session uses: the shared one (normal spaces), an isolated space's own, or none (private window, never written to disk). */
      scope: (ses: Session) => ZoomScope
      /** A page's reported zoom (percent, undefined when it does not follow page zoom) changed. */
      report?: (wc: WebContents, percent: number | undefined) => void
    }
  ) {}

  /** Last zoom reported per page: the UI only hears about changes. */
  private reported = new WeakMap<WebContents, number | undefined>()

  /** Reports the page's current zoom if it changed since the last report. */
  sync(wc: WebContents): void {
    if (wc.isDestroyed()) return
    const percent = reportedZoom(wc.getURL(), wc.getZoomFactor()) ?? undefined
    if (this.reported.has(wc) && this.reported.get(wc) === percent) return
    this.reported.set(wc, percent)
    this.d.report?.(wc, percent)
  }

  /** Chromium shares the zoom of a host between the tabs of a session: they all re-report. */
  private syncSession(ses: Session): void {
    for (const view of this.d.views.all()) if (view.webContents.session === ses) this.sync(view.webContents)
  }

  private hostsOf(ses: Session): Set<string> {
    let set = this.userHosts.get(ses)
    if (!set) this.userHosts.set(ses, (set = new Set()))
    return set
  }

  /** The saved zoom of the session's scope, or null when nothing is written to disk (private window). */
  private memory(ses: Session, create = false): SiteZoom | null {
    const scope = this.d.scope(ses)
    if (scope.kind === 'private') return null
    if (scope.kind === 'shared') return this.d.store.state.siteZoom
    const all = this.d.store.state.siteZoomBySpace
    return create ? (all[scope.id] ??= {}) : (all[scope.id] ?? {})
  }

  private get percent(): unknown {
    return this.d.store.state.settings.defaultZoom
  }

  /** Applies the saved zoom of the site, otherwise the default, to a web page (a main-frame navigation committed, or the setting changed). */
  apply(wc: WebContents): void {
    this.applyZoom(wc)
    this.sync(wc)
  }

  private applyZoom(wc: WebContents): void {
    if (wc.isDestroyed()) return
    const host = zoomHost(wc.getURL())
    const saved = host ? this.memory(wc.session)?.[host] : undefined
    if (saved !== undefined) {
      if (Math.abs(wc.getZoomLevel() - saved) > 0.01) wc.setZoomLevel(saved)
      return
    }
    const factor = defaultZoomFactor({
      defaultPercent: this.percent,
      url: wc.getURL(),
      userHosts: this.hostsOf(wc.session),
      currentFactor: wc.getZoomFactor()
    })
    if (factor !== null) wc.setZoomFactor(factor)
  }

  /** The setting changed: open pages follow, except the sites the user zoomed. */
  applyAll(): void {
    for (const view of this.d.views.all()) this.apply(view.webContents)
  }

  /** The user zoomed the page with the shortcuts: its site keeps that zoom on top of the default. */
  userZoomed(wc: WebContents): void {
    const host = zoomHost(wc.getURL())
    if (!host) return
    this.hostsOf(wc.session).add(host)
    // Remembered across launches (not in a private window), so that a restart does not hand the site back to the default.
    const memory = this.memory(wc.session, true)
    if (memory) {
      rememberSiteZoom(memory, host, wc.getZoomLevel())
      this.d.store.schedule()
    }
    this.syncSession(wc.session)
  }

  /** Back to the default zoom (100 % when none is chosen, or for a page that does not follow it); the site follows the default again. */
  reset(wc: WebContents): void {
    const host = zoomHost(wc.getURL())
    if (host) {
      this.hostsOf(wc.session).delete(host)
      const memory = this.memory(wc.session)
      if (memory && delete memory[host]) this.d.store.schedule()
    }
    wc.setZoomFactor(host && isZoomPercent(this.percent) ? this.percent / 100 : 1)
    this.syncSession(wc.session)
  }
}

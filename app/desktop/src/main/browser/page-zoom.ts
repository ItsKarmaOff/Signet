import type { Session, WebContents } from 'electron'
import type { Store } from '../core/store'
import { defaultZoomFactor, isZoomPercent, rememberSiteZoom, zoomHost } from '@shared/pagesettings'
import type { Views } from './views'

/**
 * Default page zoom (Settings › Appearance). Chromium shares one zoom level per host in a session and Electron has no
 * default zoom, so: the hosts whose zoom the user changed with the zoom shortcuts are noted (per session, in
 * memory), and every other web page gets the default when it commits and when the setting changes.
 */
export class PageZoom {
  /** Hosts zoomed by the user, per session (a private window has its own session). */
  private userHosts = new WeakMap<Session, Set<string>>()

  constructor(
    private readonly d: {
      store: Pick<Store, 'state' | 'schedule'>
      views: Pick<Views, 'all'>
      /** False for a private window's session: its zoom choices are never written to disk. */
      persistent: (ses: Session) => boolean
    }
  ) {}

  private hostsOf(ses: Session): Set<string> {
    let set = this.userHosts.get(ses)
    if (!set) this.userHosts.set(ses, (set = new Set()))
    return set
  }

  private get percent(): unknown {
    return this.d.store.state.settings.defaultZoom
  }

  /** Applies the saved zoom of the site, otherwise the default, to a web page (a main-frame navigation committed, or the setting changed). */
  apply(wc: WebContents): void {
    if (wc.isDestroyed()) return
    const host = zoomHost(wc.getURL())
    const saved = host && this.d.persistent(wc.session) ? this.d.store.state.siteZoom[host] : undefined
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
    if (this.d.persistent(wc.session)) {
      rememberSiteZoom(this.d.store.state.siteZoom, host, wc.getZoomLevel())
      this.d.store.schedule()
    }
  }

  /** Back to the default zoom (100 % when none is chosen, or for a page that does not follow it); the site follows the default again. */
  reset(wc: WebContents): void {
    const host = zoomHost(wc.getURL())
    if (host) {
      this.hostsOf(wc.session).delete(host)
      if (delete this.d.store.state.siteZoom[host]) this.d.store.schedule()
    }
    wc.setZoomFactor(host && isZoomPercent(this.percent) ? this.percent / 100 : 1)
  }
}

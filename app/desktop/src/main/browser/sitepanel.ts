import { WebContentsView, type WebContents } from 'electron'
import type { PermissionState, Rect, SiteInfo } from '@shared/types'
import type { Browser } from './browser'
import { sessionFor } from '../web/profiles'
import { siteOf } from '../core/urls'
import { t } from '../app/i18n'

/** Permissions shown in the panel: Electron identifier and label. `media` covers camera and microphone. */
const PERMISSIONS = [
  { id: 'media', label: () => t('sitePanel.media') },
  { id: 'geolocation', label: () => t('sitePanel.geolocation') },
  { id: 'notifications', label: () => t('sitePanel.notifications') },
  { id: 'clipboard-read', label: () => t('sitePanel.clipboard-read') },
  { id: 'midi', label: () => t('sitePanel.midi') },
  { id: 'display-capture', label: () => t('sitePanel.display-capture') }
]
const STATES: PermissionState[] = ['ask', 'allow', 'deny']

/** Site panel: computes the active tab's `SiteInfo` and applies permission and data changes. */
export class SitePanel {
  constructor(private b: Browser) {}

  /** Origin of a web page, or null (internal pages, empty tab). */
  private originOf(url: string): string | null {
    try {
      const u = new URL(url)
      return u.protocol === 'http:' || u.protocol === 'https:' ? u.origin : null
    } catch {
      return null
    }
  }

  info(): SiteInfo | null {
    const tab = this.b.model.activeTab()
    const origin = tab ? this.originOf(tab.url) : null
    if (!tab || !origin) return null
    const boost = this.b.store.state.boosts[siteOf(tab.url)]
    const wcId = this.b.views.viewOf(tab.id)?.webContents.id
    return {
      tabId: tab.id,
      origin,
      site: siteOf(tab.url),
      secure: tab.url.startsWith('https:'),
      permissions: PERMISSIONS.map((p) => ({ id: p.id, label: p.label(), state: this.b.permissions.get(origin, p.id) ?? 'ask' })),
      boost: { exists: !!boost, enabled: !!boost?.enabled },
      favicon: tab.favicon,
      blocked: wcId !== undefined ? this.b.adblock.blockedCount(wcId) : 0
    }
  }

  /** `anchor` is expressed in the emitting view: we bring it back to window coordinates (floating toolbar). */
  open(sender: WebContents, anchor: Rect): void {
    const info = this.info()
    if (!info) return
    let dx = 0
    let dy = 0
    const win = this.b.win
    if (win && sender !== win.webContents) {
      const view = win.contentView.children.find((v): v is WebContentsView => v instanceof WebContentsView && v.webContents === sender)
      const bounds = view?.getBounds()
      if (bounds) {
        dx = bounds.x
        dy = bounds.y
      }
    }
    const num = (n: unknown) => (typeof n === 'number' && Number.isFinite(n) ? n : 0)
    this.b.overlay.openSite(info, {
      x: num(anchor?.x) + dx,
      y: num(anchor?.y) + dy,
      width: num(anchor?.width),
      height: num(anchor?.height)
    })
  }

  setPermission(origin: string, permission: string, state: PermissionState): void {
    if (!this.originOf(origin) || !PERMISSIONS.some((p) => p.id === permission) || !STATES.includes(state)) return
    this.b.permissions.set(origin, permission, state)
  }

  /** Clears the site's cookies and data on the tab's session, then reloads the tab. */
  async clearData(origin: string): Promise<void> {
    if (!this.originOf(origin)) return
    const tab = this.b.model.activeTab()
    const loc = tab ? this.b.model.locate(tab.id) : null
    const ses = this.b.views.viewOf(tab?.id ?? '')?.webContents.session ?? sessionFor(loc?.space?.id ?? null)
    await ses.clearStorageData({ origin })
    if (tab && this.originOf(tab.url) === origin) this.b.tabs.reload(tab.id, true)
  }
}

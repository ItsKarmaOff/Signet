// Link with the phone (no account): glues together the service (service.ts), the mDNS announcement, dev server detection
// and the UI (Settings › Devices, "Open on my phone" menus, toolbar indicator).
import { app, Menu, safeStorage, webContents, type MenuItemConstructorOptions } from 'electron'
import { hostname } from 'node:os'
import { join } from 'node:path'
import QRCode from 'qrcode'
import { isLoopbackUrl, toLinkLocalhost } from '@signet/shared/link/loopback'
import type { LinkDevice } from '@signet/shared/link/protocol'
import type { LinkState } from '@shared/types'
import type { Browser } from '../../browser/browser'
import { isUiContents, t } from '../../app/i18n'
import { menuIcon } from '../../app/menuicons'
import { pushUi } from '../../ipc/push'
import { Discovery } from './discovery'
import { DevServerScanner } from './devservers'
import { cleanHostName, computerName, desktopPlatform, lanAddresses } from './identity'
import { LinkService } from './service'
import { LinkStore, type Vault } from './store'

/** Dev servers are re-read at this frequency while a phone is active. */
const SCAN_MS = 5000

const vault: Vault = {
  available: () => safeStorage.isEncryptionAvailable(),
  encrypt: (text) => safeStorage.encryptString(text).toString('base64'),
  decrypt: (data) => {
    try {
      return safeStorage.decryptString(Buffer.from(data, 'base64'))
    } catch {
      return null
    }
  }
}

export class Link {
  private store: LinkStore | null = null
  private service: LinkService | null = null
  private discovery = new Discovery()
  private scanner: DevServerScanner | null = null
  private scanTimer: NodeJS.Timeout | null = null
  private name = cleanHostName(hostname())
  private qr: { linkId: string; qr: string } | null = null
  private listenError = false
  private last = ''

  constructor(private b: Browser) {}

  /** On app start: loads the links; only listens if the master switch is ON and a link exists. */
  start(): void {
    if (this.service) return
    const store = new LinkStore(join(app.getPath('userData'), 'link.json'), vault)
    this.store = store
    this.scanner = new DevServerScanner({
      ownPort: () => this.service?.port ?? null,
      tabs: () => Object.values(this.b.store.state.tabs).filter((tab) => isLoopbackUrl(tab.url))
    })
    this.service = new LinkService({
      store,
      desktop: () => this.desktop(),
      hosts: lanAddresses,
      hooks: {
        open: (url) => this.openHere(url),
        changed: () => this.changed(),
        listening: (port) => this.onListening(port)
      }
    })
    void computerName().then((name) => {
      this.name = name
      void this.service?.sync()
    })
    void this.service.sync()
  }

  /** App shutdown: announcement withdrawn, connections closed, file written. */
  async stop(): Promise<void> {
    if (this.scanTimer) clearInterval(this.scanTimer)
    this.scanTimer = null
    this.discovery.unpublish()
    await this.service?.close()
    this.store?.flush()
  }

  /** Immediate write of the links file. */
  flush(): void {
    this.store?.flush()
  }

  private desktop(): LinkDevice {
    return { id: this.store!.desktopId, name: this.name, platform: desktopPlatform() }
  }

  private onListening(port: number | null): void {
    if (port === null || !this.store) this.discovery.unpublish()
    else this.discovery.publish(this.name, this.store.desktopId, port)
  }

  // ───────────────────────── UI ─────────────────────────

  state(): LinkState {
    const svc = this.service
    const pending = svc?.pendingOffer
    return {
      general: this.store?.general ?? true,
      name: this.name,
      devices: (svc?.summaries() ?? [])
        .map(({ link, status }) => ({
          linkId: link.linkId,
          device: link.peer,
          enabled: link.enabled,
          status,
          createdAt: link.createdAt,
          lastSeen: link.lastSeen
        }))
        .sort((a, b) => a.createdAt - b.createdAt),
      active: svc?.activeLinks().length ?? 0,
      pairing: pending && this.qr?.linkId === pending.offer.linkId ? { qr: this.qr.qr, expiresAt: pending.offer.expiresAt } : null,
      listenError: this.listenError
    }
  }

  /** Something changed: the UI is updated (if the state really changed) and the scan starts or stops. */
  private changed(): void {
    this.syncScan()
    const state = this.state()
    const json = JSON.stringify(state)
    if (json === this.last) return
    this.last = json
    for (const wc of webContents.getAllWebContents()) if (isUiContents(wc)) pushUi(wc, 'link:state', state)
  }

  setGeneral(on: boolean): Promise<void> {
    this.listenError = false
    return this.service?.setGeneral(on) ?? Promise.resolve()
  }

  setDevice(linkId: string, on: boolean): Promise<void> {
    return this.service?.setDevice(linkId, on) ?? Promise.resolve()
  }

  forget(linkId: string): Promise<void> {
    return this.service?.forget(linkId) ?? Promise.resolve()
  }

  async startPairing(): Promise<boolean> {
    const svc = this.service
    if (!svc) return false
    this.listenError = false
    const res = await svc.startPairing()
    if (!res) {
      this.listenError = svc.general
      this.changed()
      return false
    }
    // The name may have changed since startup (System Settings).
    this.qr = {
      linkId: res.offer.linkId,
      qr: `data:image/svg+xml;base64,${Buffer.from(await QRCode.toString(res.url, { type: 'svg', margin: 2, errorCorrectionLevel: 'L' })).toString('base64')}`
    }
    this.changed()
    return true
  }

  cancelPairing(): void {
    this.service?.cancelPairing()
  }

  // ───────────────────────── Dev servers ─────────────────────────

  private syncScan(): void {
    const on = (this.service?.activeLinks().length ?? 0) > 0
    if (on && !this.scanTimer) {
      this.scanTimer = setInterval(() => void this.scan(), SCAN_MS)
      void this.scan()
    } else if (!on && this.scanTimer) {
      clearInterval(this.scanTimer)
      this.scanTimer = null
    }
  }

  private async scan(): Promise<void> {
    const list = await this.scanner?.scan()
    if (list && this.scanTimer) this.service?.setServers(list, this.scanner?.ports ?? [])
  }

  // ───────────────────────── Open ─────────────────────────

  /** The phone asks to open an address: new tab in the main window. */
  private openHere(url: string): void {
    this.b.window.show()
    this.b.tabs.open(url, { activate: true })
  }

  /** Phones a page can be sent to right now. */
  private targets(): { linkId: string; name: string }[] {
    return (this.service?.activeLinks() ?? []).map((l) => ({ linkId: l.linkId, name: l.peer.name }))
  }

  /** Address of a tab to send, or null (Signet page, file…). Local addresses go through `localhost`. */
  private urlOf(tabId: string | null): string | null {
    const url = tabId ? this.b.store.state.tabs[tabId]?.url : undefined
    return url && /^https?:\/\//i.test(url) ? toLinkLocalhost(url) : null
  }

  openOnPhone(tabId: string | null, linkId?: string): boolean {
    const url = this.urlOf(tabId)
    if (!url) return false
    const id = linkId ?? this.targets()[0]?.linkId
    return !!id && (this.service?.openOnPhone(id, url) ?? false)
  }

  /** A phone is active and this tab's page can be sent to it. */
  canOpenOnPhone(tabId: string | null): boolean {
    return this.targets().length > 0 && this.urlOf(tabId) !== null
  }

  /** Command bar: an active phone receives the page; if there are several, a menu asks which one. */
  openOnPhoneChoose(tabId: string | null): void {
    const targets = this.targets()
    if (targets.length === 1) this.openOnPhone(tabId, targets[0]!.linkId)
    else if (targets.length > 1)
      Menu.buildFromTemplate(targets.map((d) => ({ label: d.name, click: () => this.openOnPhone(tabId, d.linkId) }))).popup({ window: this.b.win ?? undefined })
  }

  /** "Open on my phone" menu entry (none if no phone is active or the page cannot be sent). */
  menuItems(tabId: string | null): MenuItemConstructorOptions[] {
    const targets = this.targets()
    if (!targets.length || !this.urlOf(tabId)) return []
    const label = t('menu.tab.openOnPhone')
    if (targets.length === 1) return [{ label, icon: menuIcon('phone'), click: () => this.openOnPhone(tabId, targets[0]!.linkId) }]
    return [{ label, icon: menuIcon('phone'), submenu: targets.map((d) => ({ label: d.name, click: () => this.openOnPhone(tabId, d.linkId) })) }]
  }
}

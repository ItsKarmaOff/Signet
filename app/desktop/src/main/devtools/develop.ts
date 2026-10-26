import type { MenuItemConstructorOptions, WebContents } from 'electron'
import type { DevOverrides, DevNetwork, DevResponsive, DevUserAgent, Rect } from '@shared/types'
import { DEVICES, DEFAULT_DEVICE, MAX_DEVICE, MIN_DEVICE, deviceSpec, responsiveProfile } from '@shared/devices'
import type { Browser } from '../browser/browser'
import { acceptLanguage } from '../web/languages'
import { defaultIdentity } from '../web/identity'
import { t } from '../app/i18n'
import { buildMenu } from '../app/menuicons'

/** Chromium major version, for plausible user agents. */
const CHROME = Number(process.versions.chrome?.split('.')[0]) || 140

/** Offered user agents: the text sent to sites and the matching `navigator.platform`. */
const USER_AGENTS: Record<DevUserAgent, { ua: string; platform: string }> = {
  'safari-mac': {
    ua: 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.0 Safari/605.1.15',
    platform: 'MacIntel'
  },
  'safari-iphone': {
    ua: 'Mozilla/5.0 (iPhone; CPU iPhone OS 18_6 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.0 Mobile/15E148 Safari/604.1',
    platform: 'iPhone'
  },
  'safari-ipad': {
    ua: 'Mozilla/5.0 (iPad; CPU OS 18_6 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.0 Mobile/15E148 Safari/604.1',
    platform: 'iPad'
  },
  'chrome-android': {
    ua: `Mozilla/5.0 (Linux; Android 10; K) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/${CHROME}.0.0.0 Mobile Safari/537.36`,
    platform: 'Linux armv81'
  },
  'firefox-mac': {
    ua: `Mozilla/5.0 (Macintosh; Intel Mac OS X 10.15; rv:${CHROME + 3}.0) Gecko/20100101 Firefox/${CHROME + 3}.0`,
    platform: 'MacIntel'
  }
}

/** Simulated connections, with Chromium's preset values (latency in ms, throughput in bytes/s). */
const NETWORKS: Record<DevNetwork, { offline: boolean; latency: number; down: number; up: number }> = {
  fast4g: { offline: false, latency: 165, down: (9_000_000 / 8) * 0.9, up: (1_500_000 / 8) * 0.9 },
  slow4g: { offline: false, latency: 562.5, down: (1_600_000 / 8) * 0.9, up: (750_000 / 8) * 0.9 },
  '3g': { offline: false, latency: 2000, down: (500_000 / 8) * 0.8, up: (500_000 / 8) * 0.8 },
  offline: { offline: true, latency: 0, down: 0, up: 0 }
}

const UA_IDS = Object.keys(USER_AGENTS) as DevUserAgent[]
/** Responsive mode: device bar above the page, margin around it (resize handles). */
export const RESPONSIVE_BAR = 40
const RESPONSIVE_PAD = 16
const NETWORK_IDS = Object.keys(NETWORKS) as DevNetwork[]

/** Active development settings, without default values (empty object = none). */
function clean(o: DevOverrides): DevOverrides {
  const out: DevOverrides = {}
  if (o.noCache) out.noCache = true
  if (o.noJs) out.noJs = true
  if (o.scheme === 'light' || o.scheme === 'dark') out.scheme = o.scheme
  if (o.reducedMotion) out.reducedMotion = true
  if (o.network && o.network in NETWORKS) out.network = o.network
  if (o.userAgent && o.userAgent in USER_AGENTS) out.userAgent = o.userAgent
  const r = o.responsive
  if (r && DEVICES.some((d) => d.id === r.device)) {
    const size = (n: unknown, fallback: number) =>
      typeof n === 'number' && Number.isFinite(n) ? Math.round(Math.min(MAX_DEVICE, Math.max(MIN_DEVICE, n))) : fallback
    const spec = deviceSpec(r.device)
    out.responsive = { device: r.device, width: size(r.width, spec.width), height: size(r.height, spec.height) }
    if (r.device === 'custom') {
      if (r.base && r.base !== 'custom' && DEVICES.some((d) => d.id === r.base)) out.responsive.base = r.base
      if (typeof r.mobile === 'boolean') out.responsive.mobile = r.mobile
    }
  }
  return out
}

/** Emulation sent for a device, a size and a scale (to resend it only on change). */
function metricsKey(r: DevResponsive, scale: number): string {
  const p = responsiveProfile(r)
  return `${r.device}:${r.width}x${r.height}:${p.dpr}${p.mobile ? 'm' : ''}@${scale.toFixed(4)}`
}

/** Simulated touch and mobile device (responsive mode): the page must be reloaded to see it. */
function isMobile(o: DevOverrides): boolean {
  return !!o.responsive && responsiveProfile(o.responsive).mobile
}

/** User agent in effect: the chosen one, otherwise the responsive mode device's. */
function effectiveUserAgent(o: DevOverrides): DevUserAgent | undefined {
  return o.userAgent ?? (o.responsive ? responsiveProfile(o.responsive).userAgent : undefined)
}

/**
 * Development menu (shown with developer mode): clear the cache, erase site data, and per-tab
 * settings (cache, JavaScript, theme, animations, connection, user agent) applied through the
 * debugging protocol. They last as long as the tab is open; nothing is saved.
 */
export class Develop {
  private overrides = new Map<string, DevOverrides>()
  /** Tabs whose protocol Network domain is enabled (needed for the cache and the simulated connection). */
  private network = new Set<number>()
  /** `webContents` whose closing we follow (once each). */
  private watched = new Set<number>()
  /** Responsive mode: last emulation sent per `webContents` (device, size, scale). */
  private metrics = new Map<number, string>()
  /** Responsive mode: last display scale per `webContents`. */
  private scales = new Map<number, number>()
  /** Responsive mode: scale frozen during a handle drag (the page grows without being shrunk). */
  private frozen = new Map<number, number>()

  constructor(private b: Browser) {}

  get(tabId: string): DevOverrides {
    return this.overrides.get(tabId) ?? {}
  }

  private wcOf(tabId: string | null): WebContents | null {
    const wc = tabId ? this.b.views.viewOf(tabId)?.webContents : undefined
    return wc && !wc.isDestroyed() ? wc : null
  }

  /** Changes a tab's settings; JavaScript and the user agent only change on reload, so the page is reloaded. */
  set(tabId: string | null, patch: Partial<DevOverrides>): void {
    const wc = this.wcOf(tabId)
    if (!tabId || !wc) return
    // Responsive mode only exists in the main window (Little Signet, private, app, Peek: no layout).
    if (patch.responsive && !this.inMainWindow(tabId)) patch = { ...patch, responsive: undefined }
    const before = this.get(tabId)
    const next = clean({ ...before, ...patch })
    // Debugger taken by another tool: nothing would be applied, so we remember nothing.
    if (Object.keys(next).length && !this.attach(wc)) return
    if (Object.keys(next).length) this.overrides.set(tabId, next)
    else this.overrides.delete(tabId)
    if (!this.watched.has(wc.id)) {
      const id = wc.id
      this.watched.add(id)
      // Debugger detached (other tool, built-in tools): the state caches are no longer reliable.
      wc.debugger.on('detach', () => {
        this.network.delete(id)
        this.metrics.delete(id)
        this.scales.delete(id)
      })
      wc.once('destroyed', () => {
        this.watched.delete(id)
        this.network.delete(id)
        this.metrics.delete(id)
        this.scales.delete(id)
        this.frozen.delete(id)
        // View destroyed (tab closed or unloaded): the new one starts without settings.
        if (this.overrides.delete(tabId)) this.b.broadcast.patchTab(tabId, { devOverrides: undefined })
      })
    }
    const reload = before.noJs !== next.noJs || effectiveUserAgent(before) !== effectiveUserAgent(next) || isMobile(before) !== isMobile(next)
    // Only the responsive mode size changes (handle drag): the rest is already applied.
    const rest = (o: DevOverrides) => JSON.stringify({ ...o, responsive: undefined })
    const sizeOnly =
      !!before.responsive &&
      !!next.responsive &&
      before.responsive.device === next.responsive.device &&
      isMobile(before) === isMobile(next) &&
      rest(before) === rest(next)
    void (async () => {
      if (!sizeOnly && !(await this.apply(tabId, wc))) return
      if (wc.isDestroyed()) return
      // The device is emulated before the reload (the layout will fix the scale if needed).
      if (next.responsive && JSON.stringify(before.responsive) !== JSON.stringify(next.responsive)) {
        const scale = this.scales.get(wc.id) ?? 1
        this.metrics.set(wc.id, metricsKey(next.responsive, scale))
        await this.emulate(wc, next.responsive, scale)
      }
      if (reload && !wc.isDestroyed()) wc.reload()
    })().catch(() => {})
    this.b.broadcast.patchTab(tabId, { devOverrides: Object.keys(next).length ? next : undefined })
    // `devOverrides` does not touch the layout, except responsive mode (page view size).
    if (JSON.stringify(before.responsive) !== JSON.stringify(next.responsive)) this.b.broadcast.changed({ persist: false })
  }

  /** Main window tab (not detached: Little Signet, private, app, Peek). */
  private inMainWindow(tabId: string): boolean {
    return !this.b.store.detached.has(tabId) && tabId !== this.b.peek.tabId
  }

  /** Attaches the debugger if it is not attached; false if another tool holds it. */
  private attach(wc: WebContents): boolean {
    try {
      if (!wc.debugger.isAttached()) wc.debugger.attach('1.3')
      return true
    } catch {
      return false
    }
  }

  /** All of the tab's settings return to normal. */
  reset(tabId: string | null): void {
    if (tabId && this.overrides.has(tabId))
      this.set(tabId, {
        noCache: false,
        noJs: false,
        scheme: undefined,
        reducedMotion: false,
        network: undefined,
        userAgent: undefined,
        responsive: undefined
      })
  }

  /** Reapplies the user agent after a language change (`emulateLanguages` resets it to the default). */
  reapplyAll(): void {
    for (const id of this.overrides.keys()) {
      const wc = this.wcOf(id)
      if (wc) void this.apply(id, wc).catch(() => {})
    }
  }

  private async send(wc: WebContents, method: string, params: object): Promise<unknown> {
    if (wc.isDestroyed()) return
    try {
      return await wc.debugger.sendCommand(method, params)
    } catch (error) {
      console.warn(`Development: ${method} refused:`, error)
    }
  }

  /** Applies the settings; false if nothing could be (debugger taken by another tool, view destroyed). */
  private async apply(tabId: string, wc: WebContents): Promise<boolean> {
    const o = this.get(tabId)
    const send = (method: string, params: object) => this.send(wc, method, params)
    if (wc.isDestroyed() || !this.attach(wc)) return false
    const needsNetwork = !!o.noCache || !!o.network
    if (needsNetwork && !this.network.has(wc.id)) {
      await send('Network.enable', { maxTotalBufferSize: 1_000_000, maxResourceBufferSize: 500_000 })
      this.network.add(wc.id)
    }
    if (needsNetwork || this.network.has(wc.id)) {
      const n = o.network ? NETWORKS[o.network] : null
      await send('Network.setCacheDisabled', { cacheDisabled: !!o.noCache })
      // Recent Chromium: per-rule conditions (empty pattern = all requests); the old command stays as a fallback.
      const conditions = { latency: n?.latency ?? 0, downloadThroughput: n ? n.down : -1, uploadThroughput: n ? n.up : -1 }
      const byRule =
        !wc.isDestroyed() &&
        (await wc.debugger
          .sendCommand('Network.emulateNetworkConditionsByRule', {
            offline: n?.offline ?? false,
            matchedNetworkConditions: n ? [{ urlPattern: '', ...conditions }] : []
          })
          .then(() => true)
          .catch(() => false))
      if (!byRule) await send('Network.emulateNetworkConditions', { offline: n?.offline ?? false, ...conditions })
    }
    if (!needsNetwork && this.network.has(wc.id)) {
      await send('Network.disable', {})
      this.network.delete(wc.id)
    }
    await send('Emulation.setScriptExecutionDisabled', { value: !!o.noJs })
    await send('Emulation.setEmulatedMedia', {
      media: '',
      features: [
        { name: 'prefers-color-scheme', value: o.scheme ?? '' },
        { name: 'prefers-reduced-motion', value: o.reducedMotion ? 'reduce' : '' }
      ]
    })
    // Same command as languages (languages.ts): the chosen agent, or the default one, always with the languages.
    const chosen = effectiveUserAgent(o)
    const ua = chosen ? USER_AGENTS[chosen] : null
    const languages = acceptLanguage(this.b.store.state.settings.languages)
    // Chosen agent: as is; otherwise the default identity, client hints included (see identity.ts).
    await send('Emulation.setUserAgentOverride', ua ? { userAgent: ua.ua, acceptLanguage: languages, platform: ua.platform } : defaultIdentity(languages))
    // Leaving responsive mode: the page takes its view's size back (the emulation itself starts from `place`).
    if (!o.responsive && this.metrics.delete(wc.id)) {
      await send('Emulation.clearDeviceMetricsOverride', {})
      await send('Emulation.setTouchEmulationEnabled', { enabled: false })
      await send('Emulation.setEmitTouchEventsForMouse', { enabled: false })
    }
    return true
  }

  // ───────────────────────── Responsive mode ─────────────────────────

  /** Enters responsive mode with the last device used, or leaves it. */
  toggleResponsive(tabId: string | null): void {
    if (!tabId) return
    if (this.get(tabId).responsive) return this.setResponsive(tabId, null)
    const last = this.b.store.state.settings.responsive
    const spec = deviceSpec(last?.device ?? DEFAULT_DEVICE)
    this.setResponsive(tabId, last ?? { device: spec.id, width: spec.width, height: spec.height })
  }

  /** Start / end of a handle drag: the scale is frozen at the start, then the page is fitted to the panel again. */
  setDragging(tabId: string, dragging: boolean): void {
    const wc = this.wcOf(tabId)
    if (!wc || !this.get(tabId).responsive) return
    if (dragging) this.frozen.set(wc.id, this.scales.get(wc.id) ?? 1)
    else if (this.frozen.delete(wc.id)) this.b.broadcast.changed({ persist: false })
  }

  /** Device and size of responsive mode (remembered for next time), or null to leave it. */
  setResponsive(tabId: string | null, responsive: DevResponsive | null): void {
    this.set(tabId, { responsive: responsive ?? undefined })
    const kept = tabId ? this.get(tabId).responsive : undefined
    if (kept) {
      this.b.store.state.settings.responsive = kept
      this.b.store.schedule(1500) // during a handle drag, one write every 1.5 s at most
    }
  }

  /**
   * Places a responsive-mode tab's page in `rect` (the panel's web view area): device bar at the
   * top, page at the device's size (shrunk if there is not enough room), centered. Sends the emulation when it changes.
   */
  place(tabId: string, rect: Rect): { frame: Rect; bar: Rect; area: Rect; scale: number; responsive: DevResponsive } | null {
    const r = this.get(tabId).responsive
    const wc = this.wcOf(tabId)
    if (!r || !wc) return null
    const bar = { x: rect.x, y: rect.y, width: rect.width, height: RESPONSIVE_BAR }
    const room = { width: Math.max(1, rect.width - RESPONSIVE_PAD * 2), height: Math.max(1, rect.height - RESPONSIVE_BAR - RESPONSIVE_PAD) }
    const frozen = this.frozen.get(wc.id)
    const scale = frozen ?? Math.min(1, room.width / r.width, room.height / r.height)
    // Frozen scale: the page grows on screen up to the panel edges (beyond that, it is cropped).
    const width = Math.max(1, Math.min(Math.round(r.width * scale), frozen ? room.width : Infinity))
    const height = Math.max(1, Math.min(Math.round(r.height * scale), frozen ? room.height : Infinity))
    const frame = { x: rect.x + Math.round((rect.width - width) / 2), y: rect.y + RESPONSIVE_BAR, width, height }
    const key = metricsKey(r, scale)
    this.scales.set(wc.id, scale)
    if (this.metrics.get(wc.id) !== key) {
      this.metrics.set(wc.id, key)
      void this.emulate(wc, r, scale).catch(() => {})
    }
    return { frame, bar, area: rect, scale, responsive: r }
  }

  /** Size, density, orientation and touch of the device; the page is drawn at its view's scale. */
  private async emulate(wc: WebContents, r: DevResponsive, scale: number): Promise<void> {
    const spec = responsiveProfile(r)
    const landscape = r.width > r.height
    if (wc.isDestroyed() || !this.attach(wc)) return
    await this.send(wc, 'Emulation.setDeviceMetricsOverride', {
      width: r.width,
      height: r.height,
      deviceScaleFactor: spec.dpr,
      mobile: spec.mobile,
      scale,
      screenWidth: r.width,
      screenHeight: r.height,
      ...(spec.mobile && { screenOrientation: landscape ? { type: 'landscapePrimary', angle: 90 } : { type: 'portraitPrimary', angle: 0 } })
    })
    await this.send(wc, 'Emulation.setTouchEmulationEnabled', spec.mobile ? { enabled: true, maxTouchPoints: 5 } : { enabled: false })
    await this.send(wc, 'Emulation.setEmitTouchEventsForMouse', { enabled: spec.mobile, configuration: 'mobile' })
  }

  /** Clears the cache (of the tab's whole session) then reloads the page without cache. */
  async emptyCacheAndReload(tabId: string | null): Promise<void> {
    const wc = this.wcOf(tabId)
    if (!wc) return
    await wc.session.clearCache()
    if (!wc.isDestroyed()) wc.reloadIgnoringCache()
  }

  /** Erases cookies and storage of the page's origin (tab session), then reloads it. */
  async clearSiteData(tabId: string | null): Promise<void> {
    const wc = this.wcOf(tabId)
    if (!wc) return
    const url = wc.getURL()
    if (!/^https?:/i.test(url)) return
    const origin = new URL(url).origin
    await wc.session.clearStorageData({ origin })
    if (!wc.isDestroyed()) wc.reloadIgnoringCache()
  }

  /**
   * Development menu items. `target` gives the targeted tab at click time; the checkmarks follow `shown`
   * (tab whose state is displayed: the active tab for the app menu, the button's tab for its menu).
   */
  menuItems(target: () => string | null, shown: string | null, full: boolean): MenuItemConstructorOptions[] {
    const o = shown ? this.get(shown) : {}
    const enabled = !!this.wcOf(shown)
    const set = (patch: Partial<DevOverrides>) => () => this.set(target(), patch)
    const check = (label: string, checked: boolean, patch: Partial<DevOverrides>): MenuItemConstructorOptions => ({
      label,
      type: 'checkbox',
      checked,
      enabled,
      click: set(patch)
    })
    const radio = (label: string, checked: boolean, patch: Partial<DevOverrides>): MenuItemConstructorOptions => ({
      label,
      type: 'radio',
      checked,
      enabled,
      click: set(patch)
    })
    return [
      ...(full
        ? ([
            { label: t('develop.emptyCache'), enabled, click: () => void this.emptyCacheAndReload(target()) },
            { label: t('develop.clearSiteData'), enabled, click: () => void this.clearSiteData(target()) },
            { type: 'separator' }
          ] satisfies MenuItemConstructorOptions[])
        : []),
      ...(full
        ? []
        : ([
            { label: t('develop.responsive'), type: 'checkbox', checked: !!o.responsive, enabled, click: () => this.toggleResponsive(target()) },
            { type: 'separator' }
          ] satisfies MenuItemConstructorOptions[])),
      check(t('develop.noCache'), !!o.noCache, { noCache: !o.noCache }),
      check(t('develop.noJs'), !!o.noJs, { noJs: !o.noJs }),
      { type: 'separator' },
      {
        label: t('develop.scheme'),
        enabled,
        submenu: [
          radio(t('develop.schemeSystem'), !o.scheme, { scheme: undefined }),
          radio(t('develop.schemeLight'), o.scheme === 'light', { scheme: 'light' }),
          radio(t('develop.schemeDark'), o.scheme === 'dark', { scheme: 'dark' })
        ]
      },
      check(t('develop.reducedMotion'), !!o.reducedMotion, { reducedMotion: !o.reducedMotion }),
      {
        label: t('develop.networkTitle'),
        enabled,
        submenu: [
          radio(t('develop.network.none'), !o.network, { network: undefined }),
          { type: 'separator' },
          ...NETWORK_IDS.map((id) => radio(t(`develop.network.${id}`), o.network === id, { network: id }))
        ]
      },
      {
        label: t('develop.uaTitle'),
        enabled,
        submenu: [
          radio(t('develop.ua.default'), !o.userAgent, { userAgent: undefined }),
          { type: 'separator' },
          ...UA_IDS.map((id) => radio(t(`develop.ua.${id}`), o.userAgent === id, { userAgent: id }))
        ]
      },
      { type: 'separator' },
      { label: t('develop.reset'), enabled: enabled && Object.keys(o).length > 0, click: () => this.reset(target()) }
    ]
  }

  /** Button menu in the address bar (the tab's active settings). */
  showMenu(tabId: string): void {
    buildMenu(this.menuItems(() => tabId, tabId, false)).popup(this.b.win && !this.b.win.isDestroyed() ? { window: this.b.win } : undefined)
  }

  /** Command bar commands (developer mode) for the active tab. */
  paletteActions(tabId: string): { id: string; title: string; run: () => void }[] {
    const o = this.get(tabId)
    const act = (id: string, title: string, run: () => void) => ({ id: `develop:${id}`, title, run })
    return [
      act('responsive', t(o.responsive ? 'develop.palette.responsiveOff' : 'develop.palette.responsiveOn'), () => this.toggleResponsive(tabId)),
      act('empty-cache', t('develop.emptyCache'), () => void this.emptyCacheAndReload(tabId)),
      act('clear-site', t('develop.clearSiteData'), () => void this.clearSiteData(tabId)),
      act('no-cache', t(o.noCache ? 'develop.palette.enableCache' : 'develop.palette.disableCache'), () => this.set(tabId, { noCache: !o.noCache })),
      act('no-js', t(o.noJs ? 'develop.palette.enableJs' : 'develop.palette.disableJs'), () => this.set(tabId, { noJs: !o.noJs })),
      ...(['light', 'dark'] as const)
        .filter((s) => o.scheme !== s)
        .map((s) => act(`scheme-${s}`, t(s === 'dark' ? 'develop.palette.forceDark' : 'develop.palette.forceLight'), () => this.set(tabId, { scheme: s }))),
      ...(o.scheme ? [act('scheme-system', t('develop.palette.followSystem'), () => this.set(tabId, { scheme: undefined }))] : []),
      act('motion', t(o.reducedMotion ? 'develop.palette.motionOn' : 'develop.palette.motionOff'), () => this.set(tabId, { reducedMotion: !o.reducedMotion })),
      ...NETWORK_IDS.filter((id) => o.network !== id).map((id) =>
        act(`net-${id}`, t('develop.palette.network', { name: t(`develop.network.${id}`) }), () => this.set(tabId, { network: id }))
      ),
      ...(o.network ? [act('net-none', t('develop.palette.networkNone'), () => this.set(tabId, { network: undefined }))] : []),
      ...UA_IDS.filter((id) => o.userAgent !== id).map((id) =>
        act(`ua-${id}`, t('develop.palette.ua', { name: t(`develop.ua.${id}`) }), () => this.set(tabId, { userAgent: id }))
      ),
      ...(o.userAgent ? [act('ua-default', t('develop.palette.uaDefault'), () => this.set(tabId, { userAgent: undefined }))] : []),
      ...(Object.keys(o).length ? [act('reset', t('develop.reset'), () => this.reset(tabId))] : [])
    ]
  }
}

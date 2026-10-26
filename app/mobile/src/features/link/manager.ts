// Manager of the link with computers: control connections, mDNS discovery, pairing, localhost relay, "open on…".
// Rules: packages/shared/src/link/README.md. Pure logic (address order, waiting, computer choice):
// @signet/shared/mobile/link. Without the native module (Expo Go), nothing connects and everything stays inert.
import { AppState, Platform } from 'react-native'
import {
  LINK_PROTOCOL_VERSION,
  encodeLinkMessage,
  parseLinkMessage,
  serverNameForLink,
  type LinkDevice,
  type LinkMessage,
  type PairingOffer
} from '@signet/shared/link/protocol'
import { isLoopbackUrl, loopbackPort, toLinkLocalhost } from '@signet/shared/link/loopback'
import { pickLocalhostRoute } from '@signet/shared/link/status'
import {
  PING_INTERVAL_MS,
  PING_TIMEOUT_MS,
  activeLinkIds,
  backoffMs,
  controlAction,
  orderEndpoints,
  rememberEndpoint,
  sortServers,
  type HostPort,
  type StoredLink
} from '@signet/shared/mobile/link'
import { SignetLink, type PinnedEndpoint, type ServiceFound } from '../../../modules/signet-link'
import { t } from '@/lib/i18n'
import { useStore } from '@/state'
import { toast } from '@/ui'
import { clearRuntime, patchRuntime, useLinkRuntime } from './runtime'
import { deleteToken, getPhoneId, getToken, setToken } from './secrets'

const OPEN_TIMEOUT_MS = 3000
const HELLO_TIMEOUT_MS = 6000
const PAIR_TIMEOUT_MS = 8000

/** A line ready for `writeLine` (the module adds the `\n`). */
const line = (msg: LinkMessage): string => encodeLinkMessage(msg).slice(0, -1)

interface Conn {
  socketId: string
  connected: boolean
  /** Resolves the handshake (`welcome`, error or close). */
  settle?: (r: 'ok' | 'invalid' | 'fail') => void
}

interface LinkState {
  gen: number
  conn: Conn | null
  attempting: boolean
  attempt: number
  retryTimer: ReturnType<typeof setTimeout> | null
  pingTimer: ReturnType<typeof setInterval> | null
  pongTimer: ReturnType<typeof setTimeout> | null
}

const states = new Map<string, LinkState>()
const handlers = new Map<string, { line: (raw: string) => void; closed: (error?: string) => void }>()
const services = new Map<string, ServiceFound>()
let foreground = AppState.currentState !== 'background'
let browsing = false
/** DEV: demo links (`link-seed`) must be neither connected nor invalidated. */
let frozen = false
let socketCounter = 0

const stateOf = (linkId: string): LinkState => {
  let s = states.get(linkId)
  if (!s) {
    s = { gen: 0, conn: null, attempting: false, attempt: 0, retryTimer: null, pingTimer: null, pongTimer: null }
    states.set(linkId, s)
  }
  return s
}

const linkOf = (linkId: string): StoredLink | undefined => useStore.getState().links.find((l) => l.linkId === linkId)

/** Addresses seen via mDNS for this computer. */
const servicesFor = (desktopId: string): HostPort[] =>
  [...services.values()].filter((s) => s.desktopId === desktopId).flatMap((s) => s.hosts.map((host) => ({ host, port: s.port })))

const errorCode = (e: unknown): string => {
  const x = e as { code?: unknown; message?: unknown }
  return String(x?.code ?? x?.message ?? e)
}

const endpointOf = (link: StoredLink, c: HostPort): PinnedEndpoint => ({
  host: c.host,
  port: c.port,
  serverName: serverNameForLink(link.linkId),
  certSha256: link.certSha256
})

async function phoneDevice(): Promise<LinkDevice> {
  const name = (SignetLink?.deviceName() ?? '').trim().slice(0, 64)
  return { id: await getPhoneId(), name: name || 'Phone', platform: Platform.OS === 'ios' ? 'ios' : 'android' }
}

// --- Control: one connection per computer ---

function clearTimers(s: LinkState): void {
  if (s.retryTimer) clearTimeout(s.retryTimer)
  if (s.pingTimer) clearInterval(s.pingTimer)
  if (s.pongTimer) clearTimeout(s.pongTimer)
  s.retryTimer = s.pingTimer = s.pongTimer = null
}

/** Stops a link's connections and attempts. `pause`: notifies the computer first (`status paused:true`). */
function stopLink(linkId: string, pause = false): void {
  const s = states.get(linkId)
  if (!s) return
  s.gen++
  clearTimers(s)
  const conn = s.conn
  s.conn = null
  s.attempting = false
  s.attempt = 0
  if (conn) {
    conn.settle?.('fail')
    const close = () => {
      handlers.delete(conn.socketId)
      SignetLink?.close(conn.socketId)
    }
    // The handler stays for the duration of the write: the close that follows restarts nothing (s.conn is already empty).
    if (pause && conn.connected) SignetLink?.writeLine(conn.socketId, line({ type: 'status', paused: true })).then(close, close)
    else close()
  }
  patchRuntime(linkId, { connected: false, connecting: false, servers: [], ports: [] })
}

function scheduleRetry(linkId: string): void {
  const s = stateOf(linkId)
  if (s.retryTimer) return
  const delay = backoffMs(s.attempt++)
  s.retryTimer = setTimeout(() => {
    s.retryTimer = null
    if (wants(linkId)) void connect(linkId)
  }, delay)
}

/** This link must be connected now (app in the foreground, everything ON, valid link). */
function wants(linkId: string): boolean {
  const st = useStore.getState()
  const link = linkOf(linkId)
  return !!SignetLink && foreground && st.linksLoaded && !!link && !link.invalid && link.enabled && st.settings.linkEnabled
}

function markInvalid(linkId: string): void {
  useStore.getState().patchLink(linkId, { invalid: true })
  stopLink(linkId)
  void deleteToken(linkId)
}

async function connect(linkId: string): Promise<void> {
  const s = stateOf(linkId)
  if (s.attempting || s.conn) return
  const gen = ++s.gen
  s.attempting = true
  if (s.attempt === 0) patchRuntime(linkId, { connecting: true })
  const link = linkOf(linkId)
  const token = link ? await getToken(linkId) : null
  if (gen !== s.gen) return
  if (!link || !token) {
    s.attempting = false
    if (link) markInvalid(linkId)
    return
  }
  for (const c of orderEndpoints(servicesFor(link.desktop.id), link)) {
    if (gen !== s.gen) return
    const r = await tryControl(link, token, c, c.source === 'mdns', gen)
    if (gen !== s.gen) return
    if (r === 'ok') {
      s.attempting = false
      return
    }
    if (r === 'invalid') {
      s.attempting = false
      markInvalid(linkId)
      return
    }
  }
  s.attempting = false
  patchRuntime(linkId, { connecting: false })
  scheduleRetry(linkId)
}

async function tryControl(link: StoredLink, token: string, c: HostPort, fromMdns: boolean, gen: number): Promise<'ok' | 'invalid' | 'fail'> {
  const s = stateOf(link.linkId)
  const conn: Conn = { socketId: `ctl-${++socketCounter}`, connected: false }
  const id = conn.socketId
  const result = new Promise<'ok' | 'invalid' | 'fail'>((resolve) => {
    conn.settle = resolve
  })
  handlers.set(id, { line: (raw) => onControlLine(link.linkId, conn, raw), closed: () => onControlClosed(link.linkId, conn) })
  try {
    await SignetLink!.open(id, endpointOf(link, c), OPEN_TIMEOUT_MS)
  } catch (e) {
    handlers.delete(id)
    // A different certificate at an address the computer itself announces via mDNS: it deleted this link (default certificate).
    return fromMdns && errorCode(e).includes('pin-mismatch') ? 'invalid' : 'fail'
  }
  if (gen !== s.gen) {
    handlers.delete(id)
    SignetLink!.close(id)
    return 'fail'
  }
  s.conn = conn
  const timer = setTimeout(() => conn.settle?.('fail'), HELLO_TIMEOUT_MS)
  try {
    const device = await phoneDevice()
    await SignetLink!.writeLine(id, line({ type: 'hello', v: LINK_PROTOCOL_VERSION, linkId: link.linkId, token, role: 'control', device }))
  } catch {
    conn.settle?.('fail')
  }
  const r = await result
  clearTimeout(timer)
  if (r === 'ok') {
    useStore.getState().patchLink(link.linkId, { last: rememberEndpoint(linkOf(link.linkId)?.last ?? [], c), invalid: false })
    return r
  }
  // Failure: close cleanly before the next address.
  if (s.conn === conn) s.conn = null
  handlers.delete(id)
  SignetLink!.close(id)
  return r
}

function onControlLine(linkId: string, conn: Conn, raw: string): void {
  const s = stateOf(linkId)
  if (s.conn !== conn) return
  if (s.pongTimer) {
    clearTimeout(s.pongTimer)
    s.pongTimer = null
  }
  const msg = parseLinkMessage(raw)
  if (!msg) return
  const store = useStore.getState()
  if (!conn.connected) {
    if (msg.type === 'welcome') {
      conn.connected = true
      s.attempt = 0
      store.patchLink(linkId, { peerPaused: msg.paused, invalid: false })
      patchRuntime(linkId, { connected: true, connecting: false })
      startPing(linkId, conn)
      conn.settle?.('ok')
    } else if (msg.type === 'error') {
      conn.settle?.(msg.code === 'unknown-link' || msg.code === 'bad-token' ? 'invalid' : 'fail')
    }
    return
  }
  switch (msg.type) {
    case 'status':
      store.patchLink(linkId, { peerPaused: msg.paused })
      if (msg.paused) patchRuntime(linkId, { servers: [] })
      break
    case 'servers':
      if (!linkOf(linkId)?.peerPaused) patchRuntime(linkId, { servers: sortServers(msg.servers), ports: msg.ports })
      break
    case 'open':
      void handleOpen(linkId, msg.url)
      break
    case 'forget': {
      // The computer deleted the link: delete it here too.
      const name = linkOf(linkId)?.desktop.name ?? t('link.computer')
      void removeLocally(linkId).then(() => toast.show({ message: t('link.forgottenThere', { name }), duration: 5000 }))
      break
    }
    case 'ping':
      SignetLink?.writeLine(conn.socketId, line({ type: 'pong' })).catch(() => undefined)
      break
  }
}

function onControlClosed(linkId: string, conn: Conn): void {
  handlers.delete(conn.socketId)
  const s = stateOf(linkId)
  conn.settle?.('fail')
  if (s.conn !== conn) return
  s.conn = null
  if (s.pingTimer) clearInterval(s.pingTimer)
  if (s.pongTimer) clearTimeout(s.pongTimer)
  s.pingTimer = s.pongTimer = null
  patchRuntime(linkId, { connected: false, servers: [] })
  if (conn.connected && wants(linkId)) {
    s.attempt = 0
    scheduleRetry(linkId)
  }
}

function startPing(linkId: string, conn: Conn): void {
  const s = stateOf(linkId)
  s.pingTimer = setInterval(() => {
    if (s.conn !== conn || s.pongTimer) return
    SignetLink?.writeLine(conn.socketId, line({ type: 'ping' })).catch(() => undefined)
    s.pongTimer = setTimeout(() => {
      s.pongTimer = null
      if (s.conn !== conn) return
      SignetLink?.close(conn.socketId)
      onControlClosed(linkId, conn)
    }, PING_TIMEOUT_MS)
  }, PING_INTERVAL_MS)
}

// --- Reconciling the desired state with the connections ---

function sync(): void {
  if (!SignetLink || frozen) return
  const st = useStore.getState()
  if (!st.linksLoaded) return
  for (const id of [...states.keys()]) {
    if (!st.links.some((l) => l.linkId === id)) {
      stopLink(id)
      states.delete(id)
      clearRuntime(id)
    }
  }
  let anyWanted = false
  for (const link of st.links) {
    const s = stateOf(link.linkId)
    const enabledHere = st.settings.linkEnabled && link.enabled
    const wanted = foreground && enabledHere && !link.invalid
    anyWanted ||= wanted
    const action = controlAction({
      wantConnected: foreground,
      hasConnection: !!s.conn || s.attempting || s.retryTimer !== null,
      connected: !!s.conn?.connected,
      enabledHere,
      invalid: link.invalid
    })
    if (action === 'connect') {
      s.attempt = 0
      void connect(link.linkId)
    } else if (action === 'pause') stopLink(link.linkId, true)
    else if (action === 'close') stopLink(link.linkId)
  }
  if (anyWanted && !browsing) {
    browsing = true
    SignetLink.startBrowsing()
  } else if (!anyWanted && browsing) {
    browsing = false
    SignetLink.stopBrowsing()
    services.clear()
  }
  void applyRoute()
}

function onServiceFound(svc: ServiceFound): void {
  services.set(svc.name, svc)
  // The computer just appeared: no need to wait for the wait to end.
  for (const link of useStore.getState().links) {
    if (link.desktop.id !== svc.desktopId) continue
    const s = stateOf(link.linkId)
    if (s.retryTimer && !s.conn && !s.attempting) {
      clearTimeout(s.retryTimer)
      s.retryTimer = null
      if (wants(link.linkId)) void connect(link.linkId)
    }
  }
  void applyRoute()
}

// --- Localhost relay ---

let routeKey = 'null'
let routing: Promise<void> = Promise.resolve()

/** Sends `localhost` to the chosen computer (or removes it). Calls are chained; the promise resolves once the relay is in place. */
export function applyRoute(): Promise<void> {
  routing = routing.then(doApplyRoute).catch(() => undefined)
  return routing
}

async function doApplyRoute(): Promise<void> {
  if (!SignetLink) return
  const st = useStore.getState()
  const rt = useLinkRuntime.getState()
  const active = activeLinkIds(st.links, rt.byLink, st.settings.linkEnabled)
  const pick = pickLocalhostRoute(active, rt.chosen)
  const linkId = pick === null || pick === 'ask' ? null : pick
  const link = linkId ? linkOf(linkId) : undefined
  const token = link ? await getToken(link.linkId) : null
  const known = linkId ? rt.byLink[linkId] : undefined
  const route =
    link && token
      ? {
          endpoints: orderEndpoints(servicesFor(link.desktop.id), link).map((c) => endpointOf(link, c)),
          linkId: link.linkId,
          token,
          // Ports announced by the computer, ports of its pages, and those opened by hand here.
          ports: [...new Set([...(known?.ports ?? []), ...(known?.servers ?? []).map((s) => s.port), ...rt.extraPorts])].sort((a, b) => a - b)
        }
      : null
  const key = route ? JSON.stringify([route.linkId, route.endpoints, route.ports]) : 'null'
  if (key === routeKey) return
  routeKey = key
  await SignetLink.setLocalhostRoute(route).catch(() => false)
  useLinkRuntime.setState({ routeLinkId: route ? route.linkId : null })
}

/** Also relays this `localhost` port (opened by hand), then waits until the relay is in place. */
export async function relayLocalhostPort(url: string): Promise<void> {
  const port = loopbackPort(url)
  const rt = useLinkRuntime.getState()
  if (port !== null && !rt.extraPorts.includes(port)) useLinkRuntime.setState({ extraPorts: [...rt.extraPorts, port] })
  await applyRoute()
}

/** Chooses the computer that receives `localhost` and waits until the relay is in place. */
export async function chooseLocalhostComputer(linkId: string): Promise<void> {
  useLinkRuntime.setState({ chosen: linkId })
  await applyRoute()
}

let lastTunnelToast = ''
function onTunnelError(e: { port: number; code: string }): void {
  const key = `${e.port}:${e.code}`
  if (key === lastTunnelToast) return
  lastTunnelToast = key
  setTimeout(() => (lastTunnelToast = ''), 4000)
  const id = useLinkRuntime.getState().routeLinkId
  // The token or link is rejected by the computer: same handling as on the control connection.
  if (id && (e.code === 'bad-token' || e.code === 'unknown-link')) return markInvalid(id)
  const name = (id && linkOf(id)?.desktop.name) || t('link.computer')
  const known = ['port-closed', 'port-refused', 'paused'].includes(e.code) ? e.code : 'other'
  toast.show({ message: t(`link.tunnel.${known}`, { port: e.port, name }), duration: 5000 })
}

// --- Open on the other device ---

async function handleOpen(linkId: string, url: string): Promise<void> {
  const st = useStore.getState()
  const link = linkOf(linkId)
  const rt = useLinkRuntime.getState().byLink[linkId]
  if (!link || !rt?.connected || link.peerPaused || !link.enabled || !st.settings.linkEnabled) return
  let target = url
  // A `localhost` address sent by the computer refers to itself: route this phone to it.
  if (isLoopbackUrl(url)) {
    await chooseLocalhostComputer(linkId)
    target = toLinkLocalhost(url)
    await relayLocalhostPort(target)
  }
  useStore.getState().openTab({ url: target })
  toast.show({ message: t('link.openedFrom', { name: link.desktop.name }) })
}

/** Sends an address to the computer ("Open on computer"). False if the link is not active. */
export async function sendOpenToComputer(linkId: string, url: string): Promise<boolean> {
  const conn = states.get(linkId)?.conn
  if (!SignetLink || !conn?.connected || !/^https?:\/\//i.test(url)) return false
  try {
    await SignetLink.writeLine(conn.socketId, line({ type: 'open', url }))
    return true
  } catch {
    return false
  }
}

// --- Forget, pair ---

/** Deletes the link (token included). Notifies the computer if connected; otherwise it will learn of it at its next connection. */
export async function forgetLink(linkId: string): Promise<void> {
  const s = states.get(linkId)
  const conn = s?.conn
  if (SignetLink && conn?.connected) await SignetLink.writeLine(conn.socketId, line({ type: 'forget' })).catch(() => undefined)
  await removeLocally(linkId)
}

/** Deletes the link on this phone only (token, state, relay). */
async function removeLocally(linkId: string): Promise<void> {
  stopLink(linkId)
  states.delete(linkId)
  await deleteToken(linkId)
  useStore.getState().removeLink(linkId)
  clearRuntime(linkId)
  void applyRoute()
}

export type PairResult = { ok: true; device: LinkDevice } | { ok: false; error: string }

/** Pairs this phone with the QR code's computer: pinned socket on each address in turn, `pair`, `paired`. */
export async function pairWithOffer(offer: PairingOffer): Promise<PairResult> {
  if (!SignetLink) return { ok: false, error: 'unreachable' }
  const device = await phoneDevice()
  let failure = 'unreachable'
  for (const host of offer.hosts) {
    const id = `pair-${++socketCounter}`
    const reply = new Promise<LinkMessage | null>((resolve) => {
      handlers.set(id, { line: (raw) => resolve(parseLinkMessage(raw)), closed: () => resolve(null) })
    })
    try {
      await SignetLink.open(id, { host, port: offer.port, serverName: serverNameForLink(offer.linkId), certSha256: offer.certSha256 }, OPEN_TIMEOUT_MS)
    } catch (e) {
      handlers.delete(id)
      failure = errorCode(e).includes('pin-mismatch') ? 'pin-mismatch' : 'unreachable'
      continue
    }
    try {
      await SignetLink.writeLine(id, line({ type: 'pair', v: LINK_PROTOCOL_VERSION, linkId: offer.linkId, code: offer.code, device }))
      const answer = await Promise.race([reply, new Promise<null>((r) => setTimeout(() => r(null), PAIR_TIMEOUT_MS))])
      if (answer?.type === 'paired') {
        await savePairedLink(offer, answer.token, answer.device, { host, port: offer.port })
        return { ok: true, device: answer.device }
      }
      failure = answer?.type === 'error' ? answer.code : 'unreachable'
      if (answer?.type === 'error') return { ok: false, error: failure }
    } catch {
      failure = 'unreachable'
    } finally {
      handlers.delete(id)
      SignetLink.close(id)
    }
  }
  return { ok: false, error: failure }
}

async function savePairedLink(offer: PairingOffer, token: string, desktop: LinkDevice, worked: HostPort): Promise<void> {
  const st = useStore.getState()
  // Same computer already paired: the old link is replaced (new keys).
  for (const old of st.links.filter((l) => l.desktop.id === desktop.id || l.linkId === offer.linkId)) {
    stopLink(old.linkId)
    states.delete(old.linkId)
    clearRuntime(old.linkId)
    await deleteToken(old.linkId)
  }
  await setToken(offer.linkId, token)
  st.addLink({
    linkId: offer.linkId,
    desktop,
    certSha256: offer.certSha256,
    port: offer.port,
    hosts: offer.hosts,
    last: rememberEndpoint([], worked),
    enabled: true,
    peerPaused: false,
    invalid: false,
    pairedAt: Date.now()
  })
}

// --- Startup ---

/** Call once from the root: listens to the native module, the app state and the store. Returns the stop function. */
export function startLinkManager(): () => void {
  if (!SignetLink) return () => undefined
  useLinkRuntime.setState({ nativeAvailable: true, routeSupported: SignetLink.isLocalhostRoutingSupported() })
  const subs = [
    SignetLink.addListener('onSocketLine', (e) => handlers.get(e.id)?.line(e.line)),
    SignetLink.addListener('onSocketClosed', (e) => handlers.get(e.id)?.closed(e.error)),
    SignetLink.addListener('onServiceFound', onServiceFound),
    SignetLink.addListener('onServiceLost', (e) => {
      services.delete(e.name)
    }),
    SignetLink.addListener('onTunnelError', onTunnelError)
  ]
  const app = AppState.addEventListener('change', (state) => {
    foreground = state !== 'background'
    sync()
  })
  const unsubStore = useStore.subscribe((s, prev) => {
    if (s.links !== prev.links || s.settings.linkEnabled !== prev.settings.linkEnabled || s.linksLoaded !== prev.linksLoaded) sync()
  })
  const unsubRuntime = useLinkRuntime.subscribe((s, prev) => {
    if (s.byLink !== prev.byLink || s.chosen !== prev.chosen || s.extraPorts !== prev.extraPorts) void applyRoute()
  })
  sync()
  return () => {
    subs.forEach((x) => x.remove())
    app.remove()
    unsubStore()
    unsubRuntime()
    for (const id of [...states.keys()]) stopLink(id)
    if (browsing) SignetLink?.stopBrowsing()
    browsing = false
  }
}

/** DEV: stops all connections and no longer restarts them (for demo links). */
export function freezeLinkManager(): void {
  frozen = true
  for (const id of [...states.keys()]) stopLink(id)
  if (browsing) SignetLink?.stopBrowsing()
  browsing = false
}

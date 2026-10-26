// Link with phones: TLS server, pairing, control connections and tunnels to localhost.
// No dependency on Electron: the hosts (tabs, mDNS, UI) are plugged in by `link.ts`.
// The protocol is described in packages/shared/src/link/README.md.
import { randomBytes, timingSafeEqual } from 'node:crypto'
import { connect, type Socket } from 'node:net'
import { createSecureContext, createServer as createTlsServer, type SecureContext, type Server, type TLSSocket } from 'node:tls'
import {
  LINK_DEFAULT_PORT,
  LINK_MAX_LINE,
  LINK_PROTOCOL_VERSION,
  PAIRING_TTL_MS,
  encodeLinkMessage,
  encodePairingUrl,
  isLinkId,
  linkIdFromServerName,
  parseLinkMessage,
  type ClientHello,
  type DevServer,
  type LinkDevice,
  type LinkErrorCode,
  type LinkMessage,
  type PairingOffer
} from '@signet/shared/link/protocol'
import { linkStatus, type LinkStatus } from '@signet/shared/link/status'
import { certSha256, makeDefaultCert, makeLinkCert, type CertPair } from './certs'
import { LinkStore, newToken, hashToken, tokenMatches, type StoredLink } from './store'

/** A first line must arrive within this delay. */
const FIRST_LINE_MS = 10_000
/** Silent control connection (the phone sends a `ping` every 20 s). */
const CONTROL_IDLE_MS = 90_000
const CONNECT_MS = 3_000
/** At most one `servers` send per interval. */
const SERVERS_THROTTLE_MS = 2_000
/** Code attempts per QR code before it is cancelled. */
const MAX_BAD_CODES = 5

/** What the service asks of its host. */
export interface LinkHooks {
  /** The phone asks to open an address (the link is active on both sides). */
  open(url: string, link: StoredLink): void
  /** Something changed (list, state, pairing): the UI updates. */
  changed(): void
  /** Listening started (port) or stopped (null): mDNS announcement. */
  listening(port: number | null): void
}

export interface LinkServiceOptions {
  store: LinkStore
  /** This computer as phones see it. */
  desktop: () => LinkDevice
  hooks: LinkHooks
  /** Local network addresses, for the QR code. */
  hosts: () => string[]
  /** Preferred port (47615; 0 = random, for tests). */
  port?: number
  now?: () => number
}

interface Pending {
  linkId: string
  code: string
  expiresAt: number
  cert: CertPair
  certSha256: string
  badCodes: number
  timer: NodeJS.Timeout
}

/** Open control connection (after `welcome`). */
interface Session {
  socket: TLSSocket
  linkId: string
  peerPaused: boolean
  lastServers: string
  lastSentAt: number
  timer: NodeJS.Timeout | null
}

export interface LinkSummary {
  link: StoredLink
  status: LinkStatus
}

const sendLine = (socket: Socket, msg: LinkMessage): void => {
  if (!socket.destroyed && socket.writable) socket.write(encodeLinkMessage(msg))
}

/** Last message then close. */
const sendAndEnd = (socket: Socket, msg: LinkMessage): void => {
  if (socket.destroyed) return
  socket.end(encodeLinkMessage(msg))
  // The phone does not close: the socket is not kept more than a few seconds.
  socket.setTimeout(3000, () => socket.destroy())
}

const equalText = (a: string, b: string): boolean => {
  const x = Buffer.from(a)
  const y = Buffer.from(b)
  return x.length === y.length && timingSafeEqual(x, y)
}

/** Reads JSON lines from a byte stream; keeps what follows the lines read, as is (raw bytes of a tunnel). */
export class LineReader {
  private buf: Buffer = Buffer.alloc(0)

  /** Adds bytes. False if a line exceeds the allowed size. */
  add(chunk: Buffer): boolean {
    this.buf = this.buf.length ? Buffer.concat([this.buf, chunk]) : chunk
    const nl = this.buf.indexOf(10)
    return (nl < 0 ? this.buf.length : nl + 1) <= LINK_MAX_LINE
  }

  /** Next complete line (without line ending), or null. */
  line(): string | null {
    const nl = this.buf.indexOf(10)
    if (nl < 0) return null
    const text = this.buf.subarray(0, nl).toString('utf8').replace(/\r$/, '')
    this.buf = this.buf.subarray(nl + 1)
    return text
  }

  /** Complete lines received, or `null` if a line exceeds the allowed size. */
  push(chunk: Buffer): string[] | null {
    const lines: string[] = []
    if (!this.add(chunk)) return null
    for (let l = this.line(); l !== null; l = this.line()) {
      lines.push(l)
      if (this.buf.length > LINK_MAX_LINE && this.buf.indexOf(10) < 0) return null
    }
    return lines
  }

  /** Bytes received after the last line read. */
  rest(): Buffer {
    return this.buf
  }
}

export class LinkService {
  private server: Server | null = null
  private listenPromise: Promise<number | null> | null = null
  private port_ = 0
  private pending: Pending | null = null
  /** Last finished pairing codes: "already used" or "expired" rather than "unknown". */
  private finished = new Map<string, 'used' | 'expired'>()
  private sessions = new Map<string, Session>()
  private tunnels = new Map<string, Set<{ client: Socket; target: Socket }>>()
  private contexts = new Map<string, SecureContext>()
  private defaultContext: SecureContext | null = null
  private servers: DevServer[] = []
  private ports: number[] = []
  private closed = false
  private now: () => number

  constructor(private o: LinkServiceOptions) {
    this.now = o.now ?? Date.now
  }

  // ───────────────────────── State ─────────────────────────

  get port(): number | null {
    return this.server ? this.port_ : null
  }

  get general(): boolean {
    return this.o.store.general
  }

  /** The computer must listen: master switch ON and (a link exists or a pairing is in progress). */
  private shouldListen(): boolean {
    return !this.closed && this.o.store.general && (this.o.store.links.length > 0 || this.pending !== null)
  }

  statusOf(link: StoredLink): LinkStatus {
    return linkStatus({
      general: this.o.store.general,
      device: link.enabled,
      connected: this.sessions.has(link.linkId),
      connecting: false,
      peerPaused: link.peerPaused,
      invalid: false
    })
  }

  summaries(): LinkSummary[] {
    return this.o.store.links.map((link) => ({ link, status: this.statusOf(link) }))
  }

  /** Links active on both sides. */
  activeLinks(): StoredLink[] {
    return this.o.store.links.filter((l) => this.statusOf(l) === 'connected')
  }

  /** There is at least one control connection (even paused): the dev server scan knows whether it must run. */
  hasSessions(): boolean {
    return this.sessions.size > 0
  }

  get pendingOffer(): { offer: PairingOffer; url: string } | null {
    const p = this.pending
    if (!p) return null
    return this.offerFor(p)
  }

  private offerFor(p: Pending): { offer: PairingOffer; url: string } {
    const offer: PairingOffer = {
      v: LINK_PROTOCOL_VERSION,
      desktop: this.o.desktop(),
      linkId: p.linkId,
      certSha256: p.certSha256,
      code: p.code,
      hosts: this.o.hosts(),
      port: this.port_,
      expiresAt: p.expiresAt
    }
    return { offer, url: encodePairingUrl(offer) }
  }

  // ───────────────────────── Listening ─────────────────────────

  /** Brings listening (and connections) in line with the switches. To call after any change. */
  async sync(): Promise<void> {
    if (this.shouldListen()) await this.listen()
    else await this.stopListening()
    this.o.hooks.changed()
  }

  private async listen(): Promise<number | null> {
    if (this.server) return this.port_
    if (this.listenPromise) return this.listenPromise
    this.listenPromise = (async () => {
      try {
        if (!this.o.store.defaultCert) {
          this.o.store.defaultCert = await makeDefaultCert()
          this.o.store.save()
        }
        const dc = this.o.store.defaultCert
        this.defaultContext = createSecureContext({ key: dc.keyPem, cert: dc.certPem, minVersion: 'TLSv1.2' })
        const server = createTlsServer({
          key: dc.keyPem,
          cert: dc.certPem,
          minVersion: 'TLSv1.2',
          SNICallback: (name, cb) => cb(null, this.contextFor(name))
        })
        server.on('secureConnection', (socket) => this.onConnection(socket))
        // A failed TLS negotiation (bad client) is not a service error.
        server.on('tlsClientError', (_e, socket) => socket.destroy())
        server.on('error', () => {})
        const preferred = this.o.port ?? LINK_DEFAULT_PORT
        const port = await this.bind(server, preferred).catch(() => (preferred === 0 ? Promise.reject() : this.bind(server, 0)))
        if (this.closed || !this.shouldListen()) {
          server.close()
          return null
        }
        this.server = server
        this.port_ = port
        this.o.hooks.listening(port)
        return port
      } catch {
        return null
      } finally {
        this.listenPromise = null
      }
    })()
    return this.listenPromise
  }

  private bind(server: Server, port: number): Promise<number> {
    return new Promise((resolve, reject) => {
      const fail = (err: Error) => reject(err)
      server.once('error', fail)
      server.listen(port, () => {
        server.off('error', fail)
        const addr = server.address()
        resolve(typeof addr === 'object' && addr ? addr.port : port)
      })
    })
  }

  private async stopListening(): Promise<void> {
    if (this.listenPromise) await this.listenPromise
    const server = this.server
    if (!server) return
    this.server = null
    this.port_ = 0
    // Open connections are warned before closing.
    for (const s of this.sessions.values()) this.endSession(s, true)
    for (const set of this.tunnels.values()) for (const t of set) t.client.destroy()
    this.tunnels.clear()
    server.close()
    this.o.hooks.listening(null)
  }

  private contextFor(name: string | undefined): SecureContext | undefined {
    const linkId = linkIdFromServerName(name)
    if (linkId) {
      const cached = this.contexts.get(linkId)
      if (cached) return cached
      const cert: CertPair | undefined = this.pending?.linkId === linkId ? this.pending.cert : this.o.store.get(linkId)
      if (cert) {
        const ctx = createSecureContext({ key: cert.keyPem, cert: cert.certPem, minVersion: 'TLSv1.2' })
        this.contexts.set(linkId, ctx)
        return ctx
      }
    }
    return this.defaultContext ?? undefined
  }

  /** Final stop (app shutdown). */
  async close(): Promise<void> {
    this.closed = true
    if (this.pending) clearTimeout(this.pending.timer)
    this.pending = null
    await this.stopListening()
  }

  // ───────────────────────── Settings ─────────────────────────

  async setGeneral(on: boolean): Promise<void> {
    if (this.o.store.general === on) return
    this.o.store.general = on
    this.o.store.save()
    if (!on) this.cancelPairing(false)
    await this.sync()
  }

  async setDevice(linkId: string, on: boolean): Promise<void> {
    const link = this.o.store.get(linkId)
    if (!link || link.enabled === on) return
    link.enabled = on
    this.o.store.save()
    const session = this.sessions.get(linkId)
    if (session) {
      sendLine(session.socket, { type: 'status', paused: !on })
      if (on) this.sendServers(session, true)
    }
    if (!on) this.closeTunnels(linkId)
    this.o.hooks.changed()
  }

  /** Deletes the link; warns the phone if it is connected. */
  async forget(linkId: string): Promise<void> {
    const session = this.sessions.get(linkId)
    if (session) {
      sendLine(session.socket, { type: 'forget' })
      this.endSession(session, false)
    }
    this.drop(linkId)
    await this.sync()
  }

  private drop(linkId: string): void {
    this.o.store.remove(linkId)
    this.contexts.delete(linkId)
    this.closeTunnels(linkId)
  }

  private closeTunnels(linkId: string): void {
    const set = this.tunnels.get(linkId)
    if (!set) return
    for (const t of set) t.client.destroy()
    this.tunnels.delete(linkId)
  }

  // ───────────────────────── Pairing ─────────────────────────

  /** Creates a QR code (the previous one is cancelled). Null if the computer cannot listen. */
  async startPairing(): Promise<{ offer: PairingOffer; url: string } | null> {
    if (!this.o.store.general || this.closed) return null
    this.cancelPairing(false)
    const linkId = randomBytes(8).toString('hex')
    const cert = await makeLinkCert(linkId)
    const expiresAt = this.now() + PAIRING_TTL_MS
    const p: Pending = {
      linkId,
      code: randomBytes(16).toString('base64url'),
      expiresAt,
      cert,
      certSha256: certSha256(cert.certPem),
      badCodes: 0,
      timer: setTimeout(() => this.expirePairing(linkId), PAIRING_TTL_MS)
    }
    p.timer.unref()
    this.pending = p
    this.contexts.delete(linkId)
    if ((await this.listen()) === null) {
      this.cancelPairing(true)
      return null
    }
    this.o.hooks.changed()
    return this.offerFor(p)
  }

  cancelPairing(notify = true): void {
    const p = this.pending
    if (!p) return
    clearTimeout(p.timer)
    this.pending = null
    this.contexts.delete(p.linkId)
    if (notify) {
      this.o.hooks.changed()
      void this.sync()
    }
  }

  private finish(linkId: string, how: 'used' | 'expired'): void {
    this.finished.set(linkId, how)
    if (this.finished.size > 8) this.finished.delete(this.finished.keys().next().value!)
  }

  private expirePairing(linkId: string): void {
    if (this.pending?.linkId !== linkId) return
    this.finish(linkId, 'expired')
    this.cancelPairing()
  }

  // ───────────────────────── Connections ─────────────────────────

  private onConnection(socket: TLSSocket): void {
    socket.setNoDelay(true)
    socket.setKeepAlive(true, 30_000)
    const reader = new LineReader()
    const first = setTimeout(() => socket.destroy(), FIRST_LINE_MS)
    socket.on('error', () => socket.destroy())
    const onData = (chunk: Buffer) => {
      if (!reader.add(chunk)) return sendAndEnd(socket, { type: 'error', code: 'bad-request' })
      const line = reader.line()
      if (line === null) return
      // First line received: we stop listening here, the chosen role takes over with the bytes already received.
      clearTimeout(first)
      socket.off('data', onData)
      socket.pause()
      const msg = parseLinkMessage(line)
      if (!msg || (msg.type !== 'pair' && msg.type !== 'hello')) return sendAndEnd(socket, { type: 'error', code: 'bad-request' })
      void this.dispatch(socket, msg, reader.rest())
    }
    socket.on('data', onData)
    socket.once('close', () => clearTimeout(first))
  }

  private async dispatch(socket: TLSSocket, msg: ClientHello, rest: Buffer): Promise<void> {
    if (msg.v !== LINK_PROTOCOL_VERSION) return sendAndEnd(socket, { type: 'error', code: 'version' })
    if (msg.type === 'pair') return this.onPair(socket, msg)
    const link = this.authenticate(socket, msg)
    if (!link) return
    if (msg.role === 'control') this.onControl(socket, link, msg.device, rest)
    else await this.onTunnel(socket, link, msg.port, rest)
  }

  /** Checks the link and token; replies with an error otherwise. */
  private authenticate(socket: Socket, msg: Extract<ClientHello, { type: 'hello' }>): StoredLink | null {
    const link = this.o.store.get(msg.linkId)
    if (!link) {
      sendAndEnd(socket, { type: 'error', code: 'unknown-link' })
      return null
    }
    if (!tokenMatches(msg.token, link.tokenHash)) {
      sendAndEnd(socket, { type: 'error', code: 'bad-token' })
      return null
    }
    return link
  }

  private async onPair(socket: TLSSocket, msg: Extract<ClientHello, { type: 'pair' }>): Promise<void> {
    const fail = (code: LinkErrorCode) => sendAndEnd(socket, { type: 'error', code })
    const p = this.pending
    // The name (SNI) must be the QR code's one: otherwise the phone did not pin the right certificate.
    if (!p || p.linkId !== msg.linkId || !isLinkId(msg.linkId) || linkIdFromServerName(socket.servername || undefined) !== msg.linkId) {
      const done = this.finished.get(msg.linkId)
      return fail(done === 'used' ? 'bad-code' : done === 'expired' ? 'expired' : 'unknown-link')
    }
    if (this.now() > p.expiresAt) {
      this.expirePairing(p.linkId)
      return fail('expired')
    }
    if (!equalText(msg.code, p.code)) {
      if (++p.badCodes >= MAX_BAD_CODES) this.cancelPairing()
      return fail('bad-code')
    }
    // Good code: single use.
    clearTimeout(p.timer)
    this.pending = null
    this.finish(p.linkId, 'used')
    const token = newToken()
    // A given phone keeps only one link (re-pairing = new link).
    for (const old of this.o.store.links.filter((l) => l.peer.id === msg.device.id)) {
      const s = this.sessions.get(old.linkId)
      if (s) this.endSession(s, false)
      this.drop(old.linkId)
    }
    this.o.store.add({
      linkId: p.linkId,
      peer: msg.device,
      enabled: true,
      certPem: p.cert.certPem,
      keyPem: p.cert.keyPem,
      tokenHash: hashToken(token),
      createdAt: this.now(),
      lastSeen: null,
      peerPaused: false
    })
    sendAndEnd(socket, { type: 'paired', token, device: this.o.desktop() })
    this.o.hooks.changed()
  }

  // ───────────────────────── Control ─────────────────────────

  private onControl(socket: TLSSocket, link: StoredLink, device: LinkDevice, rest: Buffer): void {
    // Only one control connection per link: the new one replaces the old one.
    const old = this.sessions.get(link.linkId)
    if (old) this.endSession(old, false)
    link.peer = { ...link.peer, name: device.name, platform: device.platform }
    link.lastSeen = this.now()
    // The `hello` counts as a resume on the phone side.
    link.peerPaused = false
    this.o.store.save()
    const session: Session = { socket, linkId: link.linkId, peerPaused: false, lastServers: '', lastSentAt: 0, timer: null }
    this.sessions.set(link.linkId, session)
    sendLine(socket, { type: 'welcome', device: this.o.desktop(), paused: !(this.o.store.general && link.enabled) })
    this.sendServers(session, true)

    const reader = new LineReader()
    const handle = (chunk: Buffer) => {
      const lines = reader.push(chunk)
      if (lines === null) return this.endSession(session, false)
      for (const line of lines) {
        const m = parseLinkMessage(line)
        if (m) this.onControlMessage(session, link, m)
      }
    }
    socket.setTimeout(CONTROL_IDLE_MS, () => socket.destroy())
    socket.on('data', handle)
    socket.on('close', () => {
      if (this.sessions.get(link.linkId) === session) {
        this.sessions.delete(link.linkId)
        if (session.timer) clearTimeout(session.timer)
        this.o.hooks.changed()
      }
    })
    socket.resume()
    this.o.hooks.changed()
    if (rest.length) handle(rest)
  }

  private onControlMessage(session: Session, link: StoredLink, m: LinkMessage): void {
    switch (m.type) {
      case 'ping':
        return sendLine(session.socket, { type: 'pong' })
      case 'status': {
        const resumed = session.peerPaused && !m.paused
        session.peerPaused = m.paused
        link.peerPaused = m.paused
        this.o.store.save()
        if (resumed) this.sendServers(session, true)
        return this.o.hooks.changed()
      }
      case 'open':
        // Accepted only if the link is active on both sides.
        if (this.statusOf(link) === 'connected' && !session.peerPaused) this.o.hooks.open(m.url, link)
        return
      case 'forget':
        this.endSession(session, false)
        this.drop(link.linkId)
        return void this.sync()
      default:
        return
    }
  }

  /** Closes a control connection; `notify`: warns of a pause beforehand (listening stopped). */
  private endSession(session: Session, notify: boolean): void {
    if (this.sessions.get(session.linkId) === session) this.sessions.delete(session.linkId)
    if (session.timer) clearTimeout(session.timer)
    if (notify) sendAndEnd(session.socket, { type: 'status', paused: true })
    else {
      session.socket.end()
      session.socket.setTimeout(3000, () => session.socket.destroy())
    }
  }

  // ───────────────────────── Dev servers and "open on" ─────────────────────────

  /** Current list of dev servers; sent to active phones if it changed. */
  setServers(servers: DevServer[], ports: number[] = servers.map((s) => s.port)): void {
    this.servers = servers
    this.ports = ports
    for (const s of this.sessions.values()) this.sendServers(s, false)
  }

  private sendServers(session: Session, now: boolean): void {
    const link = this.o.store.get(session.linkId)
    if (!link || this.statusOf(link) !== 'connected' || session.peerPaused) return
    const json = JSON.stringify([this.servers, this.ports])
    if (!now && json === session.lastServers) return
    const wait = session.lastSentAt + SERVERS_THROTTLE_MS - this.now()
    if (!now && wait > 0) {
      // Too early: a single deferred send, with the list from that moment.
      session.timer ??= setTimeout(() => {
        session.timer = null
        this.sendServers(session, false)
      }, wait)
      return
    }
    if (session.timer) clearTimeout(session.timer)
    session.timer = null
    session.lastServers = json
    session.lastSentAt = this.now()
    sendLine(session.socket, { type: 'servers', servers: this.servers, ports: this.ports })
  }

  /** "Open on my phone". False if this phone is not active. */
  openOnPhone(linkId: string, url: string): boolean {
    const session = this.sessions.get(linkId)
    const link = this.o.store.get(linkId)
    if (!session || !link || this.statusOf(link) !== 'connected' || session.peerPaused) return false
    if (!/^https?:\/\//i.test(url) || url.length > 4096) return false
    sendLine(session.socket, { type: 'open', url })
    return true
  }

  // ───────────────────────── Tunnel ─────────────────────────

  private async onTunnel(socket: TLSSocket, link: StoredLink, port: number, rest: Buffer): Promise<void> {
    if (!this.o.store.general || !link.enabled) return sendAndEnd(socket, { type: 'error', code: 'paused' })
    if (port === this.port_) return sendAndEnd(socket, { type: 'error', code: 'port-refused' })
    let target: Socket | null = null
    for (const host of ['127.0.0.1', '::1']) {
      target = await tryConnect(host, port)
      if (target || socket.destroyed) break
    }
    if (socket.destroyed) return void target?.destroy()
    if (!target) return sendAndEnd(socket, { type: 'error', code: 'port-closed' })
    // The link may have been cut during the connection.
    if (!this.o.store.get(link.linkId) || !this.o.store.general || !link.enabled) {
      target.destroy()
      return sendAndEnd(socket, { type: 'error', code: 'paused' })
    }
    const entry = { client: socket as Socket, target }
    let set = this.tunnels.get(link.linkId)
    if (!set) this.tunnels.set(link.linkId, (set = new Set()))
    set.add(entry)
    const cleanup = () => {
      socket.destroy()
      target.destroy()
      set.delete(entry)
    }
    socket.on('error', cleanup)
    target.on('error', cleanup)
    socket.once('close', cleanup)
    target.once('close', cleanup)
    target.setNoDelay(true)
    socket.write(encodeLinkMessage({ type: 'ok' }))
    if (rest.length) target.write(rest)
    socket.pipe(target)
    target.pipe(socket)
    socket.resume()
  }
}

/** Connection to the machine itself; null if it fails. */
function tryConnect(host: string, port: number): Promise<Socket | null> {
  return new Promise((resolve) => {
    const s = connect({ host, port })
    const fail = () => {
      s.destroy()
      resolve(null)
    }
    s.setTimeout(CONNECT_MS, fail)
    s.once('error', fail)
    s.once('connect', () => {
      s.setTimeout(0)
      s.off('error', fail)
      resolve(s)
    })
  })
}

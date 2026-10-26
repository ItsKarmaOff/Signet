import { mkdtempSync, rmSync } from 'node:fs'
import { createServer, type Server } from 'node:http'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { createHash } from 'node:crypto'
import { connect, type TLSSocket } from 'node:tls'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import {
  LINK_MAX_LINE,
  decodePairingUrl,
  encodeLinkMessage,
  parseLinkMessage,
  serverNameForLink,
  type DevServer,
  type LinkDevice,
  type LinkMessage
} from '@signet/shared/link/protocol'
import { LineReader, LinkService } from './service'
import { LinkStore, hashToken, type Vault } from './store'

const vault: Vault = { available: () => true, encrypt: (s) => Buffer.from(s).toString('base64'), decrypt: (s) => Buffer.from(s, 'base64').toString() }
const DESKTOP: LinkDevice = { id: 'desk-1', name: 'Mon Mac', platform: 'macos' }
const PHONE: LinkDevice = { id: 'phone-1', name: 'iPhone de test', platform: 'ios' }
const sha256 = (der: Buffer) => createHash('sha256').update(der).digest('base64url')

/** A phone's connection: TLS with the link's SNI, certificate fingerprint captured (pinning verified by the test). */
class Phone {
  private reader = new LineReader()
  private queue: (LinkMessage | null)[] = []
  private waiting: ((m: LinkMessage | null) => void) | null = null
  closed = false
  raw: Buffer[] = []
  fingerprint = ''

  private constructor(readonly socket: TLSSocket) {
    socket.on('data', (c: Buffer) => {
      this.raw.push(c)
      for (const line of this.reader.push(c) ?? []) this.push(parseLinkMessage(line))
    })
    socket.on('close', () => {
      this.closed = true
      this.push(null)
    })
    socket.on('error', () => {})
  }

  private push(m: LinkMessage | null): void {
    if (this.waiting) {
      const w = this.waiting
      this.waiting = null
      w(m)
    } else this.queue.push(m)
  }

  static open(port: number, linkId: string): Promise<Phone> {
    return new Promise((resolve, reject) => {
      const socket = connect({
        host: '127.0.0.1',
        port,
        servername: serverNameForLink(linkId),
        rejectUnauthorized: false,
        checkServerIdentity: () => undefined
      })
      const phone = new Phone(socket)
      socket.once('secureConnect', () => {
        phone.fingerprint = sha256(socket.getPeerCertificate(true).raw)
        resolve(phone)
      })
      socket.once('error', reject)
    })
  }

  send(msg: LinkMessage): void {
    this.socket.write(encodeLinkMessage(msg))
  }

  /** Next message; null if the connection closes before. */
  next(timeout = 3000): Promise<LinkMessage | null> {
    if (this.queue.length) return Promise.resolve(this.queue.shift()!)
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error('aucun message')), timeout)
      this.waiting = (m) => (clearTimeout(timer), resolve(m))
    })
  }

  /** True if nothing arrives during `ms`. */
  async silent(ms = 150): Promise<boolean> {
    return (
      this.queue.length === 0 &&
      (await Promise.race([this.next(ms).then(() => false), new Promise<boolean>((r) => setTimeout(() => r(true), ms + 50))]).catch(() => true))
    )
  }

  end(): void {
    this.socket.destroy()
  }
}

let dir: string
let store: LinkStore
let service: LinkService
let opened: string[]
let listening: (number | null)[]
let phones: Phone[]
let servers: Server[]
let clock: number

beforeEach(() => {
  dir = mkdtempSync(join(tmpdir(), 'signet-link-'))
  clock = 1_000_000
  opened = []
  listening = []
  phones = []
  servers = []
  store = new LinkStore(join(dir, 'link.json'), vault)
  service = new LinkService({
    store,
    desktop: () => DESKTOP,
    hosts: () => ['192.168.1.20'],
    port: 0,
    now: () => clock,
    hooks: { open: (url) => opened.push(url), changed: () => {}, listening: (p) => listening.push(p) }
  })
})

afterEach(async () => {
  for (const p of phones) p.end()
  for (const s of servers) s.close()
  await service.close()
  // A store write can still land in the folder while it is removed (ENOTEMPTY on macOS): retry.
  rmSync(dir, { recursive: true, force: true, maxRetries: 5, retryDelay: 50 })
})

const open = async (port: number, linkId: string) => {
  const p = await Phone.open(port, linkId)
  phones.push(p)
  return p
}

/** Pairs a phone and returns what it keeps. */
async function pairPhone(device: LinkDevice = PHONE) {
  const res = await service.startPairing()
  if (!res) throw new Error('pas d’appairage')
  const { offer } = res
  const phone = await open(offer.port, offer.linkId)
  phone.send({ type: 'pair', v: 1, linkId: offer.linkId, code: offer.code, device })
  const reply = await phone.next()
  if (reply?.type !== 'paired') throw new Error('pairing refused: ' + JSON.stringify(reply))
  return { offer, token: reply.token, linkId: offer.linkId, reply }
}

async function control(port: number, linkId: string, token: string, device: LinkDevice = PHONE) {
  const phone = await open(port, linkId)
  phone.send({ type: 'hello', v: 1, linkId, token, role: 'control', device })
  return phone
}

const web = (host: string, body: string): Promise<{ server: Server; port: number }> =>
  new Promise((resolve, reject) => {
    const server = createServer((_req, res) => res.end(body))
    server.once('error', reject)
    server.listen(0, host, () => {
      servers.push(server)
      resolve({ server, port: (server.address() as { port: number }).port })
    })
  })

describe('listening', () => {
  it('nothing listens as long as no phone is paired', async () => {
    await service.sync()
    expect(service.port).toBeNull()
    expect(listening).toEqual([])
  })

  it('pairing opens listening; cancelling closes it if there is no link', async () => {
    const res = await service.startPairing()
    expect(res).not.toBeNull()
    expect(service.port).toBe(res!.offer.port)
    expect(listening).toEqual([res!.offer.port])
    service.cancelPairing()
    await vi.waitFor(() => expect(service.port).toBeNull())
    expect(listening.at(-1)).toBeNull()
  })

  it('no pairing when the master switch is off', async () => {
    await service.setGeneral(false)
    expect(await service.startPairing()).toBeNull()
  })
})

describe('pairing', () => {
  it('the QR code is readable, the certificate pinned and the link saved (token as SHA-256 only)', async () => {
    const res = (await service.startPairing())!
    const decoded = decodePairingUrl(res.url, clock)
    // The QR code only carries the computer name (the rest arrives with `paired`) and an end rounded to the second.
    expect(decoded.ok && decoded.offer).toMatchObject({
      linkId: res.offer.linkId,
      hosts: ['192.168.1.20'],
      desktop: { name: DESKTOP.name },
      expiresAt: Math.ceil((clock + 5 * 60_000) / 1000) * 1000
    })
    expect(res.offer.code).toHaveLength(22)
    expect(res.offer.linkId).toMatch(/^[0-9a-f]{16}$/)

    const phone = await open(res.offer.port, res.offer.linkId)
    expect(phone.fingerprint).toBe(res.offer.certSha256)
    phone.send({ type: 'pair', v: 1, linkId: res.offer.linkId, code: res.offer.code, device: PHONE })
    const reply = await phone.next()
    expect(reply).toMatchObject({ type: 'paired', device: DESKTOP })
    const token = (reply as { token: string }).token
    expect(token).toHaveLength(43)
    expect(await phone.next()).toBeNull() // the connection closes

    const link = store.get(res.offer.linkId)!
    expect(link).toMatchObject({ peer: PHONE, enabled: true, peerPaused: false })
    expect(link.tokenHash).toBe(hashToken(token))
    expect(JSON.stringify(link)).not.toContain(token)
    expect(service.pendingOffer).toBeNull()
  })

  it('a single-use code: the second attempt is refused (bad-code)', async () => {
    const { offer, linkId } = await pairPhone()
    const again = await open(offer.port, linkId)
    again.send({ type: 'pair', v: 1, linkId, code: offer.code, device: PHONE })
    expect(await again.next()).toEqual({ type: 'error', code: 'bad-code' })
  })

  it('a wrong code is refused without consuming the QR code; after 5 attempts it is cancelled', async () => {
    const { offer } = (await service.startPairing())!
    const bad = 'A'.repeat(22)
    for (let i = 0; i < 4; i++) {
      const p = await open(offer.port, offer.linkId)
      p.send({ type: 'pair', v: 1, linkId: offer.linkId, code: bad, device: PHONE })
      expect(await p.next()).toEqual({ type: 'error', code: 'bad-code' })
    }
    expect(service.pendingOffer).not.toBeNull()
    const last = await open(offer.port, offer.linkId)
    last.send({ type: 'pair', v: 1, linkId: offer.linkId, code: bad, device: PHONE })
    expect(await last.next()).toEqual({ type: 'error', code: 'bad-code' })
    expect(service.pendingOffer).toBeNull()
  })

  it('an expired QR code is refused (expired)', async () => {
    const { offer } = (await service.startPairing())!
    clock += 5 * 60_000 + 1
    const p = await open(offer.port, offer.linkId)
    p.send({ type: 'pair', v: 1, linkId: offer.linkId, code: offer.code, device: PHONE })
    expect(await p.next()).toEqual({ type: 'error', code: 'expired' })
    expect(store.links).toHaveLength(0)
  })

  it('a new QR code replaces the previous one', async () => {
    const first = (await service.startPairing())!
    const second = (await service.startPairing())!
    expect(second.offer.linkId).not.toBe(first.offer.linkId)
    const p = await open(first.offer.port, first.offer.linkId)
    p.send({ type: 'pair', v: 1, linkId: first.offer.linkId, code: first.offer.code, device: PHONE })
    expect(await p.next()).toEqual({ type: 'error', code: 'unknown-link' })
  })

  it('unknown link: default certificate (different fingerprint) and unknown-link', async () => {
    const { offer, linkId } = await pairPhone()
    const stranger = await open(offer.port, 'f'.repeat(16))
    expect(stranger.fingerprint).not.toBe(offer.certSha256)
    stranger.send({ type: 'hello', v: 1, linkId: 'f'.repeat(16), token: 'A'.repeat(43), role: 'control', device: PHONE })
    expect(await stranger.next()).toEqual({ type: 'error', code: 'unknown-link' })
    // The known link keeps its certificate.
    expect((await open(offer.port, linkId)).fingerprint).toBe(offer.certSha256)
  })

  it('the protocol version is checked', async () => {
    const { offer } = (await service.startPairing())!
    const p = await open(offer.port, offer.linkId)
    p.send({ type: 'pair', v: 99, linkId: offer.linkId, code: offer.code, device: PHONE })
    expect(await p.next()).toEqual({ type: 'error', code: 'version' })
  })

  it('re-pairing the same phone replaces its link', async () => {
    const a = await pairPhone()
    const b = await pairPhone()
    expect(store.links.map((l) => l.linkId)).toEqual([b.linkId])
    expect(store.get(a.linkId)).toBeUndefined()
  })
})

describe('control connection', () => {
  it('welcome, servers, then ping/pong', async () => {
    const { offer, linkId, token } = await pairPhone()
    const c = await control(offer.port, linkId, token)
    expect(await c.next()).toEqual({ type: 'welcome', device: DESKTOP, paused: false })
    expect(await c.next()).toEqual({ type: 'servers', servers: [], ports: [] })
    c.send({ type: 'ping' })
    expect(await c.next()).toEqual({ type: 'pong' })
    expect(service.statusOf(store.get(linkId)!)).toBe('connected')
    c.end()
    await vi.waitFor(() => expect(service.statusOf(store.get(linkId)!)).toBe('offline'))
    expect(store.get(linkId)!.lastSeen).not.toBeNull()
  })

  it('wrong token: bad-token', async () => {
    const { offer, linkId } = await pairPhone()
    const c = await control(offer.port, linkId, 'B'.repeat(43))
    expect(await c.next()).toEqual({ type: 'error', code: 'bad-token' })
    expect(await c.next()).toBeNull()
  })

  it('a line that is too long or malformed closes the connection', async () => {
    const { offer, linkId } = await pairPhone()
    const long = await open(offer.port, linkId)
    long.socket.write('x'.repeat(LINK_MAX_LINE + 10))
    expect(await long.next()).toEqual({ type: 'error', code: 'bad-request' })
    const junk = await open(offer.port, linkId)
    junk.socket.write('pas du json\n')
    expect(await junk.next()).toEqual({ type: 'error', code: 'bad-request' })
  })

  it('sends dev servers when the list changes (at most once every 2 s)', async () => {
    const { offer, linkId, token } = await pairPhone()
    const c = await control(offer.port, linkId, token)
    await c.next()
    await c.next() // servers: []
    const a: DevServer = { url: 'http://localhost:5173/', port: 5173, title: 'Vite' }
    const b: DevServer = { url: 'http://localhost:3000/', port: 3000 }
    service.setServers([a])
    service.setServers([a, b]) // grouped with the previous one
    clock += 2000 // the test clock is frozen: the 2 s delay has elapsed when the deferred send goes out
    expect(await c.next(4000)).toEqual({ type: 'servers', servers: [a, b], ports: [3000, 5173] })
    service.setServers([a, b]) // unchanged: nothing
    expect(await c.silent(300)).toBe(true)
  })

  it('the phone\'s "open" opens the address here; http(s) only', async () => {
    const { offer, linkId, token } = await pairPhone()
    const c = await control(offer.port, linkId, token)
    await c.next()
    await c.next()
    c.send({ type: 'open', url: 'https://exemple.com/' })
    c.socket.write('{"type":"open","url":"javascript:alert(1)"}\n')
    c.send({ type: 'ping' })
    await c.next() // pong: both `open`s are handled
    expect(opened).toEqual(['https://exemple.com/'])
  })

  it('"open on my phone" reaches the active phone', async () => {
    const { offer, linkId, token } = await pairPhone()
    expect(service.openOnPhone(linkId, 'http://localhost:5173/')).toBe(false) // not connected
    const c = await control(offer.port, linkId, token)
    await c.next()
    await c.next()
    expect(service.openOnPhone(linkId, 'http://localhost:5173/')).toBe(true)
    expect(await c.next()).toEqual({ type: 'open', url: 'http://localhost:5173/' })
    expect(service.openOnPhone(linkId, 'file:///etc/passwd')).toBe(false)
  })

  it('pause on the phone side: paused-there, `open` ignored, resume on return', async () => {
    const { offer, linkId, token } = await pairPhone()
    const c = await control(offer.port, linkId, token)
    await c.next()
    await c.next()
    c.send({ type: 'status', paused: true })
    await vi.waitFor(() => expect(service.statusOf(store.get(linkId)!)).toBe('paused-there'))
    c.send({ type: 'open', url: 'https://exemple.com/' })
    c.send({ type: 'ping' })
    await c.next()
    expect(opened).toEqual([])
    expect(service.openOnPhone(linkId, 'https://exemple.com/')).toBe(false)
    c.end()
    // Closed without resume: the "paused" state stays known.
    await vi.waitFor(() => expect(c.closed).toBe(true))
    expect(store.get(linkId)!.peerPaused).toBe(true)
    // The next `hello` counts as a resume.
    const again = await control(offer.port, linkId, token)
    expect(await again.next()).toMatchObject({ type: 'welcome' })
    expect(store.get(linkId)!.peerPaused).toBe(false)
  })

  it('device switch off here: status paused, connection kept, nothing pushed or opened, ping answered', async () => {
    const { offer, linkId, token } = await pairPhone()
    const c = await control(offer.port, linkId, token)
    await c.next()
    await c.next()
    await service.setDevice(linkId, false)
    expect(await c.next()).toEqual({ type: 'status', paused: true })
    expect(service.statusOf(store.get(linkId)!)).toBe('paused-here')
    service.setServers([{ url: 'http://localhost:1234/', port: 1234 }])
    c.send({ type: 'open', url: 'https://exemple.com/' })
    c.send({ type: 'ping' })
    expect(await c.next()).toEqual({ type: 'pong' })
    expect(opened).toEqual([])
    expect(c.closed).toBe(false)
    // Resume: status paused:false then servers.
    await service.setDevice(linkId, true)
    expect(await c.next()).toEqual({ type: 'status', paused: false })
    expect(await c.next()).toEqual({ type: 'servers', servers: [{ url: 'http://localhost:1234/', port: 1234 }], ports: [1234] })
    // A new `hello` during the pause receives paused:true.
    await service.setDevice(linkId, false)
    const second = await control(offer.port, linkId, token)
    expect(await second.next()).toEqual({ type: 'welcome', device: DESKTOP, paused: true })
  })

  it('master switch off: status paused then close, port closed, announcement withdrawn; reopens on return', async () => {
    const { offer, linkId, token } = await pairPhone()
    const c = await control(offer.port, linkId, token)
    await c.next()
    await c.next()
    await service.setGeneral(false)
    expect(await c.next()).toEqual({ type: 'status', paused: true })
    expect(await c.next()).toBeNull()
    expect(service.port).toBeNull()
    expect(listening.at(-1)).toBeNull()
    await expect(Phone.open(offer.port, linkId)).rejects.toThrow()
    // The link is kept, and the device switch keeps its position.
    expect(store.get(linkId)!.enabled).toBe(true)
    expect(service.statusOf(store.get(linkId)!)).toBe('paused-here')
    await service.setGeneral(true)
    expect(service.port).not.toBeNull()
    expect(listening.at(-1)).toBe(service.port)
    const back = await control(service.port!, linkId, token)
    expect(await back.next()).toMatchObject({ type: 'welcome', paused: false })
  })

  it('forgetting here warns the phone and deletes the link', async () => {
    const { offer, linkId, token } = await pairPhone()
    const c = await control(offer.port, linkId, token)
    await c.next()
    await c.next()
    await service.forget(linkId)
    expect(await c.next()).toEqual({ type: 'forget' })
    expect(store.get(linkId)).toBeUndefined()
    // No link left: listening stops.
    expect(service.port).toBeNull()
  })

  it('the phone\'s "forget" deletes the link here', async () => {
    const { offer, linkId, token } = await pairPhone()
    const keep = await pairPhone({ id: 'phone-2', name: 'Autre', platform: 'android' })
    const c = await control(offer.port, linkId, token)
    await c.next()
    await c.next()
    c.send({ type: 'forget' })
    await vi.waitFor(() => expect(store.get(linkId)).toBeUndefined())
    expect(store.get(keep.linkId)).toBeDefined()
    const stale = await open(offer.port, linkId)
    stale.send({ type: 'hello', v: 1, linkId, token, role: 'control', device: PHONE })
    expect(await stale.next()).toEqual({ type: 'error', code: 'unknown-link' })
  })
})

describe('tunnel', () => {
  const tunnel = async (port: number, linkId: string, token: string, target: number) => {
    const t = await open(port, linkId)
    t.send({ type: 'hello', v: 1, linkId, token, role: 'tunnel', port: target })
    return t
  }
  const get = async (t: Phone): Promise<string> => {
    t.socket.write('GET / HTTP/1.0\r\nHost: localhost\r\n\r\n')
    const chunks: Buffer[] = []
    await new Promise<void>((resolve) => {
      t.socket.on('data', (c: Buffer) => chunks.push(c))
      t.socket.on('close', () => resolve())
    })
    return Buffer.concat(chunks).toString()
  }

  it('relays bytes to 127.0.0.1', async () => {
    const { offer, linkId, token } = await pairPhone()
    const { port } = await web('127.0.0.1', 'bonjour-v4')
    const t = await tunnel(offer.port, linkId, token, port)
    expect(await t.next()).toEqual({ type: 'ok' })
    expect(await get(t)).toContain('bonjour-v4')
  })

  it('tries ::1 when 127.0.0.1 is closed (Vite)', async (ctx) => {
    const { offer, linkId, token } = await pairPhone()
    let v6
    try {
      v6 = await web('::1', 'bonjour-v6')
    } catch {
      return ctx.skip()
    }
    const t = await tunnel(offer.port, linkId, token, v6.port)
    expect(await t.next()).toEqual({ type: 'ok' })
    expect(await get(t)).toContain('bonjour-v6')
  })

  it('bytes sent with the first line are not lost', async () => {
    const { offer, linkId, token } = await pairPhone()
    const { port } = await web('127.0.0.1', 'tout-court')
    const t = await open(offer.port, linkId)
    t.socket.write(encodeLinkMessage({ type: 'hello', v: 1, linkId, token, role: 'tunnel', port }) + 'GET / HTTP/1.0\r\nHost: localhost\r\n\r\n')
    expect(await t.next()).toEqual({ type: 'ok' })
    await vi.waitFor(() => expect(Buffer.concat(t.raw).toString()).toContain('tout-court'))
  })

  it('closed port: port-closed', async () => {
    const { offer, linkId, token } = await pairPhone()
    const closed = await web('127.0.0.1', '')
    await new Promise((r) => closed.server.close(r))
    const t = await tunnel(offer.port, linkId, token, closed.port)
    expect(await t.next()).toEqual({ type: 'error', code: 'port-closed' })
  })

  it("the link's own port is forbidden: port-refused", async () => {
    const { offer, linkId, token } = await pairPhone()
    const t = await tunnel(offer.port, linkId, token, offer.port)
    expect(await t.next()).toEqual({ type: 'error', code: 'port-refused' })
  })

  it('wrong token and unknown link', async () => {
    const { offer, linkId } = await pairPhone()
    const { port } = await web('127.0.0.1', 'x')
    expect(await (await tunnel(offer.port, linkId, 'C'.repeat(43), port)).next()).toEqual({ type: 'error', code: 'bad-token' })
    expect(await (await tunnel(offer.port, 'e'.repeat(16), 'C'.repeat(43), port)).next()).toEqual({ type: 'error', code: 'unknown-link' })
  })

  it('link paused here: paused; open tunnels are cut', async () => {
    const { offer, linkId, token } = await pairPhone()
    const { port } = await web('127.0.0.1', 'x')
    const open1 = await tunnel(offer.port, linkId, token, port)
    expect(await open1.next()).toEqual({ type: 'ok' })
    await service.setDevice(linkId, false)
    await vi.waitFor(() => expect(open1.closed).toBe(true))
    const t = await tunnel(offer.port, linkId, token, port)
    expect(await t.next()).toEqual({ type: 'error', code: 'paused' })
  })
})

describe('LineReader', () => {
  it('assembles lines split into chunks and keeps the rest', () => {
    const r = new LineReader()
    expect(r.push(Buffer.from('{"a":'))).toEqual([])
    expect(r.push(Buffer.from('1}\n{"b":2}\nreste'))).toEqual(['{"a":1}', '{"b":2}'])
    expect(r.rest().toString()).toBe('reste')
  })

  it('refuses a line that is too long', () => {
    expect(new LineReader().push(Buffer.alloc(LINK_MAX_LINE + 1, 97))).toBeNull()
  })
})

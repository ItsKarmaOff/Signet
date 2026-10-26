import { createECDH } from 'node:crypto'
import { encodeCbor, type CborValue } from './cbor'
import type { Eid } from './eid'
import { Crypter, Noise, P256_POINT_SIZE, qrPsk, tunnelId, tunnelUrl } from './noise'
import type { WebSocketCtor, WebSocketLike } from './tunnel'

// Test helpers only (never imported by the app): the phone's side of the tunnel. The handshake responder follows Chromium's
// RespondToHandshake (device/fido/cable/v2_handshake.cc, KNpsk0 case), and a fake WebSocket plays the tunnel server and the
// phone for tunnel and session tests.

export interface Responded {
  response: Buffer
  crypter: Crypter
  handshakeHash: Buffer
}

/** The phone's half of a QR (KNpsk0) handshake; `peerIdentity` is the uncompressed identity key from the QR code. */
export function respondToHandshake(psk: Uint8Array, peerIdentity: Uint8Array, input: Uint8Array): Responded | null {
  if (input.length < P256_POINT_SIZE) return null
  const peer = Buffer.from(input.subarray(0, P256_POINT_SIZE))
  const ciphertext = input.subarray(P256_POINT_SIZE)
  const noise = new Noise()
  noise.mixHash(Buffer.of(1))
  noise.mixHash(peerIdentity)
  noise.mixKeyAndHash(psk)
  noise.mixHash(peer)
  noise.mixKey(peer)
  const ephemeral = createECDH('prime256v1')
  ephemeral.generateKeys()
  let ee: Buffer
  let se: Buffer
  try {
    ee = ephemeral.computeSecret(peer)
    se = ephemeral.computeSecret(Buffer.from(peerIdentity))
  } catch {
    return null
  }
  const payload = noise.decryptAndHash(ciphertext)
  if (!payload || payload.length !== 0) return null
  const e = ephemeral.getPublicKey(null, 'uncompressed')
  noise.mixHash(e)
  noise.mixKey(e)
  noise.mixKey(ee)
  noise.mixKey(se)
  const response = Buffer.concat([e, noise.encryptAndHash(Buffer.alloc(0))])
  const [read, write] = noise.trafficKeys()
  return { response, crypter: new Crypter(read, write), handshakeHash: noise.handshakeHash }
}

export interface FakePhoneOptions {
  secret: Buffer
  eid: Eid
  /** Uncompressed public key of the QR identity. */
  identity: Buffer
  getInfo?: Uint8Array
  /** Replaces the post-handshake message (plaintext before padding); revision 0 pads it the old way. */
  postHandshake?: Uint8Array
  revision?: 0 | 1
  /** The server sends a one-byte zero frame first. */
  connectSignal?: boolean
  /** CTAP commands received; the return value is the response (status byte + CBOR), null: no answer. */
  onRequest?: (command: Buffer) => Buffer | null
  /** Never answers the handshake. */
  silent?: boolean
  /** Refuses the connection. */
  refuse?: boolean
  /** Subprotocol the server selects. */
  protocol?: string
}

export class FakeSocket implements WebSocketLike {
  binaryType = 'blob'
  protocol: string
  readyState = 0
  closeCode: number | undefined
  sent: Buffer[] = []
  private listeners: Record<string, ((e: { data?: unknown }) => void)[]> = {}
  private crypter: Crypter | null = null
  private open = false

  constructor(
    readonly url: string,
    readonly requested: string,
    private phone: FakePhone
  ) {
    this.protocol = phone.options.protocol ?? requested
    queueMicrotask(() => {
      if (phone.options.refuse) {
        this.emit('error', {})
        this.emit('close', {})
        return
      }
      this.open = true
      this.readyState = 1
      this.emit('open', {})
    })
  }

  addEventListener(type: string, listener: (e: { data?: unknown }) => void): void {
    ;(this.listeners[type] ??= []).push(listener)
  }

  private emit(type: string, e: { data?: unknown }): void {
    for (const l of this.listeners[type] ?? []) l(e)
  }

  /** The phone's message to the browser. */
  deliver(data: Uint8Array | string): void {
    if (!this.open) return
    queueMicrotask(() => this.open && this.emit('message', { data: typeof data === 'string' ? data : new Uint8Array(data).buffer }))
  }

  /** Encrypted message from the phone (type byte for revision 1). */
  say(type: number | null, payload: Uint8Array): void {
    const plain = type === null ? Buffer.from(payload) : Buffer.concat([Buffer.of(type), payload])
    this.deliver(this.crypter!.encrypt(plain))
  }

  /** The server or phone drops the connection. */
  drop(): void {
    this.open = false
    this.readyState = 3
    this.emit('close', {})
  }

  send(data: Uint8Array): void {
    this.sent.push(Buffer.from(data))
    const { options } = this.phone
    if (options.silent) return
    const message = Buffer.from(data)
    if (!this.crypter) {
      const psk = qrPsk(options.secret, options.eid.plaintext)
      const result = respondToHandshake(psk, options.identity, message)
      if (!result) return
      this.crypter = result.crypter
      if (options.connectSignal) this.deliver(Buffer.of(0))
      this.deliver(result.response)
      const revision = options.revision ?? 1
      const info = options.getInfo ?? Uint8Array.of(0xa0)
      const body = options.postHandshake ?? encodeCbor(new Map<CborValue, CborValue>([[1, info]]))
      if (revision === 1) this.say(null, body)
      else {
        // Revision 0: padded to 512 bytes, the last two bytes (little-endian) count the padding.
        const padded = Buffer.alloc(512)
        padded.set(body)
        padded.writeUInt16LE(512 - body.length - 2, 510)
        this.say(null, padded)
      }
      this.phone.connected(this)
      return
    }
    const plain = this.crypter.decrypt(message)
    if (!plain) {
      this.phone.badMessages++
      return
    }
    if ((options.revision ?? 1) === 0) return this.handleCommand(plain, null)
    const type = plain[0]!
    if (type === 0) this.phone.shutdowns++
    else if (type === 1) this.handleCommand(plain.subarray(1), 1)
  }

  private handleCommand(command: Buffer, type: number | null): void {
    this.phone.requests.push(command)
    const reply = this.phone.options.onRequest?.(command)
    if (reply) this.say(type, reply)
  }

  close(code?: number): void {
    this.closeCode = code
    this.open = false
    this.readyState = 3
    this.phone.closed++
    queueMicrotask(() => this.emit('close', {}))
  }
}

/** A phone behind a fake tunnel server: `Impl` is the WebSocket constructor to inject. */
export class FakePhone {
  sockets: FakeSocket[] = []
  requests: Buffer[] = []
  shutdowns = 0
  closed = 0
  badMessages = 0
  private waiters: ((s: FakeSocket) => void)[] = []

  constructor(readonly options: FakePhoneOptions) {}

  /** Set by fakePhone. */
  Impl!: WebSocketCtor

  /** The URL a correct client connects to. */
  get expectedUrl(): string {
    return tunnelUrl(this.options.eid.tunnelDomain, this.options.eid.routingId, tunnelId(this.options.secret))
  }

  connected(socket: FakeSocket): void {
    for (const w of this.waiters.splice(0)) w(socket)
  }

  /** The first socket whose handshake was answered. */
  whenConnected(): Promise<FakeSocket> {
    const ready = this.sockets.find((s) => s.sent.length > 0)
    return ready ? Promise.resolve(ready) : new Promise((r) => this.waiters.push(r))
  }
}

export function fakePhone(options: FakePhoneOptions): FakePhone {
  const phone = new FakePhone(options)
  phone.Impl = class extends FakeSocket {
    constructor(url: string, protocols: string) {
      super(url, protocols, phone)
      phone.sockets.push(this)
    }
  }
  return phone
}

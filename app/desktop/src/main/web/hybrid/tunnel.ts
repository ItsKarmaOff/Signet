import type { ECDH } from 'node:crypto'
import { decodeCbor, type CborValue } from './cbor'
import type { Eid } from './eid'
import { Crypter, HandshakeInitiator, qrPsk, tunnelId, tunnelUrl } from './noise'

// Tunnel to the phone ("hybrid" transport, caBLE v2): WebSocket to the tunnel server, Noise handshake, post-handshake message
// (getInfo), then encrypted CTAP2 messages. Implements fido_tunnel_device.cc for a QR-initiated transaction; linking
// information is ignored. Pure module (no Electron), the WebSocket is injectable. Tested. Checked against Chromium, base:
// https://raw.githubusercontent.com/chromium/chromium/main/
//   device/fido/cable/fido_tunnel_device.cc (constructor: tunnel id, psk, connect URL, subprotocol; OnTunnelData: states,
//     post-handshake message, MessageType prefix, kShutdown / kUpdate; EstablishedConnection::Transact / Close)
//   device/fido/cable/websocket_adapter.cc (selected subprotocol must match, 1 MiB message limit, connect signal frame)
//   device/fido/cable/v2_constants.h (MessageType values) and device/fido/public/fido_constants.h (kCableWebSocketProtocol)
//   device/fido/cable/v2_handshake.cc (DecodePaddedCBORMap for protocol revision 0)
// Nothing secret or read from the phone is ever logged.

const SUBPROTOCOL = 'fido.cable'
/** Largest message accepted from the tunnel (websocket_adapter.cc: kMaxIncomingMessageSize). */
const MAX_MESSAGE = 1 << 20
const CONNECT_TIMEOUT = 15_000
const HANDSHAKE_TIMEOUT = 15_000

/** MessageType of v2_constants.h. */
const MSG_SHUTDOWN = 0
const MSG_CTAP = 1
const MSG_UPDATE = 2

/** What the tunnel needs from a WebSocket (the global one in Node 22+). */
export interface WebSocketLike {
  binaryType: string
  protocol?: string
  send(data: Uint8Array): void
  close(code?: number, reason?: string): void
  addEventListener(type: 'open' | 'message' | 'close' | 'error', listener: (event: { data?: unknown }) => void): void
}
export type WebSocketCtor = new (url: string, protocols: string) => WebSocketLike

export type TunnelStage = 'connect' | 'handshake' | 'post-handshake' | 'transact' | 'closed' | 'aborted'

/** A failure of the tunnel; `stage` says where (safe to log), `message` never carries data from the phone. */
export class TunnelError extends Error {
  constructor(
    readonly stage: TunnelStage,
    message: string
  ) {
    super(message)
    this.name = 'TunnelError'
  }
}

export interface TunnelOptions {
  eid: Eid
  /** QR secret (16 bytes). */
  secret: Uint8Array
  /** The identity key pair of the QR code. */
  identity: ECDH
  /** Closes the tunnel and fails whatever is pending when aborted. */
  signal?: AbortSignal
  WebSocketImpl?: WebSocketCtor
  connectTimeout?: number
  handshakeTimeout?: number
  /** Development log of the stages (no data). */
  log?: (line: string) => void
}

export interface Tunnel {
  /** The phone's getInfo response (CTAP2 CBOR), from the post-handshake message. */
  getInfo: Buffer
  /** Sends a CTAP2 command (command byte + CBOR) and resolves with the response (status byte + CBOR). One at a time. */
  transact(request: Buffer, signal?: AbortSignal): Promise<Buffer>
  close(): void
}

const toBuffer = (data: unknown): Buffer | null => {
  if (data instanceof ArrayBuffer) return Buffer.from(data)
  if (ArrayBuffer.isView(data)) return Buffer.from(data.buffer, data.byteOffset, data.byteLength)
  return null
}

/** Padding of revision 0 post-handshake messages: the last 2 bytes (little-endian) or the last byte count the padding. */
function decodePaddedMap(input: Uint8Array): CborValue | null {
  const attempts: [number, (b: Buffer) => number][] = [
    [2, (b) => b.readUInt16LE(b.length - 2)],
    [1, (b) => b[b.length - 1]!]
  ]
  const buf = Buffer.from(input)
  for (const [size, read] of attempts) {
    if (buf.length < size) continue
    const padding = read(buf)
    if (padding + size > buf.length) continue
    try {
      const value = decodeCbor(buf.subarray(0, buf.length - padding - size))
      if (value instanceof Map) return value
    } catch {
      // try the other format
    }
  }
  return null
}

interface PostHandshake {
  revision: number
  getInfo: Buffer
}

/** The post-handshake message: a CBOR map {1: getInfo bytes, 2: linking info (ignored), 3: features}. */
function parsePostHandshake(plaintext: Uint8Array): PostHandshake | null {
  let revision = 1
  let payload: CborValue | null
  try {
    payload = decodeCbor(plaintext)
  } catch {
    revision = 0
    payload = decodePaddedMap(plaintext)
  }
  if (!(payload instanceof Map)) return null
  const features = payload.get(3)
  let ctap = true
  if (features !== undefined) {
    if (!Array.isArray(features)) return null
    ctap = features.includes('ctap')
  }
  if (!ctap) return null // only CTAP2 is used here
  const getInfo = payload.get(1)
  if (!(getInfo instanceof Uint8Array)) return null
  const linking = payload.get(2)
  if (linking !== undefined && !(linking instanceof Map)) return null
  return { revision, getInfo: Buffer.from(getInfo) }
}

/** Incoming messages in order, with one reader at a time. */
class Inbox {
  private queue: Buffer[] = []
  private waiter: { resolve: (m: Buffer) => void; reject: (e: Error) => void } | null = null
  private failure: Error | null = null

  push(m: Buffer): void {
    if (this.waiter) {
      const { resolve } = this.waiter
      this.waiter = null
      resolve(m)
    } else this.queue.push(m)
  }

  fail(e: Error): void {
    if (this.failure) return
    this.failure = e
    if (this.waiter) {
      const { reject } = this.waiter
      this.waiter = null
      reject(e)
    }
  }

  /** The next message; rejects when the tunnel failed, on timeout or abort. */
  next(stage: TunnelStage, timeout: number, signal?: AbortSignal): Promise<Buffer> {
    const queued = this.queue.shift()
    if (queued) return Promise.resolve(queued)
    if (this.failure) return Promise.reject(this.failure)
    if (this.waiter) return Promise.reject(new Error('Inbox: one reader at a time'))
    return new Promise<Buffer>((resolve, reject) => {
      const done = (): void => {
        clearTimeout(timer)
        signal?.removeEventListener('abort', onAbort)
        this.waiter = null
      }
      const timer = setTimeout(() => {
        done()
        reject(new TunnelError(stage, 'timed out'))
      }, timeout)
      const onAbort = (): void => {
        done()
        reject(new TunnelError('aborted', 'aborted'))
      }
      if (signal?.aborted) return onAbort()
      signal?.addEventListener('abort', onAbort)
      this.waiter = {
        resolve: (m) => {
          done()
          resolve(m)
        },
        reject: (e) => {
          done()
          reject(e)
        }
      }
    })
  }
}

/**
 * Connects to the tunnel server, runs the handshake with the phone and reads its post-handshake message. Resolves once the
 * phone is ready for CTAP2; rejects with a TunnelError otherwise (the socket is closed then).
 */
export async function openTunnel(opts: TunnelOptions): Promise<Tunnel> {
  const { eid, secret, identity, signal, log } = opts
  const Impl = opts.WebSocketImpl ?? (globalThis as { WebSocket?: WebSocketCtor }).WebSocket
  if (!Impl) throw new TunnelError('connect', 'no WebSocket available')
  if (signal?.aborted) throw new TunnelError('aborted', 'aborted')

  const url = tunnelUrl(eid.tunnelDomain, eid.routingId, tunnelId(secret))
  log?.(`tunnel: connecting to ${eid.tunnelDomain}`)
  const ws = new Impl(url, SUBPROTOCOL)
  ws.binaryType = 'arraybuffer'

  const inbox = new Inbox()
  let closed = false
  let crypter: Crypter | null = null
  let revision = 1
  let ready = false

  const shutdown = (): void => {
    if (closed) return
    closed = true
    // A polite end for the phone (revision 1 and later), then the socket.
    if (ready && crypter && revision >= 1) {
      try {
        ws.send(crypter.encrypt(Buffer.of(MSG_SHUTDOWN)))
      } catch {
        // closing anyway
      }
    }
    try {
      ws.close(1000)
    } catch {
      // already closed
    }
    inbox.fail(new TunnelError('closed', 'closed'))
  }
  const onAbort = (): void => shutdown()
  signal?.addEventListener('abort', onAbort)
  const forget = (): void => signal?.removeEventListener('abort', onAbort)

  ws.addEventListener('message', (e) => {
    if (closed) return
    const data = toBuffer(e.data)
    if (!data) return fail(new TunnelError('closed', 'unexpected text message'))
    if (data.length > MAX_MESSAGE) return fail(new TunnelError('closed', 'message too large'))
    inbox.push(data)
  })
  ws.addEventListener('close', () => {
    forget()
    if (!closed) {
      closed = true
      inbox.fail(new TunnelError('closed', 'connection closed'))
    }
  })
  ws.addEventListener('error', () => {
    if (!closed) fail(new TunnelError('closed', 'connection error'))
  })
  function fail(e: TunnelError): void {
    inbox.fail(e)
    shutdown()
  }

  const opened = new Promise<void>((resolve, reject) => {
    const timer = setTimeout(() => reject(new TunnelError('connect', 'timed out')), opts.connectTimeout ?? CONNECT_TIMEOUT)
    ws.addEventListener('open', () => {
      clearTimeout(timer)
      resolve()
    })
    ws.addEventListener('close', () => {
      clearTimeout(timer)
      reject(new TunnelError('connect', 'connection refused or closed'))
    })
    ws.addEventListener('error', () => {
      clearTimeout(timer)
      reject(new TunnelError('connect', 'connection error'))
    })
    signal?.addEventListener('abort', () => {
      clearTimeout(timer)
      reject(new TunnelError('aborted', 'aborted'))
    })
  })

  try {
    await opened
    if (ws.protocol !== undefined && ws.protocol !== SUBPROTOCOL) throw new TunnelError('connect', 'unexpected subprotocol')
    log?.('tunnel: connected, handshake')
    const handshake = new HandshakeInitiator(qrPsk(secret, eid.plaintext), identity)
    ws.send(handshake.buildInitialMessage())

    const handshakeTimeout = opts.handshakeTimeout ?? HANDSHAKE_TIMEOUT
    let response = await inbox.next('handshake', handshakeTimeout, signal)
    // A server that supports it first sends a one-byte zero frame when the phone has connected (never a valid response).
    if (response.length === 1 && response[0] === 0) response = await inbox.next('handshake', handshakeTimeout, signal)
    const result = handshake.processResponse(response)
    if (!result) throw new TunnelError('handshake', 'handshake failed')
    crypter = result.crypter
    log?.('tunnel: handshake done')

    const encrypted = await inbox.next('post-handshake', handshakeTimeout, signal)
    const plain = crypter.decrypt(encrypted)
    const post = plain && parsePostHandshake(plain)
    if (!post) throw new TunnelError('post-handshake', 'invalid post-handshake message')
    revision = post.revision
    ready = true
    log?.(`tunnel: ready (protocol revision ${revision})`)

    let busy = false
    return {
      getInfo: post.getInfo,
      async transact(request, txSignal) {
        if (closed) throw new TunnelError('closed', 'closed')
        if (busy) throw new TunnelError('transact', 'a request is already pending')
        busy = true
        try {
          const message = revision >= 1 ? Buffer.concat([Buffer.of(MSG_CTAP), request]) : request
          ws.send(crypter!.encrypt(message))
          const both = txSignal && signal ? AbortSignal.any([txSignal, signal]) : (txSignal ?? signal)
          for (;;) {
            // Bounded by the signal (the session timeout); an hour is only the ceiling of the timer.
            const data = await inbox.next('transact', 3_600_000, both)
            const plaintext = crypter!.decrypt(data)
            if (!plaintext) throw new TunnelError('transact', 'decryption failed')
            if (revision < 1) return plaintext
            if (plaintext.length === 0) throw new TunnelError('transact', 'empty message')
            const type = plaintext[0]
            if (type === MSG_CTAP) return plaintext.subarray(1)
            if (type === MSG_UPDATE) {
              // Linking information: not used, but it must be a map.
              let update: CborValue
              try {
                update = decodeCbor(plaintext.subarray(1))
              } catch {
                throw new TunnelError('transact', 'invalid update message')
              }
              if (!(update instanceof Map)) throw new TunnelError('transact', 'invalid update message')
              continue
            }
            throw new TunnelError('transact', 'unexpected message type')
          }
        } catch (e) {
          // Anything wrong ends the tunnel (the phone may still answer later).
          shutdown()
          throw e
        } finally {
          busy = false
        }
      },
      close() {
        forget()
        shutdown()
      }
    }
  } catch (e) {
    forget()
    shutdown()
    throw e instanceof TunnelError ? e : new TunnelError('connect', 'failed')
  }
}

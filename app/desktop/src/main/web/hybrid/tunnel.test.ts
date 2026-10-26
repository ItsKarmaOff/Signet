import { createECDH } from 'node:crypto'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { encodeCbor, type CborValue } from './cbor'
import type { Eid } from './eid'
import { fakePhone, type FakePhoneOptions } from './phone.testing'
import { openTunnel, TunnelError, type TunnelOptions, type WebSocketLike } from './tunnel'

const seq = (start: number, n = 16): Buffer => Buffer.from(Array.from({ length: n }, (_, i) => start + i))

const eid: Eid = {
  plaintext: seq(16),
  nonce: seq(1, 10),
  routingId: Buffer.from([0xaa, 0xbb, 0xcc]),
  tunnelDomainId: 0,
  tunnelDomain: 'cable.ua5v.com'
}
const secret = seq(0)
const identity = createECDH('prime256v1')
identity.generateKeys()

const GET_INFO = encodeCbor(new Map<CborValue, CborValue>([[1, ['FIDO_2_0']]]))

function setup(extra: Partial<FakePhoneOptions> = {}, opts: Partial<TunnelOptions> = {}) {
  const phone = fakePhone({ secret, eid, identity: identity.getPublicKey(null, 'uncompressed'), getInfo: GET_INFO, ...extra })
  const open = (more: Partial<TunnelOptions> = {}) => openTunnel({ eid, secret, identity, WebSocketImpl: phone.Impl, ...opts, ...more })
  return { phone, open }
}

const rejection = async (p: Promise<unknown>): Promise<TunnelError> => {
  try {
    await p
  } catch (e) {
    expect(e).toBeInstanceOf(TunnelError)
    return e as TunnelError
  }
  throw new Error('did not reject')
}

/** Lets timers (fake) and microtasks run. */
const flush = async (ms = 0): Promise<void> => {
  await vi.advanceTimersByTimeAsync(ms)
}

beforeEach(() => vi.useFakeTimers())
afterEach(() => vi.useRealTimers())

describe('openTunnel', () => {
  it('connects to the tunnel with the fido.cable subprotocol and reads getInfo', async () => {
    const { phone, open } = setup()
    const tunnel = await open()
    const [socket] = phone.sockets
    expect(socket!.url).toBe(phone.expectedUrl)
    expect(socket!.url).toMatch(/^wss:\/\/cable\.ua5v\.com\/cable\/connect\/AABBCC\/[0-9A-F]{32}$/)
    expect(socket!.requested).toBe('fido.cable')
    expect(socket!.binaryType).toBe('arraybuffer')
    expect(Buffer.from(tunnel.getInfo).equals(Buffer.from(GET_INFO))).toBe(true)
    expect(phone.badMessages).toBe(0)
  })

  it('exchanges a CTAP request and response (type byte kCTAP, padded and encrypted)', async () => {
    const { phone, open } = setup({ onRequest: () => Buffer.from([0, 0xa1, 1, 2]) })
    const tunnel = await open()
    const answer = await tunnel.transact(Buffer.from([2, 0xa0]))
    expect([...answer]).toEqual([0, 0xa1, 1, 2])
    expect(phone.requests.map((r) => [...r])).toEqual([[2, 0xa0]])
    // Both messages are ciphertext (a multiple of 32 plus a 16-byte tag).
    expect((phone.sockets[0]!.sent[1]!.length - 16) % 32).toBe(0)
  })

  it('allows several requests, one after the other', async () => {
    const { open } = setup({ onRequest: (c) => Buffer.concat([Buffer.of(0), c]) })
    const tunnel = await open()
    expect([...(await tunnel.transact(Buffer.from([1])))]).toEqual([0, 1])
    expect([...(await tunnel.transact(Buffer.from([2])))]).toEqual([0, 2])
  })

  it('skips the one-byte connect signal some servers send before the response', async () => {
    const { open } = setup({ connectSignal: true })
    await expect(open()).resolves.toBeDefined()
  })

  it('ignores linking information and extra keys in the post-handshake message', async () => {
    const post = encodeCbor(
      new Map<CborValue, CborValue>([
        [1, Uint8Array.of(0xa0)],
        [2, new Map<CborValue, CborValue>([[1, 'x']])],
        [3, ['ctap', 'dc', 'other']],
        [9, 'x']
      ])
    )
    const { open } = setup({ postHandshake: post })
    expect(Buffer.from((await open()).getInfo)).toEqual(Buffer.from([0xa0]))
  })

  it('supports protocol revision 0 (padded post-handshake message, no message types)', async () => {
    const { phone, open } = setup({ revision: 0, onRequest: () => Buffer.from([0, 0xa0]) })
    const tunnel = await open()
    expect([...(await tunnel.transact(Buffer.from([1, 2])))]).toEqual([0, 0xa0])
    expect(phone.requests.map((r) => [...r])).toEqual([[1, 2]])
  })

  describe('post-handshake message', () => {
    const bad = (name: string, map: Map<CborValue, CborValue> | Uint8Array): void => {
      it(`rejects ${name}`, async () => {
        const { phone, open } = setup({ postHandshake: map instanceof Map ? encodeCbor(map) : map })
        const e = await rejection(open())
        expect(e.stage).toBe('post-handshake')
        expect(phone.closed).toBe(1)
      })
    }
    bad('a message that is not a map', Uint8Array.of(0x01))
    bad('a message that is not CBOR', Uint8Array.of(0xff, 0xff, 0xff))
    bad('a missing getInfo', new Map([[3, ['ctap']]]))
    bad('a getInfo that is not bytes', new Map<CborValue, CborValue>([[1, 'text']]))
    bad(
      'features without ctap',
      new Map<CborValue, CborValue>([
        [1, Uint8Array.of(0xa0)],
        [3, ['dc']]
      ])
    )
    bad(
      'features that are not an array',
      new Map<CborValue, CborValue>([
        [1, Uint8Array.of(0xa0)],
        [3, 'ctap']
      ])
    )
    bad(
      'linking info that is not a map',
      new Map<CborValue, CborValue>([
        [1, Uint8Array.of(0xa0)],
        [2, 5]
      ])
    )
  })

  describe('connection failures', () => {
    it('a refused connection', async () => {
      const { open } = setup({ refuse: true })
      expect((await rejection(open())).stage).toBe('connect')
    })

    it('a connection that never opens times out after 15 s and is closed', async () => {
      const sockets: { closed: boolean }[] = []
      class Hang implements WebSocketLike {
        binaryType = ''
        closed = false
        constructor() {
          sockets.push(this)
        }
        addEventListener(): void {}
        send(): void {}
        close(): void {
          this.closed = true
        }
      }
      const p = rejection(openTunnel({ eid, secret, identity, WebSocketImpl: Hang }))
      await flush(14_999)
      expect(sockets[0]!.closed).toBe(false)
      await flush(1)
      expect((await p).stage).toBe('connect')
      expect(sockets[0]!.closed).toBe(true)
    })

    it('another subprotocol than fido.cable', async () => {
      const { phone, open } = setup({ protocol: 'other' })
      expect((await rejection(open())).stage).toBe('connect')
      expect(phone.closed).toBe(1)
    })

    it('no answer to the handshake: times out after 15 s', async () => {
      const { phone, open } = setup({ silent: true })
      const p = rejection(open())
      await flush(14_999)
      expect(phone.closed).toBe(0)
      await flush(1)
      expect((await p).stage).toBe('handshake')
      expect(phone.closed).toBe(1)
    })

    it('an answer that is not a valid handshake response', async () => {
      const { phone, open } = setup({ silent: true })
      const p = rejection(open())
      await flush()
      phone.sockets[0]!.deliver(Buffer.alloc(81, 7))
      expect((await p).stage).toBe('handshake')
      expect(phone.closed).toBe(1)
    })

    it('a phone that does not know the psk of this QR code never answers', async () => {
      const { phone, open } = setup({ secret: seq(50) })
      const p = rejection(open())
      await flush(15_000)
      expect((await p).stage).toBe('handshake')
      expect(phone.sockets[0]!.sent.length).toBe(1)
    })

    it('the connection dropped during the handshake', async () => {
      const { phone, open } = setup({ silent: true })
      const p = rejection(open())
      await flush()
      phone.sockets[0]!.drop()
      expect((await p).stage).toBe('closed')
    })

    it('a text message', async () => {
      const { phone, open } = setup({ silent: true })
      const p = rejection(open())
      await flush()
      phone.sockets[0]!.deliver('hello')
      await p
      expect(phone.closed).toBe(1)
    })

    it('a message over 1 MiB', async () => {
      const { phone, open } = setup({ silent: true })
      const p = rejection(open())
      await flush()
      phone.sockets[0]!.deliver(Buffer.alloc((1 << 20) + 1))
      await p
      expect(phone.closed).toBe(1)
    })

    it('abort before connecting never opens a socket', async () => {
      const { phone, open } = setup()
      const ac = new AbortController()
      ac.abort()
      expect((await rejection(open({ signal: ac.signal }))).stage).toBe('aborted')
      expect(phone.sockets.length).toBe(0)
    })

    it('abort during the handshake closes the socket', async () => {
      const { phone, open } = setup({ silent: true })
      const ac = new AbortController()
      const p = rejection(open({ signal: ac.signal }))
      await flush()
      ac.abort()
      await p
      expect(phone.closed).toBe(1)
    })

    it('without a WebSocket implementation', async () => {
      const original = (globalThis as { WebSocket?: unknown }).WebSocket
      delete (globalThis as { WebSocket?: unknown }).WebSocket
      try {
        expect((await rejection(openTunnel({ eid, secret, identity }))).stage).toBe('connect')
      } finally {
        ;(globalThis as { WebSocket?: unknown }).WebSocket = original
      }
    })
  })

  describe('transact', () => {
    it('ignores update messages (linking) before the answer', async () => {
      const { phone, open } = setup({ onRequest: () => null })
      const tunnel = await open()
      const p = tunnel.transact(Buffer.from([1]))
      await flush()
      const socket = phone.sockets[0]!
      socket.say(2, encodeCbor(new Map<CborValue, CborValue>([[1, new Map()]])))
      socket.say(1, Uint8Array.of(0, 0xa0))
      expect([...(await p)]).toEqual([0, 0xa0])
    })

    it('an invalid update message ends the tunnel', async () => {
      const { phone, open } = setup({ onRequest: () => null })
      const tunnel = await open()
      const p = rejection(tunnel.transact(Buffer.from([1])))
      await flush()
      phone.sockets[0]!.say(2, Uint8Array.of(0xff))
      expect((await p).stage).toBe('transact')
      expect(phone.closed).toBe(1)
    })

    it('a shutdown message from the phone, an unknown type or an empty message end the tunnel', async () => {
      for (const type of [0, 3, 9]) {
        const { phone, open } = setup({ onRequest: () => null })
        const tunnel = await open()
        const p = rejection(tunnel.transact(Buffer.from([1])))
        await flush()
        phone.sockets[0]!.say(type, Uint8Array.of(0))
        expect((await p).stage).toBe('transact')
      }
      const { phone, open } = setup({ onRequest: () => null })
      const tunnel = await open()
      const p = rejection(tunnel.transact(Buffer.from([1])))
      await flush()
      phone.sockets[0]!.say(null, new Uint8Array())
      expect((await p).stage).toBe('transact')
    })

    it('a message that does not decrypt ends the tunnel', async () => {
      const { phone, open } = setup({ onRequest: () => null })
      const tunnel = await open()
      const p = rejection(tunnel.transact(Buffer.from([1])))
      await flush()
      phone.sockets[0]!.deliver(Buffer.alloc(48, 1))
      expect((await p).stage).toBe('transact')
      expect(phone.closed).toBe(1)
    })

    it('the phone dropping the connection', async () => {
      const { phone, open } = setup({ onRequest: () => null })
      const tunnel = await open()
      const p = rejection(tunnel.transact(Buffer.from([1])))
      await flush()
      phone.sockets[0]!.drop()
      expect((await p).stage).toBe('closed')
    })

    it('is bounded by the signal given to it, and closes the tunnel', async () => {
      const { phone, open } = setup({ onRequest: () => null })
      const tunnel = await open()
      const ac = new AbortController()
      const p = rejection(tunnel.transact(Buffer.from([1]), ac.signal))
      await flush(60_000)
      ac.abort()
      expect((await p).stage).toBe('aborted')
      expect(phone.closed).toBe(1)
      expect(phone.shutdowns).toBe(1)
    })

    it('is bounded by the signal of the tunnel too', async () => {
      const { phone, open } = setup({ onRequest: () => null })
      const ac = new AbortController()
      const tunnel = await open({ signal: ac.signal })
      const p = rejection(tunnel.transact(Buffer.from([1])))
      await flush()
      ac.abort()
      await p
      expect(phone.closed).toBe(1)
    })

    it('one request at a time', async () => {
      const { open } = setup({ onRequest: () => null })
      const tunnel = await open()
      void tunnel.transact(Buffer.from([1])).catch(() => {})
      await flush()
      expect((await rejection(tunnel.transact(Buffer.from([2])))).stage).toBe('transact')
    })
  })

  describe('close', () => {
    it('tells the phone (kShutdown) and closes the socket, once', async () => {
      const { phone, open } = setup()
      const tunnel = await open()
      tunnel.close()
      tunnel.close()
      await flush()
      expect(phone.shutdowns).toBe(1)
      expect(phone.closed).toBe(1)
      expect(phone.sockets[0]!.closeCode).toBe(1000)
    })

    it('makes later requests fail', async () => {
      const { open } = setup()
      const tunnel = await open()
      tunnel.close()
      expect((await rejection(tunnel.transact(Buffer.from([1])))).stage).toBe('closed')
    })

    it('does not send kShutdown before the tunnel is ready', async () => {
      const { phone, open } = setup({ silent: true })
      const ac = new AbortController()
      const p = rejection(open({ signal: ac.signal }))
      await flush()
      ac.abort()
      await p
      expect(phone.shutdowns).toBe(0)
      expect(phone.sockets[0]!.sent.length).toBe(1)
    })
  })

  it('logs stages without data', async () => {
    const lines: string[] = []
    const { open } = setup()
    await open({ log: (l) => lines.push(l) })
    expect(lines.length).toBeGreaterThan(2)
    const all = lines.join('\n')
    for (const secretBytes of [secret, eid.plaintext, eid.nonce]) expect(all).not.toContain(secretBytes.toString('hex'))
  })
})

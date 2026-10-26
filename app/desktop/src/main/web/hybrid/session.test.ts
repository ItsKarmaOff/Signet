import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { WebAuthnRequest } from '../webauthn-request'
import { createCipheriv, createHmac } from 'node:crypto'
import { createHash } from 'node:crypto'
import { encodeCbor, type CborValue } from './cbor'
import { clientDataJson } from './ctap'
import { eidKeys } from './eid'
import { FAILED_DELAY, HybridSessions, hybridTimeout, type HybridHost, type HybridTarget, type ScanState } from './session'
import type { Tunnel, TunnelOptions } from './tunnel'

const request = (publicKey: Record<string, unknown> = {}, type: 'get' | 'create' = 'get'): WebAuthnRequest => ({
  type,
  rpId: 'example.com',
  origin: 'https://example.com',
  publicKey
})
const target = (senderId = 1, frameKey = '1:1'): HybridTarget => ({ senderId, frameKey, source: null })

function makeHost() {
  const closed: boolean[] = []
  const onClosed: (() => void)[] = []
  const gone: (() => void)[] = []
  const unwatched: number[] = []
  const scans: { onData: (b: Uint8Array) => void; onState: (s: ScanState) => void; stopped: number }[] = []
  const statuses: string[] = []
  const found: unknown[] = []
  const logs: string[] = []
  /** Calls of openTunnel, to be answered by the test. */
  const opens: { options: TunnelOptions; resolve: (t: Tunnel) => void; reject: (e: unknown) => void }[] = []
  const host: HybridHost = {
    render: (text) => Promise.resolve(`qr:${text}`),
    openWindow: (_t, cb) => {
      const i = closed.push(false) - 1
      onClosed.push(cb)
      return () => (closed[i] = true)
    },
    watch: (_t, cb) => {
      const i = gone.push(cb) - 1
      return () => unwatched.push(i)
    },
    scan: (onData, onState) => {
      const scan = { onData, onState, stopped: 0 }
      scans.push(scan)
      return () => scan.stopped++
    },
    status: (s) => statuses.push(s),
    found: (e) => found.push(e),
    openTunnel: (options) => new Promise<Tunnel>((resolve, reject) => opens.push({ options, resolve, reject })),
    log: (line) => logs.push(line)
  }
  return { host, closed, onClosed, gone, unwatched, scans, statuses, found, logs, opens }
}

const NOT_ALLOWED = { error: { name: 'NotAllowedError', message: expect.stringContaining('timed out or was not allowed') } }

beforeEach(() => vi.useFakeTimers())
afterEach(() => vi.useRealTimers())

describe('hybridTimeout', () => {
  it('clamps like Chrome, 5 minutes by default', () => {
    expect(hybridTimeout({})).toBe(300_000)
    expect(hybridTimeout({ timeout: 'x' })).toBe(300_000)
    expect(hybridTimeout({ timeout: 1 })).toBe(10_000)
    expect(hybridTimeout({ timeout: 60_000 })).toBe(60_000)
    expect(hybridTimeout({ timeout: 10 ** 9 })).toBe(600_000)
  })
})

/** The advert a phone sends for this secret. */
function advertFor(secret: Uint8Array, domain = 0): Buffer {
  const plain = Buffer.alloc(16)
  plain.set([1, 2, 3, 4, 5, 6, 7, 8, 9, 10], 1)
  plain.set([0xaa, 0xbb, 0xcc], 11)
  plain.writeUInt16LE(domain, 14)
  const { aes, hmac } = eidKeys(secret)
  const c = createCipheriv('aes-256-ecb', aes, null)
  c.setAutoPadding(false)
  const block = Buffer.concat([c.update(plain), c.final()])
  return Buffer.concat([block, createHmac('sha256', hmac).update(block).digest().subarray(0, 4)])
}

describe('HybridSessions', () => {
  it('shows the prompt, then ends cancelled from the window', async () => {
    const h = makeHost()
    const sessions = new HybridSessions(h.host)
    const answer = sessions.start(request({}, 'create'), target())
    await vi.advanceTimersByTimeAsync(0)
    expect(sessions.prompt()).toMatchObject({ type: 'create', rpId: 'example.com' })
    expect(sessions.prompt()!.qr.startsWith('qr:FIDO:/')).toBe(true)
    sessions.cancel()
    await expect(answer).resolves.toEqual(NOT_ALLOWED)
    expect(h.closed).toEqual([true])
    expect(sessions.prompt()).toBeNull()
  })

  it('ends when the window is closed', async () => {
    const h = makeHost()
    const sessions = new HybridSessions(h.host)
    const answer = sessions.start(request(), target())
    await vi.advanceTimersByTimeAsync(0)
    h.onClosed[0]!()
    await expect(answer).resolves.toEqual(NOT_ALLOWED)
    expect(sessions.prompt()).toBeNull()
  })

  it('ends on timeout and cleans up', async () => {
    const h = makeHost()
    const sessions = new HybridSessions(h.host)
    const answer = sessions.start(request({ timeout: 30_000 }), target())
    await vi.advanceTimersByTimeAsync(29_999)
    expect(sessions.prompt()).not.toBeNull()
    await vi.advanceTimersByTimeAsync(1)
    await expect(answer).resolves.toEqual(NOT_ALLOWED)
    expect(h.closed).toEqual([true])
    expect(h.unwatched).toEqual([0])
    expect(vi.getTimerCount()).toBe(0)
  })

  it('ends when the tab navigates away or is destroyed', async () => {
    const h = makeHost()
    const sessions = new HybridSessions(h.host)
    const answer = sessions.start(request(), target())
    await vi.advanceTimersByTimeAsync(0)
    h.gone[0]!()
    await expect(answer).resolves.toEqual(NOT_ALLOWED)
    expect(h.closed).toEqual([true])
  })

  it('ends on abort from the same sender and frame only', async () => {
    const h = makeHost()
    const sessions = new HybridSessions(h.host)
    const answer = sessions.start(request(), target(1, '1:1'))
    await vi.advanceTimersByTimeAsync(0)
    sessions.abort(2, '1:1')
    sessions.abort(1, '1:2')
    expect(sessions.prompt()).not.toBeNull()
    sessions.abort(1, '1:1')
    await expect(answer).resolves.toEqual(NOT_ALLOWED)
    expect(sessions.prompt()).toBeNull()
  })

  it('a new request ends the previous one', async () => {
    const h = makeHost()
    const sessions = new HybridSessions(h.host)
    const first = sessions.start(request(), target(1))
    await vi.advanceTimersByTimeAsync(0)
    const second = sessions.start(request({}, 'create'), target(2))
    await expect(first).resolves.toEqual(NOT_ALLOWED)
    await vi.advanceTimersByTimeAsync(0)
    expect(h.closed).toEqual([true, false])
    expect(sessions.prompt()).toMatchObject({ type: 'create' })
    // The old window closing late does not end the new session.
    h.onClosed[0]!()
    expect(sessions.prompt()).not.toBeNull()
    sessions.cancel()
    await expect(second).resolves.toEqual(NOT_ALLOWED)
  })

  it('a session replaced while its code is drawn never opens a window', async () => {
    const h = makeHost()
    const sessions = new HybridSessions(h.host)
    const first = sessions.start(request(), target(1))
    const second = sessions.start(request(), target(2))
    await expect(first).resolves.toEqual(NOT_ALLOWED)
    await vi.advanceTimersByTimeAsync(0)
    expect(h.closed).toEqual([false])
    sessions.cancel()
    await second
  })

  it('answers once: later events change nothing', async () => {
    const h = makeHost()
    const sessions = new HybridSessions(h.host)
    const answer = sessions.start(request(), target())
    await vi.advanceTimersByTimeAsync(0)
    sessions.cancel()
    sessions.cancel()
    h.gone[0]!()
    h.onClosed[0]!()
    await vi.advanceTimersByTimeAsync(600_000)
    await expect(answer).resolves.toEqual(NOT_ALLOWED)
    expect(h.closed).toEqual([true])
  })

  it('ends if the code cannot be drawn', async () => {
    const h = makeHost()
    h.host.render = () => Promise.reject(new Error('boom'))
    const sessions = new HybridSessions(h.host)
    await expect(sessions.start(request(), target())).resolves.toEqual(NOT_ALLOWED)
    expect(h.closed).toEqual([])
    expect(vi.getTimerCount()).toBe(0)
  })

  describe('phone discovery', () => {
    /** Starts a session and returns it with the fake host. */
    async function open() {
      const h = makeHost()
      const sessions = new HybridSessions(h.host)
      const answer = sessions.start(request(), target())
      await vi.advanceTimersByTimeAsync(0)
      const session = (sessions as unknown as { current: { keys: { secret: Buffer }; eid: { tunnelDomain: string; routingId: Buffer } | null } }).current
      return { h, sessions, answer, session }
    }

    it('starts the scan once the window is open and stops it when the session ends', async () => {
      const { h, sessions, answer } = await open()
      expect(h.scans).toHaveLength(1)
      expect(h.scans[0]!.stopped).toBe(0)
      expect(sessions.prompt()).toMatchObject({ status: 'waiting' })
      sessions.cancel()
      await answer
      expect(h.scans[0]!.stopped).toBe(1)
    })

    it('never scans for a session replaced before its window opens', async () => {
      const h = makeHost()
      const sessions = new HybridSessions(h.host)
      const first = sessions.start(request(), target(1))
      const second = sessions.start(request(), target(2))
      await first
      await vi.advanceTimersByTimeAsync(0)
      expect(h.scans).toHaveLength(1)
      sessions.cancel()
      await second
    })

    it('ignores adverts that are not for this session', async () => {
      const { h, sessions, session } = await open()
      h.scans[0]!.onData(advertFor(Buffer.alloc(16, 9)))
      h.scans[0]!.onData(Buffer.alloc(20))
      h.scans[0]!.onData(Buffer.alloc(3))
      expect(session.eid).toBeNull()
      expect(h.scans[0]!.stopped).toBe(0)
      expect(sessions.prompt()).toMatchObject({ status: 'waiting' })
      expect(h.found).toEqual([])
    })

    it('keeps the decrypted advert, stops the scan and starts connecting', async () => {
      const { h, sessions, session } = await open()
      h.scans[0]!.onData(advertFor(Buffer.alloc(16, 9)))
      h.scans[0]!.onData(advertFor(session.keys.secret, 1))
      expect(session.eid?.tunnelDomain).toBe('cable.auth.com')
      expect(session.eid?.routingId.toString('hex')).toBe('aabbcc')
      expect(h.scans[0]!.stopped).toBe(1)
      expect(h.statuses).toEqual(['connecting'])
      expect(h.found).toHaveLength(1)
      expect(h.opens).toHaveLength(1)
      expect(sessions.prompt()).toMatchObject({ status: 'connecting' })
      // The session stays open, and later adverts or scan states change nothing.
      h.scans[0]!.onData(advertFor(session.keys.secret, 0))
      h.scans[0]!.onState('error')
      expect(session.eid?.tunnelDomain).toBe('cable.auth.com')
      expect(sessions.prompt()).toMatchObject({ status: 'connecting' })
      expect(h.opens).toHaveLength(1)
      expect(h.closed).toEqual([false])
    })

    it('forwards scan states to the prompt, once each', async () => {
      const { h, sessions } = await open()
      h.scans[0]!.onState('off')
      h.scans[0]!.onState('off')
      expect(sessions.prompt()).toMatchObject({ status: 'off' })
      h.scans[0]!.onState('waiting')
      h.scans[0]!.onState('no-adapter')
      expect(h.statuses).toEqual(['off', 'waiting', 'no-adapter'])
    })

    it('says nothing once ended', async () => {
      const { h, sessions, answer } = await open()
      sessions.cancel()
      await answer
      h.scans[0]!.onState('denied')
      h.scans[0]!.onData(Buffer.alloc(20))
      expect(h.statuses).toEqual([])
    })
  })

  describe('tunnel and CTAP', () => {
    const getInfo = encodeCbor(
      new Map<CborValue, CborValue>([
        [1, ['FIDO_2_0']],
        [
          4,
          new Map<CborValue, CborValue>([
            ['uv', true],
            ['rk', true]
          ])
        ]
      ])
    )
    const challenge = Buffer.alloc(32, 5)
    const rpId = 'example.com'
    const getRequest = (): WebAuthnRequest => ({
      type: 'get',
      rpId,
      origin: `https://${rpId}`,
      publicKey: { challenge: challenge.toString('base64url'), allowCredentials: [{ type: 'public-key', id: Buffer.alloc(8, 1).toString('base64url') }] }
    })

    const fakeTunnel = (transact: Tunnel['transact']) => {
      const tunnel = { getInfo: Buffer.from(getInfo), transact: vi.fn(transact), close: vi.fn() }
      return tunnel
    }

    /** A valid answer to the getAssertion request above. */
    function assertion(auth = authenticatorData(rpId)): Buffer {
      return Buffer.concat([
        Buffer.of(0),
        Buffer.from(
          encodeCbor(
            new Map<CborValue, CborValue>([
              [2, new Uint8Array(auth)],
              [3, Uint8Array.of(1, 2, 3)]
            ])
          )
        )
      ])
    }
    const authenticatorData = (id: string): Buffer => Buffer.concat([createHash('sha256').update(id).digest(), Buffer.of(0x05), Buffer.alloc(4)])

    /** Session with the phone found and the tunnel being opened. */
    async function connecting(req = getRequest()) {
      const h = makeHost()
      const sessions = new HybridSessions(h.host)
      const answer = sessions.start(req, target())
      await vi.advanceTimersByTimeAsync(0)
      const session = (sessions as unknown as { current: { keys: { secret: Buffer } } }).current
      h.scans[0]!.onData(advertFor(session.keys.secret, 0))
      return { h, sessions, answer, session }
    }

    it('opens the tunnel with the advert, the QR secret and the identity key', async () => {
      const { h, session } = await connecting()
      expect(h.opens).toHaveLength(1)
      const { options } = h.opens[0]!
      expect(options.eid.tunnelDomain).toBe('cable.ua5v.com')
      expect(options.secret).toBe(session.keys.secret)
      expect(options.identity.getPublicKey().length).toBe(65)
      expect(options.signal!.aborted).toBe(false)
      expect(h.statuses).toEqual(['connecting'])
    })

    it('success: confirm on the phone, the CTAP request goes out, the credential ends the session', async () => {
      const { h, sessions, answer } = await connecting()
      const tunnel = fakeTunnel(() => Promise.resolve(assertion()))
      h.opens[0]!.resolve(tunnel)
      await vi.advanceTimersByTimeAsync(0)
      expect(h.statuses).toEqual(['connecting', 'confirm'])
      expect(tunnel.transact).toHaveBeenCalledTimes(1)
      const sent = tunnel.transact.mock.calls[0]![0]
      expect(sent[0]).toBe(0x02)
      const result = await answer
      expect(result).toMatchObject({
        credential: { id: Buffer.alloc(8, 1).toString('base64url'), type: 'public-key', authenticatorAttachment: 'cross-platform' }
      })
      expect((result as { credential: { response: { clientDataJSON: string } } }).credential.response.clientDataJSON).toBe(
        Buffer.from(clientDataJson('get', challenge, `https://${rpId}`)).toString('base64url')
      )
      // The window closes, the tunnel is closed, nothing is left running.
      expect(h.closed).toEqual([true])
      expect(tunnel.close).toHaveBeenCalled()
      expect(sessions.prompt()).toBeNull()
      expect(vi.getTimerCount()).toBe(0)
      expect(h.scans[0]!.stopped).toBeGreaterThanOrEqual(1)
    })

    it('a registration works the same way', async () => {
      const req = {
        type: 'create' as const,
        rpId,
        origin: `https://${rpId}`,
        publicKey: {
          challenge: challenge.toString('base64url'),
          rp: { name: 'Example' },
          user: { id: 'AQID', name: 'a', displayName: 'A' },
          pubKeyCredParams: [{ type: 'public-key', alg: -7 }]
        }
      }
      const { h, answer } = await connecting(req)
      const tunnel = fakeTunnel(() => Promise.resolve(Buffer.of(0x19)))
      h.opens[0]!.resolve(tunnel)
      await vi.advanceTimersByTimeAsync(0)
      expect(tunnel.transact.mock.calls[0]![0][0]).toBe(0x01)
      await vi.advanceTimersByTimeAsync(FAILED_DELAY)
      await expect(answer).resolves.toMatchObject({ error: { name: 'InvalidStateError' } })
    })

    it('a tunnel that cannot be opened: failed for 3 s, then NotAllowedError', async () => {
      const { h, sessions, answer } = await connecting()
      h.opens[0]!.reject(new Error('boom'))
      await vi.advanceTimersByTimeAsync(0)
      expect(h.statuses).toEqual(['connecting', 'failed'])
      expect(sessions.prompt()).toMatchObject({ status: 'failed' })
      await vi.advanceTimersByTimeAsync(FAILED_DELAY - 1)
      expect(sessions.prompt()).not.toBeNull()
      expect(h.closed).toEqual([false])
      await vi.advanceTimersByTimeAsync(1)
      await expect(answer).resolves.toEqual(NOT_ALLOWED)
      expect(h.closed).toEqual([true])
      expect(vi.getTimerCount()).toBe(0)
    })

    it('CTAP errors: excluded credential -> InvalidStateError, others -> NotAllowedError', async () => {
      for (const [status, name] of [
        [0x19, 'InvalidStateError'],
        [0x2e, 'NotAllowedError'],
        [0x27, 'NotAllowedError']
      ] as const) {
        const { h, answer } = await connecting()
        h.opens[0]!.resolve(fakeTunnel(() => Promise.resolve(Buffer.of(status))))
        await vi.advanceTimersByTimeAsync(0)
        expect(h.statuses).toEqual(['connecting', 'confirm', 'failed'])
        await vi.advanceTimersByTimeAsync(FAILED_DELAY)
        await expect(answer).resolves.toMatchObject({ error: { name } })
      }
    })

    it('an invalid response (rpIdHash of another site) fails', async () => {
      const { h, answer } = await connecting()
      h.opens[0]!.resolve(fakeTunnel(() => Promise.resolve(assertion(authenticatorData('evil.example')))))
      await vi.advanceTimersByTimeAsync(FAILED_DELAY)
      await expect(answer).resolves.toEqual(NOT_ALLOWED)
      expect(h.statuses).toEqual(['connecting', 'confirm', 'failed'])
    })

    it('a tunnel that fails while waiting for the phone fails the session', async () => {
      const { h, answer } = await connecting()
      const tunnel = fakeTunnel(() => Promise.reject(new Error('closed')))
      h.opens[0]!.resolve(tunnel)
      await vi.advanceTimersByTimeAsync(FAILED_DELAY)
      await expect(answer).resolves.toEqual(NOT_ALLOWED)
      expect(tunnel.close).toHaveBeenCalled()
    })

    it('a request the phone cannot be sent (no challenge) fails', async () => {
      const { h, answer } = await connecting(request({}))
      const tunnel = fakeTunnel(() => Promise.resolve(Buffer.of(0)))
      h.opens[0]!.resolve(tunnel)
      await vi.advanceTimersByTimeAsync(FAILED_DELAY)
      await expect(answer).resolves.toEqual(NOT_ALLOWED)
      expect(tunnel.transact).not.toHaveBeenCalled()
    })

    it('cancel while connecting aborts the tunnel and closes it if it opens late', async () => {
      const { h, sessions, answer } = await connecting()
      sessions.cancel()
      await expect(answer).resolves.toEqual(NOT_ALLOWED)
      expect(h.opens[0]!.options.signal!.aborted).toBe(true)
      const tunnel = fakeTunnel(() => Promise.resolve(assertion()))
      h.opens[0]!.resolve(tunnel)
      await vi.advanceTimersByTimeAsync(0)
      expect(tunnel.transact).not.toHaveBeenCalled()
      expect(tunnel.close).toHaveBeenCalled()
      expect(h.statuses).toEqual(['connecting'])
    })

    it('cancel while confirming closes the tunnel and aborts the pending request', async () => {
      const { h, sessions, answer } = await connecting()
      let signal: AbortSignal | undefined
      const tunnel = fakeTunnel((_req, s) => {
        signal = s
        return new Promise(() => {})
      })
      h.opens[0]!.resolve(tunnel)
      await vi.advanceTimersByTimeAsync(0)
      expect(h.statuses).toEqual(['connecting', 'confirm'])
      sessions.cancel()
      await expect(answer).resolves.toEqual(NOT_ALLOWED)
      expect(tunnel.close).toHaveBeenCalledTimes(1)
      expect(signal!.aborted).toBe(true)
      expect(h.statuses).toEqual(['connecting', 'confirm'])
    })

    it('cancel while "failed" is shown ends at once', async () => {
      const { h, sessions, answer } = await connecting()
      h.opens[0]!.reject(new Error('boom'))
      await vi.advanceTimersByTimeAsync(1000)
      sessions.cancel()
      await expect(answer).resolves.toEqual(NOT_ALLOWED)
      expect(vi.getTimerCount()).toBe(0)
    })

    it('the session timeout ends a request waiting for the phone', async () => {
      const req = getRequest()
      req.publicKey.timeout = 30_000
      const { h, answer } = await connecting(req)
      const tunnel = fakeTunnel(() => new Promise(() => {}))
      h.opens[0]!.resolve(tunnel)
      await vi.advanceTimersByTimeAsync(30_000)
      await expect(answer).resolves.toEqual(NOT_ALLOWED)
      expect(tunnel.close).toHaveBeenCalled()
      expect(vi.getTimerCount()).toBe(0)
    })

    it('logs stages without keys or data', async () => {
      const { h, answer, session } = await connecting()
      h.opens[0]!.options.log?.('tunnel: x')
      h.opens[0]!.resolve(fakeTunnel(() => Promise.resolve(Buffer.of(0x2e))))
      await vi.advanceTimersByTimeAsync(FAILED_DELAY)
      await answer
      const text = h.logs.join('\n')
      expect(text).toContain('sending getAssertion')
      expect(text).toContain('failed: CTAP status 0x2e')
      expect(text).not.toContain(session.keys.secret.toString('hex'))
      expect(text).not.toContain(challenge.toString('hex'))
    })
  })
})

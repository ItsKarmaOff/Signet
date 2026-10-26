import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { WebAuthnRequest } from '../webauthn-request'
import { createCipheriv, createHmac } from 'node:crypto'
import { eidKeys } from './eid'
import { HybridSessions, hybridTimeout, type HybridHost, type HybridTarget, type ScanState } from './session'

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
    found: (e) => found.push(e)
  }
  return { host, closed, onClosed, gone, unwatched, scans, statuses, found }
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

    it('keeps the decrypted advert, stops the scan and shows the phone as found', async () => {
      const { h, sessions, session } = await open()
      h.scans[0]!.onData(advertFor(Buffer.alloc(16, 9)))
      h.scans[0]!.onData(advertFor(session.keys.secret, 1))
      expect(session.eid?.tunnelDomain).toBe('cable.auth.com')
      expect(session.eid?.routingId.toString('hex')).toBe('aabbcc')
      expect(h.scans[0]!.stopped).toBe(1)
      expect(h.statuses).toEqual(['found'])
      expect(h.found).toHaveLength(1)
      expect(sessions.prompt()).toMatchObject({ status: 'found' })
      // The session stays open, and later adverts or scan states change nothing.
      h.scans[0]!.onData(advertFor(session.keys.secret, 0))
      h.scans[0]!.onState('error')
      expect(session.eid?.tunnelDomain).toBe('cable.auth.com')
      expect(sessions.prompt()).toMatchObject({ status: 'found' })
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
})

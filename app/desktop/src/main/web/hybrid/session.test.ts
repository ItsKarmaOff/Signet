import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { WebAuthnRequest } from '../webauthn-request'
import { HybridSessions, hybridTimeout, type HybridHost, type HybridTarget } from './session'

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
    }
  }
  return { host, closed, onClosed, gone, unwatched }
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
})

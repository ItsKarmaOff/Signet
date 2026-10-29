import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { AUTH_EVENT } from '@shared/extension-apis'
import { ProxyAuthBroker } from './auth'
import type { ApiContext } from './types'

const A = 'a'.repeat(32)
const challenge = { url: 'https://example.com/', host: 'proxy.example', port: 89, scheme: 'basic', realm: 'r' }
const creds = { authCredentials: { username: 'u', password: 'p' } }

beforeEach(() => vi.useFakeTimers())
afterEach(() => vi.useRealTimers())

/** A context; `gone()` simulates its page closing or its worker stopping. */
function context(kind: 'worker' | 'page', id: string) {
  let goneFn = () => {}
  const ctx: ApiContext = { kind, id, url: `chrome-extension://${A}/${id}.html`, onGone: (fn) => (goneFn = fn) }
  return { ctx, gone: () => goneFn() }
}

function make(responder: string | null = A) {
  const emit = vi.fn()
  const log = vi.fn()
  const broker = new ProxyAuthBroker({ emitter: { emit }, responder: () => responder ?? undefined, log }, 30_000, 3, 60_000)
  const lastKey = () => emit.mock.calls.at(-1)![2][0] as string
  return { broker, emit, log, lastKey }
}

describe('ProxyAuthBroker', () => {
  it('only takes over proxy challenges when an extension can answer', () => {
    expect(make().broker.canHandle(true)).toBe(true)
    expect(make().broker.canHandle(false)).toBe(false)
    expect(make(null).broker.canHandle(true)).toBe(false)
  })

  it('asks the extension (waking its worker, no queueing) and returns its credentials', async () => {
    const { broker, emit, lastKey } = make()
    const worker = context('worker', 'worker-1')
    broker.setListeners(A, worker.ctx, 1)
    const result = broker.handle(challenge)
    expect(emit).toHaveBeenCalledWith(
      A,
      AUTH_EVENT,
      [expect.any(String), expect.objectContaining({ isProxy: true, challenger: { host: 'proxy.example', port: 89 } })],
      true,
      false
    )
    broker.respond(A, worker.ctx, lastKey(), creds)
    expect(await result).toEqual({ action: 'credentials', username: 'u', password: 'p' })
  })

  it('worker and offscreen page: an empty answer first does not cancel, the credentials of the other win', async () => {
    const { broker, lastKey, log } = make()
    const worker = context('worker', 'worker-1')
    const page = context('page', 'page-7')
    broker.setListeners(A, worker.ctx, 1)
    broker.setListeners(A, page.ctx, 1)
    const result = broker.handle(challenge)
    broker.respond(A, page.ctx, lastKey(), {}) // the offscreen document knows nothing
    let settled = false
    void result.then(() => (settled = true))
    await vi.advanceTimersByTimeAsync(0)
    expect(settled).toBe(false)
    broker.respond(A, worker.ctx, lastKey(), creds)
    expect(await result).toEqual({ action: 'credentials', username: 'u', password: 'p' })
    expect(log.mock.calls.map((c) => c[0]).join('\n')).toMatch(/answered by page .*: empty[\s\S]*answered by worker .*: credentials supplied/)
  })

  it('cancels once everybody answered without credentials, saying why', async () => {
    const { broker, lastKey } = make()
    const worker = context('worker', 'worker-1')
    const page = context('page', 'page-7')
    broker.setListeners(A, worker.ctx, 1)
    broker.setListeners(A, page.ctx, 1)
    const r1 = broker.handle(challenge)
    broker.respond(A, worker.ctx, lastKey(), { cancel: true })
    broker.respond(A, page.ctx, lastKey(), {})
    expect(await r1).toEqual({ action: 'cancel', reason: 'extension cancelled' })
    const r2 = broker.handle({ ...challenge, port: 90 })
    broker.respond(A, worker.ctx, lastKey(), {})
    broker.respond(A, page.ctx, lastKey(), undefined)
    expect(await r2).toEqual({ action: 'cancel', reason: 'no listener supplied credentials' })
  })

  it('a context that goes away stops being waited for', async () => {
    const { broker, lastKey } = make()
    const worker = context('worker', 'worker-1')
    const popup = context('page', 'page-9')
    broker.setListeners(A, worker.ctx, 1)
    broker.setListeners(A, popup.ctx, 1)
    const result = broker.handle(challenge)
    broker.respond(A, worker.ctx, lastKey(), {})
    popup.gone()
    expect((await result).action).toBe('cancel')
  })

  it('no worker known: a woken worker answering satisfies the placeholder', async () => {
    const { broker, lastKey } = make()
    const result = broker.handle(challenge)
    broker.respond(A, context('worker', 'worker-5').ctx, lastKey(), {})
    expect(await result).toEqual({ action: 'cancel', reason: 'no listener supplied credentials' })
  })

  it('silence times out; another extension cannot answer', async () => {
    const { broker, lastKey } = make()
    const worker = context('worker', 'worker-1')
    broker.setListeners(A, worker.ctx, 1)
    const result = broker.handle(challenge)
    broker.respond('b'.repeat(32), worker.ctx, lastKey(), creds)
    await vi.advanceTimersByTimeAsync(30_000)
    expect(await result).toEqual({ action: 'cancel', reason: 'no answer' })
  })

  it('stops after repeated challenges (bad credentials), and recovers after the window', async () => {
    const { broker, lastKey } = make()
    const worker = context('worker', 'worker-1')
    broker.setListeners(A, worker.ctx, 1)
    for (let i = 0; i < 3; i++) {
      const r = broker.handle(challenge)
      broker.respond(A, worker.ctx, lastKey(), creds)
      expect((await r).action).toBe('credentials')
    }
    expect(await broker.handle(challenge)).toEqual({ action: 'cancel', reason: 'too many attempts' })
    await vi.advanceTimersByTimeAsync(61_000)
    const r = broker.handle(challenge)
    broker.respond(A, worker.ctx, lastKey(), creds)
    expect((await r).action).toBe('credentials')
  })

  it('skips when nobody controls the proxy', async () => {
    expect(await make(null).broker.handle(challenge)).toEqual({ action: 'skip' })
  })
})

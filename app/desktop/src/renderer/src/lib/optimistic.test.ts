import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// `useOptimistic` goes through useSyncExternalStore: we replace it to capture subscription and read without React rendering.
const store = vi.hoisted(() => ({ subscribe: null as ((fn: () => void) => () => void) | null, get: null as (() => unknown) | null }))
vi.mock('react', () => ({
  useSyncExternalStore: (sub: (fn: () => void) => () => void, get: () => unknown) => {
    store.subscribe = sub
    store.get = get
    return get()
  }
}))

type Mod = typeof import('./optimistic')
let mod: Mod
let notified: number
let off: () => void
const read = () => store.get!() as { tabId?: string; spaceId?: string }

/** Promise resolved on demand, to simulate the main process's response. */
function deferred() {
  let resolve!: () => void
  let reject!: (e: unknown) => void
  const promise = new Promise<void>((res, rej) => {
    resolve = res
    reject = rej
  })
  return { promise, resolve, reject }
}
const flush = () => vi.advanceTimersByTimeAsync(0)

beforeEach(async () => {
  vi.useFakeTimers()
  vi.resetModules()
  mod = await import('./optimistic')
  mod.useOptimistic() // capte subscribe/get
  notified = 0
  off = store.subscribe!(() => notified++)
})
afterEach(() => {
  off()
  vi.useRealTimers()
  vi.unstubAllGlobals()
})

function stubSignet(api: Record<string, unknown>) {
  vi.stubGlobal('window', { signet: api })
}

describe('optimistic selection', () => {
  it('is empty at the start', () => {
    expect(read()).toEqual({})
  })

  it('activateTab affiche l’onglet tout de suite', () => {
    stubSignet({ activateTab: () => deferred().promise })
    mod.activateTab('t1')
    expect(read()).toEqual({ tabId: 't1' })
    expect(notified).toBe(1)
  })

  it('drops at the first state received after the response from main', async () => {
    const d = deferred()
    stubSignet({ activateTab: () => d.promise })
    mod.activateTab('t1')
    mod.settleOptimistic() // state received before the response: we keep it
    expect(read()).toEqual({ tabId: 't1' })
    d.resolve()
    await flush()
    mod.settleOptimistic()
    expect(read()).toEqual({})
  })

  it('drops after 400 ms if no state arrives', async () => {
    const d = deferred()
    stubSignet({ activateTab: () => d.promise })
    mod.activateTab('t1')
    d.resolve()
    await flush()
    await vi.advanceTimersByTimeAsync(399)
    expect(read()).toEqual({ tabId: 't1' })
    await vi.advanceTimersByTimeAsync(2)
    expect(read()).toEqual({})
  })

  it('drops right away if the command fails', async () => {
    const d = deferred()
    stubSignet({ activateTab: () => d.promise })
    mod.activateTab('t1')
    d.reject(new Error('refus'))
    await flush()
    expect(read()).toEqual({})
  })

  it('switchSpace forgets the tab choice', () => {
    stubSignet({ activateTab: () => deferred().promise, switchSpace: () => deferred().promise })
    mod.activateTab('t1')
    mod.switchSpace('s2')
    expect(read()).toEqual({ spaceId: 's2', tabId: undefined })
  })

  it('a more recent command silences the previous one', async () => {
    const first = deferred()
    stubSignet({ activateTab: vi.fn().mockReturnValueOnce(first.promise).mockReturnValue(deferred().promise) })
    mod.activateTab('t1')
    mod.activateTab('t2')
    first.resolve()
    await flush()
    await vi.advanceTimersByTimeAsync(1000)
    expect(read()).toEqual({ tabId: 't2' }) // the delay of t1 removed nothing
  })

  it('settleOptimistic with nothing pending does not notify', () => {
    mod.settleOptimistic()
    expect(notified).toBe(0)
  })
})

describe('adoptPendingSpace', () => {
  it('sets the space chosen elsewhere and expires at the next state', () => {
    mod.adoptPendingSpace('s3')
    expect(read()).toEqual({ spaceId: 's3', tabId: undefined })
    mod.settleOptimistic()
    expect(read()).toEqual({})
  })

  it('drops after 1 s if the main process refused', async () => {
    mod.adoptPendingSpace('s3')
    await vi.advanceTimersByTimeAsync(999)
    expect(read().spaceId).toBe('s3')
    await vi.advanceTimersByTimeAsync(2)
    expect(read()).toEqual({})
  })
})

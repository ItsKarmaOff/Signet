import { beforeEach, describe, expect, it, vi } from 'vitest'

const electron = vi.hoisted(() => ({
  handlers: new Map<string, (e: unknown, ...args: unknown[]) => unknown>(),
  contents: [] as unknown[]
}))
vi.mock('electron', () => ({
  app: { isPackaged: true },
  ipcMain: { handle: (channel: string, fn: (e: unknown, ...args: unknown[]) => unknown) => electron.handlers.set(channel, fn) },
  webContents: { getAllWebContents: () => electron.contents }
}))

import type { Session } from 'electron'
import { CRX_EVENT, CRX_INVOKE } from '@shared/extension-apis'
import { ApiRouter, extensionOfPage, extensionOfScope, hasPermission } from './router'

const A = 'a'.repeat(32)
const B = 'b'.repeat(32)

function fakeSession(manifests: Record<string, { permissions?: string[] }>) {
  const status: ((d: { versionId: number; runningStatus: string }) => void)[] = []
  const workers = new Map<number, { scope: string; send: ReturnType<typeof vi.fn>; ipc: { handle: ReturnType<typeof vi.fn> } }>()
  const started = vi.fn()
  const ses = {
    extensions: {
      getExtension: (id: string) => (manifests[id] ? { id, manifest: manifests[id] } : null),
      getAllExtensions: () => Object.entries(manifests).map(([id, manifest]) => ({ id, manifest }))
    },
    serviceWorkers: {
      on: (name: string, fn: (typeof status)[number]) => name === 'running-status-changed' && status.push(fn),
      getAllRunning: () => ({}),
      getWorkerFromVersionID: (v: number) => workers.get(v),
      startWorkerForScope: (scope: string) => {
        started(scope)
        const worker = { scope, send: vi.fn(), ipc: { handle: vi.fn() } }
        return Promise.resolve(worker)
      }
    }
  } as unknown as Session
  const run = (versionId: number, scope: string) => {
    const worker = { scope, send: vi.fn(), ipc: { handle: vi.fn() } }
    workers.set(versionId, worker)
    status.forEach((fn) => fn({ versionId, runningStatus: 'running' }))
    return worker
  }
  return { ses, run, started }
}

beforeEach(() => {
  electron.handlers.clear()
  electron.contents.length = 0
})

describe('helpers', () => {
  it('extracts ids only from well-formed extension URLs', () => {
    expect(extensionOfScope(`chrome-extension://${A}/`)).toBe(A)
    expect(extensionOfScope(`chrome-extension://${A}/sub/`)).toBeNull()
    expect(extensionOfScope('https://example.com/')).toBeNull()
    expect(extensionOfPage(`chrome-extension://${A}/popup.html`)).toBe(A)
    expect(extensionOfPage(`https://evil.com/chrome-extension://${A}/`)).toBeNull()
    expect(extensionOfPage(undefined)).toBeNull()
  })
  it('hasPermission reads the manifest permissions', () => {
    expect(hasPermission({ permissions: ['alarms'] }, 'alarms')).toBe(true)
    expect(hasPermission({ host_permissions: ['alarms'] }, 'alarms')).toBe(false)
    expect(hasPermission(undefined, 'alarms')).toBe(false)
  })
})

describe('ApiRouter.dispatch', () => {
  it('runs the handler with the sender extension and checks the permission', async () => {
    const router = new ApiRouter()
    const fn = vi.fn((c: { extensionId: string }, x: unknown) => `${c.extensionId}:${String(x)}`)
    router.register('alarms', 'alarms.get', fn)
    const { ses } = fakeSession({ [A]: { permissions: ['alarms'] }, [B]: { permissions: ['storage'] } })
    router.attach(ses)
    expect(await router.dispatch(ses, A, 'alarms.get', ['n'])).toEqual({ ok: true, value: `${A}:n` })
    const denied = await router.dispatch(ses, B, 'alarms.get', ['n'])
    expect(denied).toMatchObject({ ok: false })
    expect((denied as { error: string }).error).toMatch(/denied/)
    expect(fn).toHaveBeenCalledOnce()
  })

  it('accepts any of several permissions, and none when the list is empty', async () => {
    const router = new ApiRouter()
    router.register(['declarativeNetRequest', 'declarativeNetRequestWithHostAccess'], 'dnr.x', () => 'dnr')
    router.register('', 'open.x', () => 'open')
    const { ses } = fakeSession({ [A]: { permissions: ['declarativeNetRequestWithHostAccess'] }, [B]: { permissions: [] } })
    router.attach(ses)
    expect(await router.dispatch(ses, A, 'dnr.x', [])).toEqual({ ok: true, value: 'dnr' })
    expect(await router.dispatch(ses, B, 'dnr.x', [])).toMatchObject({ ok: false })
    expect(await router.dispatch(ses, B, 'open.x', [])).toEqual({ ok: true, value: 'open' })
  })

  it('refuses unknown methods, unloaded extensions, bad args and unattached sessions', async () => {
    const router = new ApiRouter()
    router.register('alarms', 'alarms.get', () => 1)
    const { ses } = fakeSession({ [A]: { permissions: ['alarms'] } })
    router.attach(ses)
    expect(await router.dispatch(ses, A, 'nope', [])).toMatchObject({ ok: false })
    expect(await router.dispatch(ses, B, 'alarms.get', [])).toMatchObject({ ok: false })
    expect(await router.dispatch(ses, A, 'alarms.get', 'x')).toMatchObject({ ok: false })
    expect(await router.dispatch(fakeSession({}).ses, A, 'alarms.get', [])).toMatchObject({ ok: false })
  })

  it('turns a handler error into data', async () => {
    const router = new ApiRouter()
    router.register('alarms', 'alarms.get', () => {
      throw new Error('boom')
    })
    const { ses } = fakeSession({ [A]: { permissions: ['alarms'] } })
    router.attach(ses)
    expect(await router.dispatch(ses, A, 'alarms.get', [])).toMatchObject({ ok: false })
  })
})

describe('sender checks', () => {
  function setup() {
    const router = new ApiRouter()
    router.register('alarms', 'alarms.get', ({ extensionId }) => extensionId)
    const { ses, run } = fakeSession({ [A]: { permissions: ['alarms'] }, [B]: { permissions: ['alarms'] } })
    router.attach(ses)
    router.registerPageIpc()
    return { router, ses, run, call: electron.handlers.get(CRX_INVOKE)! }
  }
  const event = (ses: Session, url: string, main = true) => {
    const frame = { url }
    return { senderFrame: frame, sender: { session: ses, mainFrame: main ? frame : {} } }
  }

  it('page: the extension id comes from the main frame URL', async () => {
    const { ses, call } = setup()
    expect(await call(event(ses, `chrome-extension://${B}/popup.html`), 'alarms.get', [])).toEqual({ ok: true, value: B })
  })

  it('page: refuses web pages, subframes and missing frames', async () => {
    const { ses, call } = setup()
    expect(await call(event(ses, 'https://example.com/'), 'alarms.get', [])).toMatchObject({ ok: false })
    expect(await call(event(ses, `chrome-extension://${A}/p.html`, false), 'alarms.get', [])).toMatchObject({ ok: false })
    expect(await call({ senderFrame: null, sender: { session: ses } }, 'alarms.get', [])).toMatchObject({ ok: false })
  })

  it('worker: handler bound to the id of the worker scope, one registration per worker', async () => {
    const { run } = setup()
    const worker = run(1, `chrome-extension://${A}/`)
    expect(worker.ipc.handle).toHaveBeenCalledOnce()
    const [channel, handler] = worker.ipc.handle.mock.calls[0]
    expect(channel).toBe(CRX_INVOKE)
    expect(await handler({}, 'alarms.get', [])).toEqual({ ok: true, value: A })
  })

  it('worker: ignores non-extension workers', () => {
    const { run } = setup()
    expect(run(2, 'https://example.com/').ipc.handle).not.toHaveBeenCalled()
  })
})

describe('ApiRouter.emit', () => {
  it('sends to a running worker, wakes a stopped one only when asked, and reaches open pages', async () => {
    const router = new ApiRouter()
    const { ses, run, started } = fakeSession({ [A]: { permissions: ['alarms'] } })
    router.attach(ses)
    const page = { isDestroyed: () => false, session: ses, getURL: () => `chrome-extension://${A}/popup.html`, send: vi.fn() }
    const other = { isDestroyed: () => false, session: ses, getURL: () => 'https://example.com/', send: vi.fn() }
    electron.contents.push(page, other)

    router.emit(A, 'alarms.onAlarm', [{ name: 'x' }], false)
    expect(started).not.toHaveBeenCalled()
    expect(page.send).toHaveBeenCalledWith(CRX_EVENT, 'alarms.onAlarm', [{ name: 'x' }])
    expect(other.send).not.toHaveBeenCalled()

    router.emit(A, 'alarms.onAlarm', [{ name: 'x' }], true)
    expect(started).toHaveBeenCalledWith(`chrome-extension://${A}/`)

    const worker = run(7, `chrome-extension://${A}/`)
    started.mockClear()
    router.emit(A, 'alarms.onAlarm', [1], true)
    expect(worker.send).toHaveBeenCalledWith(CRX_EVENT, 'alarms.onAlarm', [1])
    expect(started).not.toHaveBeenCalled()
  })

  it('does nothing for an extension that is not loaded', () => {
    const router = new ApiRouter()
    const { ses, started } = fakeSession({})
    router.attach(ses)
    router.emit(A, 'x', [], true)
    expect(started).not.toHaveBeenCalled()
  })
})

describe('queued events', () => {
  it('keeps an event when the worker will not start, retries with backoff, and delivers once it runs', async () => {
    vi.useFakeTimers()
    try {
      const router = new ApiRouter()
      const { ses, run, started } = fakeSession({ [A]: { permissions: ['alarms'] } })
      // The worker refuses to start twice, then comes up by itself.
      const start = vi.fn().mockRejectedValue(new Error('Failed to start service worker.'))
      ;(ses.serviceWorkers as unknown as { startWorkerForScope: unknown }).startWorkerForScope = start
      router.attach(ses)
      router.emit(A, 'alarms.onAlarm', [{ name: 'x' }], true)
      await vi.advanceTimersByTimeAsync(0)
      expect(start).toHaveBeenCalledTimes(1)
      await vi.advanceTimersByTimeAsync(1000)
      expect(start).toHaveBeenCalledTimes(2)
      expect(started).not.toHaveBeenCalled()
      // It runs by itself: the queued event reaches it after the settle delay.
      const worker = run(3, `chrome-extension://${A}/`)
      expect(worker.send).not.toHaveBeenCalled()
      await vi.advanceTimersByTimeAsync(300)
      expect(worker.send).toHaveBeenCalledWith(CRX_EVENT, 'alarms.onAlarm', [{ name: 'x' }])
      // Delivered once only.
      await vi.advanceTimersByTimeAsync(10_000)
      expect(worker.send).toHaveBeenCalledOnce()
    } finally {
      vi.useRealTimers()
    }
  })

  it('does not queue time-sensitive events', async () => {
    vi.useFakeTimers()
    try {
      const router = new ApiRouter()
      const { ses } = fakeSession({ [A]: { permissions: ['alarms'] } })
      const start = vi.fn().mockRejectedValue(new Error('no'))
      ;(ses.serviceWorkers as unknown as { startWorkerForScope: unknown }).startWorkerForScope = start
      router.attach(ses)
      router.emit(A, 'webRequest.onAuthRequired', [], true, false)
      await vi.advanceTimersByTimeAsync(10_000)
      expect(start).toHaveBeenCalledTimes(1)
    } finally {
      vi.useRealTimers()
    }
  })
})

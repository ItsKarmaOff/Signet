import { EventEmitter } from 'node:events'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const appEvents = new EventEmitter()
vi.mock('electron', () => ({
  app: { on: (e: string, f: (...a: unknown[]) => void) => appEvents.on(e, f), quit: vi.fn(), getPath: () => '' }
}))
vi.mock('../app/uilog', () => ({ logUiError: vi.fn() }))
vi.mock('../core/urls', () => ({ isAppUrl: (u: string) => u.startsWith('signetui://app/') || u.startsWith('http://localhost:5174/') }))

import { BOOT_WATCHDOG_MS, guardUiViews, tabContents } from './guard'
import { logUiError } from '../app/uilog'

const APP = 'signetui://app/index.html'

class FakeWc extends EventEmitter {
  url = APP
  destroyed = false
  loading = false
  boot: unknown = undefined
  reject = false
  reload = vi.fn()
  getURL = () => this.url
  isDestroyed = () => this.destroyed
  isLoading = () => this.loading
  executeJavaScript = vi.fn(async () => {
    if (this.reject) throw new Error('gone')
    return this.boot
  })
  load() {
    this.emit('did-start-loading')
    this.emit('did-finish-load')
  }
}

const make = () => {
  const wc = new FakeWc()
  appEvents.emit('web-contents-created', {}, wc)
  return wc
}
const wait = (ms: number) => vi.advanceTimersByTimeAsync(ms)

describe('interface boot watchdog', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    appEvents.removeAllListeners()
    vi.mocked(logUiError).mockClear()
    guardUiViews()
  })
  afterEach(() => vi.useRealTimers())

  it('reloads a view whose boot marker is missing', async () => {
    const wc = make()
    wc.load()
    await wait(BOOT_WATCHDOG_MS - 1)
    expect(wc.executeJavaScript).not.toHaveBeenCalled()
    await wait(1)
    expect(logUiError).toHaveBeenCalledWith('main', 'script never ran', 'rechargement')
    await wait(1000)
    expect(wc.reload).toHaveBeenCalledTimes(1)
  })

  it('reloads a view stuck on "started"', async () => {
    const wc = make()
    wc.boot = 'started'
    wc.load()
    await wait(BOOT_WATCHDOG_MS + 1000)
    expect(logUiError).toHaveBeenCalledWith('main', 'boot stuck', 'rechargement')
    expect(wc.reload).toHaveBeenCalledTimes(1)
  })

  it('does nothing when ready or when the renderer retries by itself', async () => {
    const wc = make()
    for (const boot of ['ready', 'retrying']) {
      wc.boot = boot
      wc.load()
      await wait(BOOT_WATCHDOG_MS + 60_000)
    }
    expect(wc.reload).not.toHaveBeenCalled()
    expect(logUiError).not.toHaveBeenCalled()
  })

  it('logs once per outage and backs off', async () => {
    const wc = make()
    wc.load()
    await wait(BOOT_WATCHDOG_MS + 1000)
    expect(wc.reload).toHaveBeenCalledTimes(1)
    wc.load()
    await wait(BOOT_WATCHDOG_MS + 1000)
    expect(wc.reload).toHaveBeenCalledTimes(1) // second delay is 2 s
    await wait(1000)
    expect(wc.reload).toHaveBeenCalledTimes(2)
    expect(logUiError).toHaveBeenCalledTimes(1)
    // Recovery: ready resets the outage, the next one is logged again.
    wc.boot = 'ready'
    wc.load()
    await wait(BOOT_WATCHDOG_MS + 1)
    wc.boot = undefined
    wc.load()
    await wait(BOOT_WATCHDOG_MS + 1)
    expect(logUiError).toHaveBeenCalledTimes(2)
  })

  it('is cancelled by a new load, by a destroy', async () => {
    const wc = make()
    wc.load()
    await wait(BOOT_WATCHDOG_MS - 1000)
    wc.emit('did-start-loading')
    await wait(BOOT_WATCHDOG_MS)
    expect(wc.executeJavaScript).not.toHaveBeenCalled()
    wc.load()
    wc.destroyed = true
    wc.emit('destroyed')
    await wait(BOOT_WATCHDOG_MS * 2)
    expect(wc.executeJavaScript).not.toHaveBeenCalled()
    expect(wc.reload).not.toHaveBeenCalled()
  })

  it('ignores non-app URLs, navigation in between and script errors', async () => {
    const web = make()
    web.url = 'https://example.com/'
    web.load()
    await wait(BOOT_WATCHDOG_MS * 2)
    expect(web.executeJavaScript).not.toHaveBeenCalled()

    const moved = make()
    moved.load()
    moved.url = `${APP}#find`
    await wait(BOOT_WATCHDOG_MS * 2)
    expect(moved.reload).not.toHaveBeenCalled()

    const broken = make()
    broken.reject = true
    broken.load()
    await wait(BOOT_WATCHDOG_MS * 2)
    expect(broken.reload).not.toHaveBeenCalled()
  })

  it('does not touch tab views', async () => {
    const wc = make()
    tabContents.add(wc as never)
    wc.url = 'https://example.com/'
    wc.load()
    await wait(BOOT_WATCHDOG_MS * 2)
    expect(wc.reload).not.toHaveBeenCalled()
  })

  it('no longer reloads once the dev session ended', async () => {
    vi.stubEnv('ELECTRON_RENDERER_URL', 'http://localhost:5174')
    const wc = make()
    wc.url = 'http://localhost:5174/'
    wc.emit('did-fail-load', {}, -102, 'refused', 'http://localhost:5174/', true)
    const other = make()
    other.load()
    await wait(BOOT_WATCHDOG_MS + 60_000)
    expect(other.reload).not.toHaveBeenCalled()
    vi.unstubAllEnvs()
  })
})

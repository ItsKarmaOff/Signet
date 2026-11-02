import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { Session } from 'electron'
import { HANDLER_TIMEOUT_MS, NetworkHub, compilePattern, networkHub } from './network'

/** A session whose `webRequest` keeps the last listener of each event, like Electron. */
function fakeSession() {
  const listeners: Record<string, ((...a: never[]) => void) | null> = {}
  const calls: string[] = []
  const webRequest = new Proxy(
    {},
    {
      get: (_t, event: string) => (a: unknown, b?: (...x: never[]) => void) => {
        calls.push(a === null ? `${event}:null` : `${event}:${JSON.stringify((a as { urls: string[] }).urls)}`)
        listeners[event] = a === null ? null : (b ?? null)
      }
    }
  )
  return { ses: { webRequest } as unknown as Session, listeners, calls }
}

const req = (url = 'https://example.com/a', extra: object = {}) => ({ url, resourceType: 'script', ...extra }) as never

/** Runs the real blocking listener and returns what it answered. */
function run(l: ((...a: never[]) => void) | null, details: object): Promise<Record<string, unknown>> {
  return new Promise((resolve) => (l as (d: object, cb: (r: Record<string, unknown>) => void) => void)(details, resolve))
}

describe('compilePattern', () => {
  const t = (p: string, u: string) => compilePattern(p)!(u)
  it('handles <all_urls>', () => {
    expect(t('<all_urls>', 'https://a.com/x')).toBe(true)
    expect(t('<all_urls>', 'http://a.com/')).toBe(true)
  })
  it('scheme * is http and https only', () => {
    expect(t('*://a.com/*', 'http://a.com/x')).toBe(true)
    expect(t('*://a.com/*', 'https://a.com/')).toBe(true)
    expect(t('*://a.com/*', 'ftp://a.com/')).toBe(false)
    expect(t('https://a.com/*', 'http://a.com/')).toBe(false)
  })
  it('matches hosts and wildcard subdomains', () => {
    expect(t('*://*.example.com/*', 'https://example.com/')).toBe(true)
    expect(t('*://*.example.com/*', 'https://a.b.example.com/x')).toBe(true)
    expect(t('*://*.example.com/*', 'https://badexample.com/')).toBe(false)
    expect(t('*://example.com/*', 'https://www.example.com/')).toBe(false)
    expect(t('http://*/*', 'http://anything.dev/z')).toBe(true)
  })
  it('matches path globs including the query', () => {
    expect(t('*://a.com/ads/*.js', 'https://a.com/ads/x.js')).toBe(true)
    expect(t('*://a.com/ads/*.js', 'https://a.com/ads/x.css')).toBe(false)
    expect(t('*://a.com/p?id=*', 'https://a.com/p?id=3')).toBe(true)
    expect(t('*://a.com/', 'https://a.com/other')).toBe(false)
  })
  it('handles ports and rejects invalid patterns', () => {
    expect(t('http://localhost:3000/*', 'http://localhost:3000/a')).toBe(true)
    expect(t('http://localhost:3000/*', 'http://localhost:4000/a')).toBe(false)
    expect(compilePattern('nonsense')).toBeNull()
  })
})

describe('NetworkHub', () => {
  let s: ReturnType<typeof fakeSession>
  let hub: NetworkHub
  beforeEach(() => {
    s = fakeSession()
    hub = new NetworkHub(s.ses)
  })
  afterEach(() => vi.useRealTimers())

  it('installs the real listener only while a consumer exists', () => {
    expect(s.calls).toEqual([])
    const off = hub.on('onCompleted', 'a', 1, undefined, vi.fn())
    hub.on('onCompleted', 'b', 2, undefined, vi.fn())
    expect(s.calls).toEqual(['onCompleted:["<all_urls>"]'])
    off()
    expect(hub.has('onCompleted')).toBe(true)
    hub.off('onCompleted', 'b')
    expect(s.calls).toEqual(['onCompleted:["<all_urls>"]', 'onCompleted:null'])
    expect(hub.has('onCompleted')).toBe(false)
  })

  it('runs blocking handlers in order, not registration order', async () => {
    const seen: string[] = []
    hub.on('onBeforeRequest', 'late', 200, undefined, (_d, cb) => (seen.push('late'), cb({})))
    hub.on('onBeforeRequest', 'early', 100, undefined, (_d, cb) => (seen.push('early'), cb({})))
    expect(await run(s.listeners.onBeforeRequest, req())).toEqual({})
    expect(seen).toEqual(['early', 'late'])
  })

  it('stops the chain on cancel or redirect', async () => {
    const after = vi.fn((_d: unknown, cb: (r: object) => void) => cb({}))
    hub.on('onBeforeRequest', 'a', 1, undefined, (_d, cb) => cb({ cancel: true }))
    hub.on('onBeforeRequest', 'b', 2, undefined, after as never)
    expect(await run(s.listeners.onBeforeRequest, req())).toEqual({ cancel: true })
    hub.on('onBeforeRequest', 'a', 1, undefined, (_d, cb) => cb({ redirectURL: 'https://x.test/' }))
    expect(await run(s.listeners.onBeforeRequest, req())).toEqual({ redirectURL: 'https://x.test/' })
    expect(after).not.toHaveBeenCalled()
  })

  it('hands modified headers to the next handler and returns them', async () => {
    let seenByB: unknown
    hub.on('onBeforeSendHeaders', 'a', 1, undefined, (d, cb) => cb({ requestHeaders: { ...d.requestHeaders, 'Sec-GPC': '1' } }))
    hub.on('onBeforeSendHeaders', 'b', 2, undefined, (d, cb) => ((seenByB = d.requestHeaders), cb({})))
    const out = await run(s.listeners.onBeforeSendHeaders, req('https://a.com/', { requestHeaders: { A: 'x' } }))
    expect(seenByB).toEqual({ A: 'x', 'Sec-GPC': '1' })
    expect(out).toEqual({ requestHeaders: { A: 'x', 'Sec-GPC': '1' } })
  })

  it('applies each consumer filter', async () => {
    const h = vi.fn((_d: unknown, cb: (r: object) => void) => cb({ cancel: true }))
    hub.on('onBeforeRequest', 'a', 1, { urls: ['*://ads.test/*'], types: ['script'] }, h as never)
    expect(await run(s.listeners.onBeforeRequest, req('https://other.test/'))).toEqual({})
    expect(await run(s.listeners.onBeforeRequest, req('https://ads.test/x', { resourceType: 'image' }))).toEqual({})
    expect(await run(s.listeners.onBeforeRequest, req('https://ads.test/x'))).toEqual({ cancel: true })
  })

  it('survives a handler that throws', async () => {
    const err = vi.spyOn(console, 'error').mockImplementation(() => {})
    hub.on('onBeforeRequest', 'bad', 1, undefined, () => {
      throw new Error('boom')
    })
    hub.on('onBeforeRequest', 'ok', 2, undefined, (_d, cb) => cb({ cancel: true }))
    expect(await run(s.listeners.onBeforeRequest, req())).toEqual({ cancel: true })
    await run(s.listeners.onBeforeRequest, req())
    expect(err).toHaveBeenCalledTimes(1) // logged once
    err.mockRestore()
  })

  it('moves on when a handler never answers', async () => {
    vi.useFakeTimers()
    const err = vi.spyOn(console, 'error').mockImplementation(() => {})
    hub.on('onHeadersReceived', 'stuck', 1, undefined, () => {})
    hub.on('onHeadersReceived', 'ok', 2, undefined, (_d, cb) => cb({ responseHeaders: { A: ['1'] } }))
    const p = run(s.listeners.onHeadersReceived, req('https://a.com/', { responseHeaders: {} }))
    await vi.advanceTimersByTimeAsync(HANDLER_TIMEOUT_MS)
    expect(await p).toEqual({ responseHeaders: { A: ['1'] } })
    err.mockRestore()
  })

  it('ignores a second answer from the same handler', async () => {
    hub.on('onBeforeRequest', 'twice', 1, undefined, (_d, cb) => (cb({}), cb({ cancel: true })))
    expect(await run(s.listeners.onBeforeRequest, req())).toEqual({})
  })

  it('fans observers out, isolating failures', () => {
    const err = vi.spyOn(console, 'error').mockImplementation(() => {})
    const ok = vi.fn()
    hub.on('onErrorOccurred', 'bad', 1, undefined, () => {
      throw new Error('x')
    })
    hub.on('onErrorOccurred', 'ok', 2, { urls: ['https://*/*'] }, ok)
    ;(s.listeners.onErrorOccurred as (d: object) => void)(req())
    ;(s.listeners.onErrorOccurred as (d: object) => void)(req('http://plain.test/'))
    expect(ok).toHaveBeenCalledTimes(1)
    err.mockRestore()
  })

  it('offers an Electron-style shim (Ghostery)', async () => {
    const wr = hub.consumer('adblock', 100)
    const listener = vi.fn((_d: unknown, cb: (r: object) => void) => cb({ cancel: true }))
    wr.onBeforeRequest({ urls: ['<all_urls>'] }, listener as never)
    expect(await run(s.listeners.onBeforeRequest, req())).toEqual({ cancel: true })
    wr.onHeadersReceived({ urls: ['<all_urls>'] }, ((_d: unknown, cb: (r: object) => void) => cb({})) as never)
    wr.onHeadersReceived(null)
    wr.onBeforeRequest(null)
    expect(hub.has('onBeforeRequest')).toBe(false)
    expect(s.listeners.onBeforeRequest).toBeNull()
    expect(s.listeners.onHeadersReceived).toBeNull()
  })

  it('release removes everything a consumer registered', () => {
    hub.on('onCompleted', 'dev', 1, undefined, vi.fn())
    hub.on('onErrorOccurred', 'dev', 1, undefined, vi.fn())
    hub.release('dev')
    expect(s.listeners.onCompleted).toBeNull()
    expect(s.listeners.onErrorOccurred).toBeNull()
  })

  it('networkHub returns one hub per session', () => {
    expect(networkHub(s.ses)).toBe(networkHub(s.ses))
    expect(networkHub(fakeSession().ses)).not.toBe(networkHub(s.ses))
  })
})

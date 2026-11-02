import { describe, expect, it } from 'vitest'
import type { Session } from 'electron'
import { CONTINUE_TITLE, HttpsOnly } from './https-only'

const make = (httpsOnly = true, devAliases: { name: string; target: string }[] = []) => {
  const settings = { httpsOnly, devAliases }
  const h = new HttpsOnly({ store: { state: { settings } } as never, tabOf: (id) => (id === 7 ? 't' : null) })
  return Object.assign(h, { settings })
}
const ses = () => ({}) as Session

/** A session whose `webRequest` keeps the last listener of each event, like Electron. */
function fakeSession() {
  const listeners: Record<string, ((...a: never[]) => void) | null> = {}
  const webRequest = new Proxy(
    {},
    { get: (_t, event: string) => (a: unknown, b?: (...x: never[]) => void) => void (listeners[event] = a === null ? null : (b ?? null)) }
  )
  return { ses: { webRequest } as unknown as Session, listeners }
}

/** Runs the real onBeforeRequest listener and returns what it answered. */
function request(listener: ((...a: never[]) => void) | null, url: string, extra: object = {}): Promise<Record<string, unknown>> {
  const details = { url, resourceType: 'mainFrame', method: 'GET', webContentsId: 7, ...extra }
  return new Promise((resolve) => (listener as (d: object, cb: (r: Record<string, unknown>) => void) => void)(details, resolve))
}

describe('HttpsOnly.resolve', () => {
  it('does nothing when the mode is off', () => {
    expect(make(false).resolve('t', ses(), 'http://example.com/')).toBe('http://example.com/')
  })
  it('upgrades http and leaves exempt hosts and local names', () => {
    const h = make(true, [{ name: 'site', target: 'http://site.test:7000' }])
    const s = ses()
    expect(h.resolve('t', s, 'http://example.com/a?b#c')).toBe('https://example.com/a?b#c')
    expect(h.resolve('t', s, 'http://localhost:3000/')).toBe('http://localhost:3000/')
    expect(h.resolve('t', s, 'http://site.test:7000/x')).toBe('http://site.test:7000/x')
    expect(h.resolve('t', s, 'https://example.com/')).toBe('https://example.com/')
  })
  it('stops upgrading a site that keeps redirecting back to http', () => {
    const h = make()
    const s = ses()
    const results = [1, 2, 3, 4].map(() => h.resolve('t', s, 'http://loop.example.com/'))
    expect(results.slice(0, 3)).toEqual(Array(3).fill('https://loop.example.com/'))
    expect(results[3]).toBe('http://loop.example.com/')
  })
})

describe('HttpsOnly warning', () => {
  it('a failed upgraded load shows the warning; a DNS error or another host does not', () => {
    const h = make()
    h.resolve('t', ses(), 'http://example.com/a')
    expect(h.insecureFailure('t', 'https://example.com/a', -102)).toBe('http://example.com/a')
    expect(h.insecureFailure('t', 'https://example.com/a', -200)).toBe('http://example.com/a')
    expect(h.insecureFailure('t', 'https://example.com/a', -105)).toBeNull()
    expect(h.insecureFailure('t', 'https://other.com/a', -102)).toBeNull()
    expect(h.insecureFailure('other-tab', 'https://example.com/a', -102)).toBeNull()
  })
  it('a typed address promoted to https counts as an upgrade', () => {
    const h = make()
    h.noteTyped('t', 'https://example.com/')
    expect(h.insecureFailure('t', 'https://example.com/', -107)).toBe('http://example.com/')
  })
  it('no warning when the mode is off', () => {
    const h = make(false)
    h.noteTyped('t', 'https://example.com/')
    expect(h.insecureFailure('t', 'https://example.com/', -102)).toBeNull()
  })
  it('continue needs the right token, allows the host for that session only, once', () => {
    const h = make()
    const a = ses()
    const b = ses()
    h.resolve('t', a, 'http://example.com/a')
    const token = h.warn('t', 'http://example.com/a')
    expect(h.continueFrom('t', a, CONTINUE_TITLE + 'forged')).toBeNull()
    expect(h.continueFrom('other', a, CONTINUE_TITLE + token)).toBeNull()
    expect(h.continueFrom('t', a, CONTINUE_TITLE + token)).toBe('http://example.com/a')
    expect(h.continueFrom('t', a, CONTINUE_TITLE + token)).toBeNull()
    expect(h.resolve('t', a, 'http://example.com/a')).toBe('http://example.com/a')
    expect(h.resolve('t', a, 'http://example.com/other')).toBe('http://example.com/other')
    expect(h.resolve('t', b, 'http://example.com/a')).toBe('https://example.com/a') // another session (private window)
  })
})

describe('HttpsOnly request upgrade', () => {
  it('redirects a POST main-frame request to https (the redirect keeps method and body)', async () => {
    const h = make()
    const s = fakeSession()
    h.attach(s.ses)
    const out = await request(s.listeners.onBeforeRequest, 'http://example.com/submit?a=1', { method: 'POST' })
    expect(out).toEqual({ redirectURL: 'https://example.com/submit?a=1' })
  })
  it('leaves frames and subresources alone', async () => {
    const h = make()
    const s = fakeSession()
    h.attach(s.ses)
    for (const resourceType of ['subFrame', 'image', 'script', 'stylesheet', 'xhr', 'media']) {
      expect(await request(s.listeners.onBeforeRequest, 'http://example.com/a', { resourceType })).toEqual({})
    }
    expect(await request(s.listeners.onBeforeRequest, 'https://example.com/')).toEqual({})
  })
  it('leaves exempt hosts, local names and allowed hosts alone', async () => {
    const h = make(true, [{ name: 'site', target: 'http://site.test:7000' }])
    const s = fakeSession()
    h.attach(s.ses)
    const go = (url: string) => request(s.listeners.onBeforeRequest, url, { method: 'POST' })
    expect(await go('http://localhost:3000/')).toEqual({})
    expect(await go('http://192.168.1.2/')).toEqual({})
    expect(await go('http://site.test:7000/x')).toEqual({})
    h.allow(s.ses, 'http://example.com/')
    expect(await go('http://example.com/a')).toEqual({})
    expect(await go('http://other.example.com/a')).toEqual({ redirectURL: 'https://other.example.com/a' })
  })
  it('stops upgrading a site that keeps redirecting back to http', async () => {
    const h = make()
    const s = fakeSession()
    h.attach(s.ses)
    const out = []
    for (let i = 0; i < 4; i++) out.push(await request(s.listeners.onBeforeRequest, 'http://loop.example.com/'))
    expect(out.slice(0, 3)).toEqual(Array(3).fill({ redirectURL: 'https://loop.example.com/' }))
    expect(out[3]).toEqual({})
  })
  it('ignores a main frame that is not a tab', async () => {
    const h = make()
    const s = fakeSession()
    h.attach(s.ses)
    expect(await request(s.listeners.onBeforeRequest, 'http://example.com/', { webContentsId: 99 })).toEqual({})
    expect(await request(s.listeners.onBeforeRequest, 'http://example.com/', { webContentsId: undefined })).toEqual({})
  })
  it('is registered only while the setting is on, on every session', () => {
    const h = make(false)
    const a = fakeSession()
    const b = fakeSession()
    h.attach(a.ses)
    h.attach(b.ses)
    expect(a.listeners.onBeforeRequest ?? null).toBeNull()
    h.settings.httpsOnly = true
    h.sync()
    expect(a.listeners.onBeforeRequest).toBeTypeOf('function')
    expect(b.listeners.onBeforeRequest).toBeTypeOf('function')
    h.settings.httpsOnly = false
    h.sync()
    expect(a.listeners.onBeforeRequest).toBeNull()
    expect(b.listeners.onBeforeRequest).toBeNull()
    h.settings.httpsOnly = true
    h.sync()
    h.detach(b.ses)
    expect(b.listeners.onBeforeRequest).toBeNull()
    expect(a.listeners.onBeforeRequest).toBeTypeOf('function')
  })
  it('registers on a session attached while the setting is already on', () => {
    const h = make()
    const s = fakeSession()
    h.attach(s.ses)
    expect(s.listeners.onBeforeRequest).toBeTypeOf('function')
  })
  it('a failed upgraded load still shows the warning, with the continue token', async () => {
    const h = make()
    const s = fakeSession()
    h.attach(s.ses)
    await request(s.listeners.onBeforeRequest, 'http://example.com/a', { method: 'POST' })
    expect(h.insecureFailure('t', 'https://example.com/a', -102)).toBe('http://example.com/a')
    expect(h.insecureFailure('t', 'http://example.com/a', -102)).toBe('http://example.com/a')
    const token = h.warn('t', 'http://example.com/a')
    expect(h.continueFrom('t', s.ses, CONTINUE_TITLE + token)).toBe('http://example.com/a')
    expect(await request(s.listeners.onBeforeRequest, 'http://example.com/a')).toEqual({})
  })
})

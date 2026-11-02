import { describe, expect, it, vi } from 'vitest'
import type { Session } from 'electron'
import { Gpc } from './gpc'

vi.mock('../core/urls', () => ({ gpcPath: '/preload/gpc.js' }))

const make = (on = true) => {
  const settings: { globalPrivacyControl?: boolean } = on ? { globalPrivacyControl: true } : {}
  const g = new Gpc({ store: { state: { settings } } as never })
  return Object.assign(g, { settings })
}

/** A session whose `webRequest` keeps the last listener of each event, like Electron, and records preload scripts. */
function fakeSession() {
  const listeners: Record<string, ((...a: never[]) => void) | null> = {}
  const webRequest = new Proxy(
    {},
    { get: (_t, event: string) => (a: unknown, b?: (...x: never[]) => void) => void (listeners[event] = a === null ? null : (b ?? null)) }
  )
  const scripts = new Set<string>()
  const ses = {
    webRequest,
    registerPreloadScript: (o: { id: string }) => void scripts.add(o.id),
    unregisterPreloadScript: (id: string) => void scripts.delete(id)
  } as unknown as Session
  return { ses, listeners, scripts }
}

function send(listener: ((...a: never[]) => void) | null, url: string, requestHeaders: Record<string, string> = {}): Promise<Record<string, unknown>> {
  const details = { url, resourceType: 'mainFrame', method: 'GET', requestHeaders }
  return new Promise((resolve) => (listener as (d: object, cb: (r: Record<string, unknown>) => void) => void)(details, resolve))
}

describe('Gpc', () => {
  it('adds Sec-GPC: 1 and keeps the existing headers', async () => {
    const g = make()
    const s = fakeSession()
    g.attach(s.ses)
    const out = await send(s.listeners.onBeforeSendHeaders, 'https://example.com/a', { Accept: '*/*', Cookie: 'a=b' })
    expect(out).toEqual({ requestHeaders: { Accept: '*/*', Cookie: 'a=b', 'Sec-GPC': '1' } })
    expect(await send(s.listeners.onBeforeSendHeaders, 'http://example.com/', {})).toEqual({ requestHeaders: { 'Sec-GPC': '1' } })
  })
  it('leaves other schemes alone', async () => {
    const g = make()
    const s = fakeSession()
    g.attach(s.ses)
    expect(await send(s.listeners.onBeforeSendHeaders, 'ftp://example.com/', {})).toEqual({})
    expect(await send(s.listeners.onBeforeSendHeaders, 'file:///tmp/a', {})).toEqual({})
  })
  it('registers nothing while the setting is off', () => {
    const g = make(false)
    const s = fakeSession()
    g.attach(s.ses)
    expect(s.listeners.onBeforeSendHeaders ?? null).toBeNull()
    expect(s.scripts.size).toBe(0)
  })
  it('follows the setting on every session, without restart', () => {
    const g = make(false)
    const a = fakeSession()
    const b = fakeSession()
    g.attach(a.ses)
    g.attach(b.ses)
    g.settings.globalPrivacyControl = true
    g.sync()
    for (const s of [a, b]) {
      expect(s.listeners.onBeforeSendHeaders).toBeTypeOf('function')
      expect([...s.scripts]).toEqual(['signet-gpc'])
    }
    g.sync() // idempotent
    expect(a.scripts.size).toBe(1)
    delete g.settings.globalPrivacyControl
    g.sync()
    for (const s of [a, b]) {
      expect(s.listeners.onBeforeSendHeaders).toBeNull()
      expect(s.scripts.size).toBe(0)
    }
  })
  it('covers a session attached later (private window) and releases it on detach', () => {
    const g = make()
    const priv = fakeSession()
    g.attach(priv.ses)
    expect(priv.listeners.onBeforeSendHeaders).toBeTypeOf('function')
    expect(priv.scripts.has('signet-gpc')).toBe(true)
    g.detach(priv.ses)
    expect(priv.listeners.onBeforeSendHeaders).toBeNull()
  })
})

import { describe, expect, it, vi } from 'vitest'

vi.mock('electron', () => ({ app: { isPackaged: false }, ipcMain: { handle: vi.fn() } }))

import { answerWebAuthn } from './webauthn'
import { isWebAuthnError, validateWebAuthnRequest, type WebAuthnError, type WebAuthnRequest } from './webauthn-request'

const get = { type: 'get', publicKey: { challenge: 'abc' } }
const create = (rp: Record<string, unknown> = { name: 'x' }) => ({
  type: 'create',
  publicKey: { challenge: 'abc', rp, user: { id: 'u', name: 'n', displayName: 'n' }, pubKeyCredParams: [{ type: 'public-key', alg: -7 }] }
})
const run = (origin: string | undefined, payload: unknown, extra: { topOrigin?: string; isMainFrame?: boolean } = {}) =>
  validateWebAuthnRequest({ origin, topOrigin: extra.topOrigin ?? origin, isMainFrame: extra.isMainFrame ?? true, payload })
const getWith = (rpId: unknown) => ({ type: 'get', publicKey: { challenge: 'abc', rpId } })
const ok = (r: WebAuthnRequest | WebAuthnError): WebAuthnRequest => {
  expect(isWebAuthnError(r)).toBe(false)
  return r as WebAuthnRequest
}
const err = (r: WebAuthnRequest | WebAuthnError, name: string): void => {
  expect(isWebAuthnError(r) ? r.name : 'ok').toBe(name)
}

describe('origin', () => {
  it('accepts https and defaults the rpId to the host', () => {
    const r = ok(run('https://Login.Example.com', get))
    expect(r.rpId).toBe('login.example.com')
    expect(r.origin).toBe('https://Login.Example.com')
  })
  it('accepts http on localhost only', () => {
    ok(run('http://localhost:3000', get))
    ok(run('http://app.localhost', get))
    err(run('http://example.com', get), 'SecurityError')
  })
  it('refuses IP hosts and other schemes', () => {
    err(run('https://192.168.1.2', get), 'SecurityError')
    err(run('https://[::1]:8443', get), 'SecurityError')
    err(run('ftp://example.com', get), 'SecurityError')
  })
  it('refuses a missing or opaque origin', () => {
    err(run(undefined, get), 'NotAllowedError')
    err(run('null', get), 'NotAllowedError')
    err(run('', get), 'NotAllowedError')
  })
})

describe('rpId', () => {
  const host = 'https://login.microsoftonline.com'
  it('accepts the host and its registrable parent', () => {
    expect(ok(run(host, getWith('login.microsoftonline.com'))).rpId).toBe('login.microsoftonline.com')
    expect(ok(run(host, getWith('MicrosoftOnline.com'))).rpId).toBe('microsoftonline.com')
    expect(ok(run('https://foo.github.io', getWith('foo.github.io'))).rpId).toBe('foo.github.io')
    ok(run('http://localhost', getWith('localhost')))
  })
  it('reads rp.id for create', () => {
    expect(ok(run(host, create({ id: 'microsoftonline.com', name: 'x' }))).rpId).toBe('microsoftonline.com')
    expect(ok(run(host, create())).rpId).toBe('login.microsoftonline.com')
    err(run(host, create({ id: 'evil.com', name: 'x' })), 'SecurityError')
  })
  it('refuses public suffixes', () => {
    err(run('https://example.com', getWith('com')), 'SecurityError')
    err(run('https://example.co.uk', getWith('co.uk')), 'SecurityError')
    err(run('https://foo.github.io', getWith('github.io')), 'SecurityError')
  })
  it('refuses unrelated domains and partial labels', () => {
    err(run(host, getWith('example.com')), 'SecurityError')
    err(run('https://evil-example.com', getWith('example.com')), 'SecurityError')
    err(run('https://example.com', getWith('sub.example.com')), 'SecurityError')
    err(run('https://example.com', getWith('')), 'SecurityError')
  })
  it('refuses a non-string rpId', () => {
    err(run(host, getWith(42)), 'TypeError')
  })
})

describe('frames', () => {
  it('refuses a cross-origin subframe', () => {
    err(run('https://pay.example.com', get, { topOrigin: 'https://example.com', isMainFrame: false }), 'NotAllowedError')
  })
  it('accepts a same-origin subframe', () => {
    ok(run('https://example.com', get, { topOrigin: 'https://example.com', isMainFrame: false }))
  })
})

describe('malformed payloads', () => {
  const o = 'https://example.com'
  it.each([
    ['not an object', 'x'],
    ['null', null],
    ['an array', []],
    ['unknown type', { type: 'delete', publicKey: { challenge: 'a' } }],
    ['no publicKey', { type: 'get' }],
    ['publicKey not an object', { type: 'get', publicKey: 'x' }],
    ['no challenge', { type: 'get', publicKey: {} }],
    ['challenge not a string', { type: 'get', publicKey: { challenge: 1 } }],
    ['create without rp', { type: 'create', publicKey: { challenge: 'a', user: { id: 'u' }, pubKeyCredParams: [] } }],
    ['create without user.id', { type: 'create', publicKey: { challenge: 'a', rp: {}, user: {}, pubKeyCredParams: [] } }],
    ['create without user', { type: 'create', publicKey: { challenge: 'a', rp: {}, pubKeyCredParams: [] } }],
    ['create with pubKeyCredParams not an array', { type: 'create', publicKey: { challenge: 'a', rp: {}, user: { id: 'u' }, pubKeyCredParams: {} } }]
  ])('%s gives a TypeError', (_label, payload) => {
    err(run(o, payload), 'TypeError')
  })
})

describe('answerWebAuthn', () => {
  const notAllowed = {
    error: {
      name: 'NotAllowedError',
      message: 'The operation either timed out or was not allowed. See: https://www.w3.org/TR/webauthn-2/#sctn-privacy-considerations-client.'
    }
  }
  const sender = { id: 7 } as never
  const frame = { processId: 1, routingId: 2 } as never

  it('hands a request to the hybrid session', async () => {
    const request = vi.fn().mockResolvedValue(notAllowed)
    const b = { passkeys: { request } } as never
    const r = ok(run('https://example.com', get))
    expect(await answerWebAuthn(b, r, sender, frame)).toEqual(notAllowed)
    expect(request).toHaveBeenCalledWith(r, sender, frame)
  })
  it('refuses a conditional request without a session', async () => {
    const request = vi.fn()
    const b = { passkeys: { request } } as never
    const r = ok(run('https://example.com', { type: 'get', mediation: 'conditional', publicKey: { challenge: 'abc' } }))
    expect(await answerWebAuthn(b, r, sender, frame)).toEqual(notAllowed)
    expect(request).not.toHaveBeenCalled()
  })
})

import { describe, expect, it } from 'vitest'
import { base64UrlDecode, base64UrlDecodeText, base64UrlEncode, base64UrlEncodeText } from './base64url'
import { isLoopbackHost, isLoopbackUrl, loopbackPort, toLinkLocalhost } from './loopback'
import { decodePairingUrl, encodeLinkMessage, encodePairingUrl, linkIdFromServerName, parseLinkMessage, serverNameForLink, type PairingOffer } from './protocol'
import { linkStatus, pickLocalhostRoute } from './status'

const TOKEN = 'A'.repeat(43)
const CODE = 'b'.repeat(22)
const DESKTOP = { id: 'desk-1', name: 'MacBook de Christophe', platform: 'macos' } as const
const PHONE = { id: 'phone-1', name: 'iPhone', platform: 'ios' } as const

const offer = (extra: Partial<PairingOffer> = {}): PairingOffer => ({
  v: 1,
  desktop: DESKTOP,
  linkId: '0123456789abcdef',
  certSha256: TOKEN,
  code: CODE,
  hosts: ['192.168.1.20', 'fe80::1%en0'],
  port: 47615,
  expiresAt: 2_000,
  ...extra
})

describe('base64url', () => {
  it('encodes and decodes bytes', () => {
    for (const len of [0, 1, 2, 3, 4, 5, 32]) {
      const bytes = Uint8Array.from({ length: len }, (_, i) => (i * 37 + 250) & 0xff)
      expect(base64UrlDecode(base64UrlEncode(bytes))).toEqual(bytes)
    }
    expect(base64UrlEncode(Uint8Array.from([0xfb, 0xff]))).toBe('-_8')
  })

  it('keeps UTF-8 text (accents, emoji, kanji)', () => {
    const t = 'Ordinateur de Zoé 💻 澪'
    expect(base64UrlDecodeText(base64UrlEncodeText(t))).toBe(t)
  })

  it('rejects characters outside the alphabet', () => {
    expect(() => base64UrlDecode('ab+/')).toThrow()
    expect(() => base64UrlDecode('a')).toThrow()
  })
})

describe('QR code', () => {
  it('round-trips (only the computer name travels, expiry rounded to the second)', () => {
    const url = encodePairingUrl(offer({ expiresAt: 1_792_935_614_470 }))
    expect(url.startsWith('signet://link?d=1~')).toBe(true)
    expect(decodePairingUrl(url, 1_000)).toEqual({
      ok: true,
      offer: { ...offer(), desktop: { name: DESKTOP.name }, expiresAt: 1_792_935_615_000 }
    })
  })

  it('reste court (QR code peu dense)', () => {
    const url = encodePairingUrl(offer({ hosts: ['192.168.219.102'], expiresAt: 1_792_935_614_470 }))
    expect(url.length).toBeLessThan(180)
  })

  it('still reads the old format (JSON in base64url)', () => {
    const legacy = 'signet://link?d=' + base64UrlEncodeText(JSON.stringify(offer({ expiresAt: 5_000 })))
    expect(decodePairingUrl(legacy, 1_000)).toEqual({ ok: true, offer: { ...offer({ expiresAt: 5_000 }), desktop: { name: DESKTOP.name } } })
  })

  it('rejects an expired code, another version or a corrupted payload', () => {
    expect(decodePairingUrl(encodePairingUrl(offer()), 3_000)).toEqual({ ok: false, error: 'expired' })
    expect(decodePairingUrl(encodePairingUrl(offer({ v: 2 })), 0)).toEqual({ ok: false, error: 'version' })
    expect(decodePairingUrl(encodePairingUrl(offer({ code: 'court' })), 0)).toEqual({ ok: false, error: 'invalid' })
    expect(decodePairingUrl(encodePairingUrl(offer({ hosts: [] })), 0)).toEqual({ ok: false, error: 'invalid' })
    expect(decodePairingUrl('https://example.com', 0)).toEqual({ ok: false, error: 'invalid' })
    expect(decodePairingUrl('signet://link?d=%%%', 0)).toEqual({ ok: false, error: 'invalid' })
    expect(decodePairingUrl('signet://link?d=1~a~b', 0)).toEqual({ ok: false, error: 'invalid' })
  })
})

describe('messages', () => {
  it('reads the first lines sent by the phone', () => {
    const pair = { type: 'pair', v: 1, linkId: '0123456789abcdef', code: CODE, device: PHONE } as const
    expect(parseLinkMessage(encodeLinkMessage(pair))).toEqual(pair)
    const control = { type: 'hello', v: 1, linkId: '0123456789abcdef', token: TOKEN, role: 'control', device: PHONE } as const
    expect(parseLinkMessage(JSON.stringify(control))).toEqual(control)
    const tunnel = { type: 'hello', v: 1, linkId: '0123456789abcdef', token: TOKEN, role: 'tunnel', port: 5173 } as const
    expect(parseLinkMessage(JSON.stringify(tunnel))).toEqual(tunnel)
  })

  it('rejects malformed input', () => {
    expect(parseLinkMessage('pas du json')).toBeNull()
    expect(parseLinkMessage('[]')).toBeNull()
    expect(parseLinkMessage(JSON.stringify({ type: 'hello', v: 1, linkId: 'XYZ', token: TOKEN, role: 'tunnel', port: 1 }))).toBeNull()
    expect(parseLinkMessage(JSON.stringify({ type: 'hello', v: 1, linkId: '0123456789abcdef', token: TOKEN, role: 'tunnel', port: 70000 }))).toBeNull()
    expect(parseLinkMessage(JSON.stringify({ type: 'error', code: 'nope' }))).toBeNull()
    expect(parseLinkMessage(JSON.stringify({ type: 'open', url: 'javascript:alert(1)' }))).toBeNull()
    expect(parseLinkMessage(JSON.stringify({ type: 'inconnu' }))).toBeNull()
  })

  it('cleans names and the server list', () => {
    const w = parseLinkMessage(JSON.stringify({ type: 'welcome', paused: false, device: { ...DESKTOP, name: '  Mac\u0007 ' } }))
    expect(w).toEqual({ type: 'welcome', paused: false, device: { ...DESKTOP, name: 'Mac' } })
    const s = parseLinkMessage(
      JSON.stringify({
        type: 'servers',
        servers: [
          { url: 'http://localhost:5173/', port: 5173, title: ' Densho ', fromTab: true },
          { url: 'https://example.com/', port: 443 },
          { url: 'http://localhost:3000/', port: 'x' }
        ]
      })
    )
    expect(s).toEqual({ type: 'servers', servers: [{ url: 'http://localhost:5173/', port: 5173, title: 'Densho', fromTab: true }], ports: [5173] })
    const withPorts = parseLinkMessage(JSON.stringify({ type: 'servers', servers: [], ports: [5173, 3000, 3000, 0, 'x', 70000] }))
    expect(withPorts).toEqual({ type: 'servers', servers: [], ports: [3000, 5173] })
  })

  it('maps the SNI to the link', () => {
    expect(linkIdFromServerName(serverNameForLink('0123456789abcdef'))).toBe('0123456789abcdef')
    expect(linkIdFromServerName('example.com')).toBeNull()
    expect(linkIdFromServerName(undefined)).toBeNull()
  })
})

describe('localhost', () => {
  it('recognizes the machine itself', () => {
    for (const h of ['localhost', 'app.localhost', '127.0.0.1', '127.1.2.3', '[::1]', '0.0.0.0']) expect(isLoopbackHost(h)).toBe(true)
    for (const h of ['192.168.1.2', 'example.com', 'localhost.example.com']) expect(isLoopbackHost(h)).toBe(false)
    expect(isLoopbackUrl('ws://localhost:5173/')).toBe(true)
    expect(isLoopbackUrl('file:///tmp')).toBe(false)
  })

  it('maps local IPs to localhost', () => {
    expect(toLinkLocalhost('http://127.0.0.1:5173/a?b=1#c')).toBe('http://localhost:5173/a?b=1#c')
    expect(toLinkLocalhost('http://[::1]:3000/')).toBe('http://localhost:3000/')
    expect(toLinkLocalhost('http://localhost:3000/x')).toBe('http://localhost:3000/x')
    expect(toLinkLocalhost('https://example.com/')).toBe('https://example.com/')
  })

  it('gives the port', () => {
    expect(loopbackPort('http://localhost:5173/')).toBe(5173)
    expect(loopbackPort('http://localhost/')).toBe(80)
    expect(loopbackPort('https://localhost/')).toBe(443)
    expect(loopbackPort('https://example.com/')).toBeNull()
  })
})

describe('link state', () => {
  const base = { general: true, device: true, connected: true, peerPaused: false, invalid: false }

  it('only works if everything is ON', () => {
    expect(linkStatus(base)).toBe('connected')
    expect(linkStatus({ ...base, general: false })).toBe('paused-here')
    expect(linkStatus({ ...base, device: false })).toBe('paused-here')
    expect(linkStatus({ ...base, peerPaused: true })).toBe('paused-there')
    expect(linkStatus({ ...base, connected: false })).toBe('offline')
    expect(linkStatus({ ...base, connected: false, connecting: true })).toBe('connecting')
    expect(linkStatus({ ...base, invalid: true, general: false })).toBe('invalid')
  })

  it('picks the computer for localhost', () => {
    expect(pickLocalhostRoute([], null)).toBeNull()
    expect(pickLocalhostRoute(['a'], null)).toBe('a')
    expect(pickLocalhostRoute(['a', 'b'], null)).toBe('ask')
    expect(pickLocalhostRoute(['a', 'b'], 'b')).toBe('b')
    expect(pickLocalhostRoute(['a', 'b'], 'c')).toBe('ask')
  })
})

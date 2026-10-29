import { describe, expect, it } from 'vitest'
import { httpsUpgrade, isExemptHost, isInsecureSiteError, isUpgradeOf } from './https-only'

describe('httpsUpgrade', () => {
  it('upgrades http keeping host, path, query and fragment', () => {
    expect(httpsUpgrade('http://example.com/a/b?x=1&y=2#frag')).toBe('https://example.com/a/b?x=1&y=2#frag')
    expect(httpsUpgrade('http://example.com')).toBe('https://example.com/')
  })
  it('keeps an explicit port', () => {
    expect(httpsUpgrade('http://example.com:8080/x')).toBe('https://example.com:8080/x')
    expect(httpsUpgrade('http://example.com:80/x')).toBe('https://example.com/x')
  })
  it('leaves other schemes alone', () => {
    expect(httpsUpgrade('https://example.com')).toBeNull()
    expect(httpsUpgrade('file:///a.html')).toBeNull()
    expect(httpsUpgrade('signet://settings')).toBeNull()
    expect(httpsUpgrade('not a url')).toBeNull()
  })
  it('leaves exempt hosts alone', () => {
    for (const u of [
      'http://localhost:3000',
      'http://app.localhost',
      'http://127.0.0.1:8080',
      'http://192.168.1.2',
      'http://nas/',
      'http://printer.local/',
      'http://[::1]:3000/'
    ])
      expect(httpsUpgrade(u), u).toBeNull()
  })
  it('leaves allowed hosts alone, case-insensitively', () => {
    const allowedHosts = new Set(['old.example.com'])
    expect(httpsUpgrade('http://OLD.example.com/x', { allowedHosts })).toBeNull()
    expect(httpsUpgrade('http://other.example.com/x', { allowedHosts })).toBe('https://other.example.com/x')
  })
  it('leaves local name targets alone', () => {
    expect(httpsUpgrade('http://site.test:7000/x', { aliasTargets: ['http://site.test:7000'] })).toBeNull()
    expect(httpsUpgrade('http://site.test:7000/x')).toBe('https://site.test:7000/x')
  })
})

describe('isExemptHost', () => {
  it('loopback and private IPv4', () => {
    for (const h of ['127.0.0.1', '127.8.8.8', '10.0.0.5', '172.16.0.1', '172.31.255.1', '192.168.0.1', '169.254.10.1', '0.0.0.0'])
      expect(isExemptHost(h), h).toBe(true)
    for (const h of ['172.15.0.1', '172.32.0.1', '8.8.8.8', '192.169.0.1', '169.253.0.1', '11.0.0.1']) expect(isExemptHost(h), h).toBe(false)
  })
  it('IPv6', () => {
    for (const h of ['[::1]', '::1', '[fd12:3456::1]', '[fc00::1]', '[fe80::1]', '[febf::1]', '[::ffff:7f00:1]', '[::ffff:c0a8:101]'])
      expect(isExemptHost(h), h).toBe(true)
    for (const h of ['[2001:db8::1]', '[fec0::1]', '[::ffff:808:808]']) expect(isExemptHost(h), h).toBe(false)
  })
  it('names', () => {
    for (const h of ['localhost', 'a.localhost', 'nas.local', 'intranet', 'LOCALHOST']) expect(isExemptHost(h), h).toBe(true)
    for (const h of ['example.com', 'a.b.example.com', 'local.example.com', 'notlocalhost.com']) expect(isExemptHost(h), h).toBe(false)
  })
})

describe('isInsecureSiteError', () => {
  it('connection, TLS and timeout errors', () => {
    for (const c of [-102, -107, -113, -200, -201, -205, -118, -7, -100, -101, -324, -103]) expect(isInsecureSiteError(c), String(c)).toBe(true)
  })
  it('not a DNS error, offline or a canceled navigation', () => {
    for (const c of [-105, -137, -106, -3, -300]) expect(isInsecureSiteError(c), String(c)).toBe(false)
  })
})

describe('isUpgradeOf', () => {
  it('same host over https', () => {
    expect(isUpgradeOf('http://a.com/x', 'https://a.com/x')).toBe(true)
    expect(isUpgradeOf('http://a.com/x', 'https://A.com:8443/y')).toBe(true)
    expect(isUpgradeOf('http://a.com/x', 'https://b.com/x')).toBe(false)
    expect(isUpgradeOf('http://a.com/x', 'http://a.com/x')).toBe(false)
  })
})

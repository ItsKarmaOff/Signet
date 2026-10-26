import { describe, expect, it } from 'vitest'
import { BLOCKED_DOMAINS, COSMETIC_CSS, cosmeticScript, hostOf, isBlockedUrl } from './content-blocking'

describe('hostOf', () => {
  it('reads the host of http(s) addresses', () => {
    expect(hostOf('https://Ads.Example.com/a?b=1')).toBe('ads.example.com')
    expect(hostOf('http://example.com:8080/x')).toBe('example.com')
    expect(hostOf('https://user:pw@example.com/')).toBe('example.com')
    expect(hostOf('https://example.com./')).toBe('example.com')
  })
  it('returns null for other schemes', () => {
    expect(hostOf('about:blank')).toBeNull()
    expect(hostOf('data:text/html,hi')).toBeNull()
    expect(hostOf('')).toBeNull()
  })
})

describe('isBlockedUrl', () => {
  it('blocks the domain and its subdomains', () => {
    expect(isBlockedUrl('https://doubleclick.net/ad')).toBe(true)
    expect(isBlockedUrl('https://stats.g.doubleclick.net/collect')).toBe(true)
    expect(isBlockedUrl('http://WWW.Taboola.com/')).toBe(true)
  })
  it('does not block domains that merely end the same way', () => {
    expect(isBlockedUrl('https://notdoubleclick.net/')).toBe(false)
    expect(isBlockedUrl('https://doubleclick.net.example.org/')).toBe(false)
  })
  it('lets ordinary sites, internal pages and non-web schemes through', () => {
    expect(isBlockedUrl('https://www.wikipedia.org/')).toBe(false)
    expect(isBlockedUrl('about:blank')).toBe(false)
    expect(isBlockedUrl('tel:+33123456789')).toBe(false)
  })
  it('accepts a custom list', () => {
    expect(isBlockedUrl('https://x.example.com/', ['example.com'])).toBe(true)
  })
  it('the list only contains lowercase domains, no duplicates', () => {
    expect(new Set(BLOCKED_DOMAINS).size).toBe(BLOCKED_DOMAINS.length)
    for (const d of BLOCKED_DOMAINS) expect(d).toMatch(/^[a-z0-9.-]+\.[a-z]+$/)
  })
})

describe('cosmeticScript', () => {
  it('embeds the escaped stylesheet', () => {
    const js = cosmeticScript()
    expect(js).toContain(JSON.stringify(COSMETIC_CSS))
    expect(js.endsWith('true;')).toBe(true)
  })
})

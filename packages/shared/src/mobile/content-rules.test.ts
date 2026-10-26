import { describe, expect, it } from 'vitest'
import { convertFilterList, isSafeSelector, patternToRegex } from './content-rules'

const one = (line: string) => convertFilterList(line).rules

describe('patternToRegex', () => {
  it('anchors a domain with ||', () => {
    expect(patternToRegex('||ads.example.com^')).toBe('^[^:]+://+([^/]+\\.)?ads\\.example\\.com[^a-zA-Z0-9_.%-]')
  })
  it('handles |, trailing |, * and ^', () => {
    expect(patternToRegex('|https://x.com/a|')).toBe('^https://x\\.com/a$')
    expect(patternToRegex('/banner/*/img')).toBe('/banner/.*/img')
  })
  it('rejects regular expressions, non-ASCII and patterns that are too short', () => {
    expect(patternToRegex('/ad[sz]+/')).toBeNull()
    expect(patternToRegex('||é.com^')).toBeNull()
    expect(patternToRegex('ad')).toBeNull()
    expect(patternToRegex('*')).toBeNull()
  })
})

describe('network rules', () => {
  it('blocks a third-party domain by type', () => {
    expect(one('||tracker.net^$third-party,script,image')).toEqual([
      {
        trigger: {
          'url-filter': '^[^:]+://+([^/]+\\.)?tracker\\.net[^a-zA-Z0-9_.%-]',
          'resource-type': ['script', 'image', 'svg-document'],
          'load-type': ['third-party']
        },
        action: { type: 'block' }
      }
    ])
  })
  it('translates domain= and ~domain', () => {
    const [r] = one('/ads/banner$domain=a.com|b.com')
    expect(r.trigger['if-domain']).toEqual(['*a.com', '*b.com'])
    const [n] = one('/ads/banner$domain=~a.com')
    expect(n.trigger['unless-domain']).toEqual(['*a.com'])
  })
  it('excludes negated types', () => {
    const [r] = one('/pixel.gif$~script')
    expect(r.trigger['resource-type']).not.toContain('script')
    expect(r.trigger['resource-type']).toContain('image')
  })
  it('puts @@ exceptions last with ignore-previous-rules', () => {
    const { rules, stats } = convertFilterList('@@||good.com/ads.js\n||evil.com^\n')
    expect(rules.map((r) => r.action.type)).toEqual(['block', 'ignore-previous-rules'])
    expect(stats).toMatchObject({ network: 1, exceptions: 1 })
  })
  it('ignores what is inexpressible', () => {
    for (const l of ['||a.com^$csp=script-src', '||a.com^$removeparam=x', '/regex.+/', '||a.com^$subdocument', '||a.com^$redirect=noop.js'])
      expect(one(l)).toEqual([])
  })
  it('accepts subdocument only with third-party', () => {
    expect(one('||a.com^$subdocument,third-party')[0].trigger['resource-type']).toEqual(['document'])
  })
  it('deduplicates', () => {
    expect(one('||a.com^\n||a.com^')).toHaveLength(1)
  })
  it('ignores comments and headers', () => {
    expect(convertFilterList('[Adblock Plus 2.0]\n! Title\n\n').stats.lines).toBe(0)
  })
})

describe('cosmetic rules', () => {
  it('groups generic selectors', () => {
    const { rules } = convertFilterList('##.ad\n###banner\n')
    expect(rules).toEqual([{ trigger: { 'url-filter': '.*' }, action: { type: 'css-display-none', selector: '.ad, #banner' } }])
  })
  it('groups by domain', () => {
    const { rules } = convertFilterList('a.com,b.com##.x\na.com##.y\n')
    expect(rules).toHaveLength(2)
    expect(rules[0]).toEqual({ trigger: { 'url-filter': '.*', 'if-domain': ['*a.com'] }, action: { type: 'css-display-none', selector: '.x, .y' } })
  })
  it('ignores domains that are only negated, exceptions and extensions', () => {
    for (const l of ['~a.com##.x', 'a.com#@#.x', 'a.com#?#div:has(.x)', 'a.com#$#abort-on-property-read x', 'a.com##+js(set, x, 1)']) expect(one(l)).toEqual([])
  })
  it('rejects risky selectors', () => {
    expect(isSafeSelector('.ad')).toBe(true)
    expect(isSafeSelector('div[id^="ad-"]:not(.keep)')).toBe(true)
    expect(isSafeSelector('div:has-text(Ad)')).toBe(false)
    expect(isSafeSelector('a::before')).toBe(false)
    expect(isSafeSelector('div[x="a')).toBe(false)
    expect(isSafeSelector('.a{color:red}')).toBe(false)
  })
  it('can disable hiding per site', () => {
    expect(convertFilterList('a.com##.x', { siteCosmetic: false }).rules).toEqual([])
  })
})

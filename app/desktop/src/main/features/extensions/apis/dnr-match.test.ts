import { describe, expect, it } from 'vitest'
import type { Rule } from './dnr'
import {
  RuleIndex,
  anchoredHost,
  applyHeaderOps,
  applyTransform,
  compileRule,
  dnrType,
  makeRequest,
  redirectTarget,
  registrableDomain,
  resolve,
  ruleMatches,
  upgradedUrl,
  urlFilterTest,
  type CompiledRule,
  type DnrRequest,
  type ResolveContext
} from './dnr-match'

const rule = (id: number, condition: Record<string, unknown>, action: Rule['action'] = { type: 'block' }, priority?: number, ext = 'ext'): CompiledRule =>
  compileRule(ext, '_dynamic', { id, condition, action, ...(priority !== undefined && { priority }) })!
const req = (url: string, extra: { type?: string; initiator?: string; method?: string; tabId?: number } = {}): DnrRequest =>
  makeRequest({ url, type: 'script', ...extra })!
const ctx = (over: Partial<ResolveContext> = {}): ResolveContext => ({ rank: () => 0, canAct: () => true, ...over })

describe('urlFilter', () => {
  const t = (filter: string, url: string, cs = false) => urlFilterTest(filter, cs)(url, url.toLowerCase())
  it('matches plain text anywhere, case-insensitively by default', () => {
    expect(t('/ads/', 'https://a.com/ADS/x.js')).toBe(true)
    expect(t('/ads/', 'https://a.com/ADS/x.js', true)).toBe(false)
    expect(t('banner', 'https://a.com/x')).toBe(false)
  })
  it('handles the start and end anchors', () => {
    expect(t('|https://a.com/', 'https://a.com/x')).toBe(true)
    expect(t('|a.com', 'https://a.com/')).toBe(false)
    expect(t('.js|', 'https://a.com/x.js')).toBe(true)
    expect(t('.js|', 'https://a.com/x.js?v=1')).toBe(false)
    expect(t('|https://a.com/x|', 'https://a.com/x')).toBe(true)
  })
  it('anchors || on a host label', () => {
    expect(t('||example.com^', 'https://example.com/a')).toBe(true)
    expect(t('||example.com^', 'https://sub.example.com/a')).toBe(true)
    expect(t('||example.com^', 'https://notexample.com/a')).toBe(false)
    expect(t('||example.com^', 'https://other.com/?u=example.com')).toBe(false)
    expect(t('||example.com/ads', 'http://x.example.com/ads/1')).toBe(true)
    expect(t('||example.com^', 'https://example.com.evil.net/')).toBe(false)
  })
  it('^ matches separators and the end of the URL, not letters, digits or _ - . %', () => {
    expect(t('a.com^', 'https://a.com/')).toBe(true)
    expect(t('a.com^', 'https://a.com')).toBe(true)
    expect(t('a.com^', 'https://a.com:81/')).toBe(true)
    expect(t('a.com^', 'https://a.com.au/')).toBe(false)
    expect(t('a.com^', 'https://a.com-x/')).toBe(false)
    expect(t('a.com^', 'https://a.com_x/')).toBe(false)
  })
  it('* is a wildcard', () => {
    expect(t('/ads/*/banner', 'https://a.com/ads/x/y/banner')).toBe(true)
    expect(t('/ads/*/banner', 'https://a.com/ads/banner')).toBe(false)
    expect(t('||a.com/*.gif|', 'https://a.com/x/y.gif')).toBe(true)
  })
  it('extracts the host of a ||host^ filter only when it is exact', () => {
    expect(anchoredHost('||Ads.Example.com^')).toBe('ads.example.com')
    expect(anchoredHost('||example.com/path')).toBe('example.com')
    expect(anchoredHost('||example.com')).toBeNull()
    expect(anchoredHost('||example.*')).toBeNull()
    expect(anchoredHost('|https://a.com/')).toBeNull()
  })
})

describe('request helpers', () => {
  it('maps Electron resource types', () => {
    expect(dnrType('mainFrame')).toBe('main_frame')
    expect(dnrType('subFrame')).toBe('sub_frame')
    expect(dnrType('xhr')).toBe('xmlhttprequest')
    expect(dnrType('webSocket')).toBe('websocket')
    expect(dnrType('cspReport')).toBe('csp_report')
    expect(dnrType('serviceWorker')).toBe('other')
    expect(dnrType(undefined)).toBe('other')
  })
  it('ignores URLs that are not network requests', () => {
    expect(makeRequest({ url: 'data:text/plain,hi' })).toBeNull()
    expect(makeRequest({ url: 'chrome-extension://abc/x.js' })).toBeNull()
    expect(makeRequest({ url: 'not a url' })).toBeNull()
  })
  it('finds the registrable domain', () => {
    expect(registrableDomain('www.example.com')).toBe('example.com')
    expect(registrableDomain('a.b.example.co.uk')).toBe('example.co.uk')
    expect(registrableDomain('user.github.io')).toBe('user.github.io')
    expect(registrableDomain('localhost')).toBe('localhost')
    expect(registrableDomain('127.0.0.1')).toBe('127.0.0.1')
  })
})

describe('conditions', () => {
  it('matches every type but main_frame by default', () => {
    const r = rule(1, { urlFilter: 'a.com' })
    expect(ruleMatches(r, req('https://a.com/', { type: 'script' }))).toBe(true)
    expect(ruleMatches(r, req('https://a.com/', { type: 'sub_frame' }))).toBe(true)
    expect(ruleMatches(r, req('https://a.com/', { type: 'main_frame' }))).toBe(false)
    expect(ruleMatches(rule(2, { urlFilter: 'a.com', excludedResourceTypes: ['image'] }), req('https://a.com/', { type: 'main_frame' }))).toBe(false)
  })
  it('honors resourceTypes and excludedResourceTypes', () => {
    const r = rule(1, { resourceTypes: ['main_frame', 'xmlhttprequest'], excludedResourceTypes: ['xmlhttprequest'] })
    expect(ruleMatches(r, req('https://a.com/', { type: 'main_frame' }))).toBe(true)
    expect(ruleMatches(r, req('https://a.com/', { type: 'xmlhttprequest' }))).toBe(false)
    expect(ruleMatches(r, req('https://a.com/', { type: 'script' }))).toBe(false)
  })
  it('honors request methods', () => {
    const r = rule(1, { requestMethods: ['post'] })
    expect(ruleMatches(r, req('https://a.com/', { method: 'POST' }))).toBe(true)
    expect(ruleMatches(r, req('https://a.com/', { method: 'GET' }))).toBe(false)
    expect(ruleMatches(rule(2, { excludedRequestMethods: ['get'] }), req('https://a.com/', { method: 'GET' }))).toBe(false)
  })
  it('matches request domains with their subdomains', () => {
    const r = rule(1, { requestDomains: ['Example.com'], excludedRequestDomains: ['no.example.com'] })
    expect(ruleMatches(r, req('https://example.com/'))).toBe(true)
    expect(ruleMatches(r, req('https://a.example.com/'))).toBe(true)
    expect(ruleMatches(r, req('https://no.example.com/'))).toBe(false)
    expect(ruleMatches(r, req('https://badexample.com/'))).toBe(false)
  })
  it('matches initiator domains; a request without initiator never matches initiatorDomains', () => {
    const r = rule(1, { initiatorDomains: ['site.org'] })
    expect(ruleMatches(r, req('https://x.com/', { initiator: 'https://www.site.org' }))).toBe(true)
    expect(ruleMatches(r, req('https://x.com/', { initiator: 'https://other.org' }))).toBe(false)
    expect(ruleMatches(r, req('https://x.com/'))).toBe(false)
    const ex = rule(2, { excludedInitiatorDomains: ['site.org'] })
    expect(ruleMatches(ex, req('https://x.com/', { initiator: 'https://site.org' }))).toBe(false)
    expect(ruleMatches(ex, req('https://x.com/'))).toBe(true)
  })
  it('tells first party from third party by registrable domain', () => {
    const third = rule(1, { domainType: 'thirdParty' })
    const first = rule(2, { domainType: 'firstParty' })
    const same = req('https://cdn.site.org/a.js', { initiator: 'https://www.site.org' })
    const other = req('https://tracker.net/a.js', { initiator: 'https://www.site.org' })
    expect(ruleMatches(third, same)).toBe(false)
    expect(ruleMatches(third, other)).toBe(true)
    expect(ruleMatches(first, same)).toBe(true)
    expect(ruleMatches(first, other)).toBe(false)
  })
  it('uses tab ids for session rules only', () => {
    const s = compileRule('e', '_session', { id: 1, condition: { tabIds: [5] }, action: { type: 'block' } })!
    expect(ruleMatches(s, req('https://a.com/', { tabId: 5 }))).toBe(true)
    expect(ruleMatches(s, req('https://a.com/', { tabId: 6 }))).toBe(false)
    expect(ruleMatches(rule(2, { tabIds: [5] }), req('https://a.com/', { tabId: 6 }))).toBe(true)
  })
  it('matches regexFilter, skips invalid ones and rejects both filters together', () => {
    const r = rule(1, { regexFilter: '^https://a\\.com/(ad|banner)s?/' })
    expect(ruleMatches(r, req('https://a.com/ads/1'))).toBe(true)
    expect(ruleMatches(r, req('https://a.com/news/1'))).toBe(false)
    expect(ruleMatches(rule(2, { regexFilter: 'A\\.COM' }), req('https://a.com/'))).toBe(true)
    expect(ruleMatches(rule(3, { regexFilter: 'A\\.COM', isUrlFilterCaseSensitive: true }), req('https://a.com/'))).toBe(false)
    expect(ruleMatches(rule(4, { regexFilter: '(' }), req('https://a.com/'))).toBe(false)
    expect(compileRule('e', 's', { id: 5, condition: { urlFilter: 'a', regexFilter: 'b' }, action: { type: 'block' } })).toBeNull()
  })
})

describe('RuleIndex', () => {
  const rules = [
    rule(1, { urlFilter: '||ads.example.com^' }),
    rule(2, { requestDomains: ['tracker.net', 'metrics.io'] }),
    rule(3, { urlFilter: '/banner/' }),
    rule(4, { urlFilter: '||other.org^' }),
    rule(5, { urlFilter: '||example.com' })
  ]
  const index = new RuleIndex(rules)
  const ids = (url: string) =>
    index
      .match(req(url))
      .map((r) => r.id)
      .sort()
  it('looks up the host and its parent domains plus the generic rules', () => {
    expect(ids('https://ads.example.com/x')).toEqual([1, 5])
    expect(ids('https://deep.ads.example.com/x')).toEqual([1, 5])
    expect(ids('https://www.tracker.net/p')).toEqual([2])
    expect(ids('https://site.org/banner/1')).toEqual([3])
    expect(ids('https://example.com/')).toEqual([5])
  })
  it('only considers the rules of the host, not the whole set', () => {
    const many = Array.from({ length: 100_000 }, (_, i) => rule(i + 1, { urlFilter: `||site${i}.example^` }))
    many.push(rule(200_000, { urlFilter: '/ads/' }))
    const big = new RuleIndex(many)
    expect(big.size).toBe(100_001)
    expect(big.candidates('www.site42.example')).toBe(2) // the generic rule and the one of site42.example
    expect(big.candidates('unrelated.org')).toBe(1)
    expect(big.match(req('https://site7.example/x')).map((r) => r.id)).toEqual([8])
  })
  it('reports a rule filed under several buckets once', () => {
    const idx = new RuleIndex([rule(1, { requestDomains: ['b.c', 'c'] })])
    expect(idx.match(req('https://a.b.c/')).length).toBe(1)
  })
})

describe('resolve', () => {
  const run = (rules: CompiledRule[], url = 'https://a.com/x', c: Partial<ResolveContext> = {}, extra = {}) => {
    const q = req(url, extra)
    return resolve(new RuleIndex(rules).match(q), q, ctx(c))
  }
  it('lets the highest priority win', () => {
    const res = run([rule(1, { urlFilter: 'a.com' }, { type: 'block' }, 1), rule(2, { urlFilter: 'a.com' }, { type: 'allow' }, 2)])
    expect(res.action).toBe('allow')
    expect(run([rule(1, { urlFilter: 'a.com' }, { type: 'block' }, 3), rule(2, { urlFilter: 'a.com' }, { type: 'allow' }, 2)]).action).toBe('block')
  })
  it('breaks ties allow > allowAllRequests > block > upgradeScheme > redirect', () => {
    const mk = (type: string, id: number) => rule(id, { urlFilter: 'a.com' }, { type, redirect: { url: 'https://b.com/' } })
    expect(run([mk('redirect', 1), mk('upgradeScheme', 2)]).action).toBe('upgrade')
    expect(run([mk('upgradeScheme', 2), mk('block', 3)]).action).toBe('block')
    expect(run([mk('block', 3), mk('allowAllRequests', 4)]).action).toBe('allow')
    expect(run([mk('allowAllRequests', 4), mk('allow', 5), mk('block', 3)]).rule?.id).toBe(5)
  })
  it('defaults the priority to 1', () => {
    expect(rule(1, { urlFilter: 'a' }).priority).toBe(1)
  })
  it('gives precedence to the extension ranked first, and an allow ends the decision', () => {
    const a = rule(1, { urlFilter: 'a.com' }, { type: 'allow' }, 1, 'A')
    const b = rule(2, { urlFilter: 'a.com' }, { type: 'block' }, 9, 'B')
    expect(run([a, b], undefined, { rank: (e) => (e === 'A' ? 0 : 1) }).action).toBe('allow')
    expect(run([a, b], undefined, { rank: (e) => (e === 'B' ? 0 : 1) }).action).toBe('block')
  })
  it('ignores rules the extension cannot act on', () => {
    const res = run([rule(1, { urlFilter: 'a.com' }, { type: 'block' })], undefined, { canAct: () => false })
    expect(res.action).toBe('none')
  })
  it('applies modifyHeaders above the winning allow, never when blocked', () => {
    const set = (id: number, priority: number, value: string) =>
      rule(id, { urlFilter: 'a.com' }, { type: 'modifyHeaders', requestHeaders: [{ header: 'X-A', operation: 'set', value }] }, priority)
    const allow = rule(9, { urlFilter: 'a.com' }, { type: 'allow' }, 5)
    const res = run([allow, set(1, 3, 'low'), set(2, 7, 'high')])
    expect(res.action).toBe('allow')
    expect(res.requestHeaders).toEqual([{ header: 'X-A', operation: 'set', value: 'high' }])
    expect(run([rule(8, { urlFilter: 'a.com' }, { type: 'block' }, 1), set(3, 9, 'v')]).requestHeaders).toEqual([])
  })
  it('combines headers: set locks lower priorities, append and equal priorities combine, other extensions cannot touch a locked header', () => {
    const mh = (id: number, priority: number, ops: unknown[], ext = 'A') =>
      rule(id, { urlFilter: 'a.com' }, { type: 'modifyHeaders', requestHeaders: ops }, priority, ext)
    const res = run(
      [
        mh(1, 5, [{ header: 'X-A', operation: 'set', value: '1' }]),
        mh(2, 3, [{ header: 'X-A', operation: 'set', value: '2' }]),
        mh(3, 3, [{ header: 'X-B', operation: 'append', value: 'b1' }]),
        mh(4, 2, [{ header: 'X-B', operation: 'append', value: 'b2' }]),
        mh(5, 9, [{ header: 'x-a', operation: 'remove' }], 'B')
      ],
      undefined,
      { rank: (e) => (e === 'A' ? 0 : 1) }
    )
    expect(res.requestHeaders.map((o) => `${o.header}:${o.operation}:${o.value ?? ''}`)).toEqual(['X-A:set:1', 'X-B:append:b1', 'X-B:append:b2'])
  })
  it('remembers allowAllRequests for frame requests only, and applies an inherited one', () => {
    const allowAll = rule(1, { urlFilter: 'a.com', resourceTypes: ['main_frame'] }, { type: 'allowAllRequests' }, 4)
    const main = run([allowAll], 'https://a.com/', {}, { type: 'main_frame' })
    expect(main.action).toBe('allow')
    expect(main.frameAllows).toEqual([{ ext: 'ext', priority: 4 }])
    expect(run([allowAll], 'https://a.com/', {}, { type: 'script' }).frameAllows).toEqual([])
    const blockLow = rule(2, { urlFilter: 'tracker' }, { type: 'block' }, 2)
    const blockHigh = rule(3, { urlFilter: 'tracker' }, { type: 'block' }, 6)
    expect(run([blockLow], 'https://tracker.net/', { frameAllows: new Map([['ext', 4]]) }).action).toBe('allow')
    expect(run([blockHigh], 'https://tracker.net/', { frameAllows: new Map([['ext', 4]]) }).action).toBe('block')
  })
})

describe('redirects', () => {
  const redirect = (r: Record<string, unknown>, extra: Record<string, unknown> = {}) =>
    rule(1, { urlFilter: 'a.com', ...extra }, { type: 'redirect', redirect: r }, 1, 'abcdefghijklmnopabcdefghijklmnop')
  const q = req('https://a.com:8080/p/q?x=1&y=2#frag')
  it('redirects to a URL or an extension path', () => {
    expect(redirectTarget(redirect({ url: 'https://b.com/z' }), q)).toBe('https://b.com/z')
    expect(redirectTarget(redirect({ url: 'javascript:alert(1)' }), q)).toBeNull()
    expect(redirectTarget(redirect({ extensionPath: '/res/noop.js' }), q)).toBe('chrome-extension://abcdefghijklmnopabcdefghijklmnop/res/noop.js')
    expect(redirectTarget(redirect({ extensionPath: 'res/noop.js' }), q)).toBeNull()
    expect(redirectTarget(redirect({ url: q.url }), q)).toBeNull()
  })
  it('transforms scheme, host, port, path, query, fragment and credentials', () => {
    expect(applyTransform(q.url, { scheme: 'http', host: 'b.com', port: '' })).toBe('http://b.com/p/q?x=1&y=2#frag')
    expect(applyTransform(q.url, { path: '/new', query: '?k=v', fragment: '#f2' })).toBe('https://a.com:8080/new?k=v#f2')
    expect(applyTransform(q.url, { query: '', fragment: '' })).toBe('https://a.com:8080/p/q')
    expect(applyTransform(q.url, { username: 'u', password: 'p' })).toBe('https://u:p@a.com:8080/p/q?x=1&y=2#frag')
  })
  it('removes and adds query parameters', () => {
    expect(applyTransform(q.url, { queryTransform: { removeParams: ['x'] } })).toBe('https://a.com:8080/p/q?y=2#frag')
    expect(applyTransform(q.url, { queryTransform: { removeParams: ['x', 'y'] } })).toBe('https://a.com:8080/p/q#frag')
    expect(
      applyTransform(q.url, {
        queryTransform: {
          addOrReplaceParams: [
            { key: 'x', value: '9' },
            { key: 'z', value: 'a b' }
          ]
        }
      })
    ).toBe('https://a.com:8080/p/q?x=9&y=2&z=a%20b#frag')
    expect(applyTransform(q.url, { queryTransform: { addOrReplaceParams: [{ key: 'new', value: '1', replaceOnly: true }] } })).toBe(
      'https://a.com:8080/p/q?x=1&y=2#frag'
    )
  })
  it('substitutes regex groups', () => {
    const r = redirect({ regexSubstitution: 'https://mirror.com/\\1/\\2?o=\\0' }, { urlFilter: undefined, regexFilter: '^https://a\\.com/(\\w+)/(\\w+)$' })
    const url = req('https://a.com/foo/bar')
    expect(redirectTarget(r, url)).toBe('https://mirror.com/foo/bar?o=https://a.com/foo/bar')
    expect(redirectTarget(redirect({ regexSubstitution: 'https://m.com/\\9?q=1' }, { urlFilter: undefined, regexFilter: 'a\\.com' }), url)).toBe(
      'https://m.com/?q=1'
    )
  })
  it('upgrades http and ws only', () => {
    expect(upgradedUrl('http://a.com:80/x')).toBe('https://a.com/x')
    expect(upgradedUrl('http://a.com:8080/x')).toBe('https://a.com:8080/x')
    expect(upgradedUrl('ws://a.com/s')).toBe('wss://a.com/s')
    expect(upgradedUrl('https://a.com/')).toBeNull()
  })
})

describe('applyHeaderOps', () => {
  it('sets, appends and removes headers case-insensitively', () => {
    const out = applyHeaderOps(
      { 'User-Agent': 'x', Accept: 'a' },
      [
        { header: 'user-agent', operation: 'set', value: 'y' },
        { header: 'Accept', operation: 'append', value: 'b' },
        { header: 'X-New', operation: 'append', value: 'n' },
        { header: 'Missing', operation: 'remove' }
      ],
      false
    )
    expect(out).toEqual({ 'user-agent': 'y', Accept: 'a, b', 'X-New': 'n' })
  })
  it('keeps arrays for response headers and does not touch the input', () => {
    const input = { 'Set-Cookie': ['a=1'], 'X-Frame-Options': ['DENY'] }
    const out = applyHeaderOps(
      input,
      [
        { header: 'set-cookie', operation: 'append', value: 'b=2' },
        { header: 'X-Frame-Options', operation: 'remove' },
        { header: 'CSP', operation: 'set', value: "default-src 'self'" }
      ],
      true
    )
    expect(out).toEqual({ 'Set-Cookie': ['a=1', 'b=2'], CSP: ["default-src 'self'"] })
    expect(input['X-Frame-Options']).toEqual(['DENY'])
  })
})

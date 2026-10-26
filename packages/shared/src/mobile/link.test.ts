import { describe, expect, it } from 'vitest'
import {
  activeLinkIds,
  addLink,
  backoffMs,
  controlAction,
  decideLocalhost,
  orderEndpoints,
  pairErrorKey,
  parseLinks,
  patchLink,
  rememberEndpoint,
  removeLink,
  replacedLinks,
  serializeLinks,
  sortServers,
  type StoredLink
} from './link'

const mk = (n: number, extra: Partial<StoredLink> = {}): StoredLink => ({
  linkId: n.toString(16).padStart(16, '0'),
  desktop: { id: `desk-${n}`, name: `Mac ${n}`, platform: 'macos' },
  certSha256: 'A'.repeat(43),
  port: 47615,
  hosts: ['192.168.1.2', '10.0.0.2'],
  last: [],
  enabled: true,
  peerPaused: false,
  invalid: false,
  pairedAt: 1,
  ...extra
})

describe('link list', () => {
  it('reads back its own save and ignores the rest', () => {
    const list = [mk(1), mk(2, { enabled: false, last: [{ host: '1.1.1.1', port: 5 }] })]
    expect(parseLinks(JSON.parse(serializeLinks(list)))).toEqual(list)
    expect(parseLinks('x')).toEqual([])
    expect(parseLinks([null, 3, { linkId: 'zz' }, mk(1), mk(1)])).toEqual([mk(1)])
    expect(parseLinks([{ ...mk(1), enabled: 'oui', last: 'x', hosts: [3, 'a'] }])[0]).toMatchObject({ enabled: true, last: [], hosts: ['a'] })
  })
  it('a new pairing replaces the link of the same computer', () => {
    const old = mk(1)
    const fresh = mk(9, { desktop: old.desktop })
    expect(addLink([old, mk(2)], fresh).map((l) => l.linkId)).toEqual([fresh.linkId, mk(2).linkId])
    expect(replacedLinks([old, mk(2)], fresh)).toEqual([old])
  })
  it('removes and edits a link', () => {
    expect(removeLink([mk(1), mk(2)], mk(1).linkId)).toEqual([mk(2)])
    expect(patchLink([mk(1), mk(2)], mk(2).linkId, { invalid: true })[1]!.invalid).toBe(true)
  })
})

describe('adresses', () => {
  it('mDNS, then last addresses, then QR code, no duplicates', () => {
    const link = mk(1, {
      last: [
        { host: '10.0.0.2', port: 47615 },
        { host: '9.9.9.9', port: 1000 }
      ]
    })
    const c = orderEndpoints([{ host: '192.168.1.2', port: 50000 }], link)
    expect(c.map((x) => `${x.source}:${x.host}:${x.port}`)).toEqual([
      'mdns:192.168.1.2:50000',
      'last:10.0.0.2:47615',
      'last:9.9.9.9:1000',
      'offer:192.168.1.2:47615'
    ])
  })
  it('remembers the last addresses, most recent first', () => {
    let last = rememberEndpoint([], { host: 'a', port: 1 })
    last = rememberEndpoint(last, { host: 'b', port: 1 })
    last = rememberEndpoint(last, { host: 'a', port: 1 })
    expect(last.map((x) => x.host)).toEqual(['a', 'b'])
    for (let i = 0; i < 10; i++) last = rememberEndpoint(last, { host: `h${i}`, port: 1 })
    expect(last).toHaveLength(4)
  })
  it('reconnection wait: 1 s up to 30 s', () => {
    expect([0, 1, 2, 3, 4, 5, 20].map(backoffMs)).toEqual([1000, 2000, 4000, 8000, 16000, 30000, 30000])
    expect(backoffMs(-3)).toBe(1000)
  })
})

describe('control connection', () => {
  const base = { wantConnected: true, hasConnection: false, connected: false, enabledHere: true, invalid: false }
  it('connects when everything is allowed', () => {
    expect(controlAction(base)).toBe('connect')
    expect(controlAction({ ...base, hasConnection: true, connected: true })).toBe('none')
  })
  it('switch off: pause then close if connected, otherwise close, never connect', () => {
    expect(controlAction({ ...base, enabledHere: false, hasConnection: true, connected: true })).toBe('pause')
    expect(controlAction({ ...base, enabledHere: false, hasConnection: true })).toBe('close')
    expect(controlAction({ ...base, enabledHere: false })).toBe('none')
  })
  it('background: closes silently; invalid link: closes', () => {
    expect(controlAction({ ...base, wantConnected: false, hasConnection: true, connected: true })).toBe('close')
    expect(controlAction({ ...base, wantConnected: false })).toBe('none')
    expect(controlAction({ ...base, invalid: true, hasConnection: true })).toBe('close')
    expect(controlAction({ ...base, invalid: true })).toBe('none')
  })
})

describe('localhost', () => {
  const a = mk(1).linkId
  const b = mk(2).linkId
  it('active links: connected, allowed, valid, not paused, master switch ON', () => {
    const rt = { [a]: { connected: true }, [b]: { connected: true } }
    expect(activeLinkIds([mk(1), mk(2)], rt, true)).toEqual([a, b])
    expect(activeLinkIds([mk(1), mk(2)], rt, false)).toEqual([])
    expect(activeLinkIds([mk(1, { enabled: false }), mk(2, { peerPaused: true })], rt, true)).toEqual([])
    expect(activeLinkIds([mk(1, { invalid: true }), mk(2)], { [b]: { connected: false } }, true)).toEqual([])
  })
  it('adresse locale : un ordinateur, plusieurs, aucun', () => {
    expect(decideLocalhost('https://example.com', [a], null)).toEqual({ kind: 'none' })
    expect(decideLocalhost('http://127.0.0.1:5173/x', [], null)).toEqual({ kind: 'none' })
    expect(decideLocalhost('http://127.0.0.1:5173/x', [a], null)).toEqual({ kind: 'route', linkId: a, url: 'http://localhost:5173/x' })
    expect(decideLocalhost('http://localhost:3000', [a, b], null)).toEqual({ kind: 'ask', url: 'http://localhost:3000' })
    expect(decideLocalhost('http://localhost:3000', [a, b], b)).toEqual({ kind: 'route', linkId: b, url: 'http://localhost:3000' })
    expect(decideLocalhost('http://localhost:3000', [a, b], 'gone')).toMatchObject({ kind: 'ask' })
  })
  it('sorted servers', () => {
    const s = sortServers([
      { url: 'http://localhost:9000', port: 9000 },
      { url: 'http://localhost:3000', port: 3000 },
      { url: 'http://localhost:8000', port: 8000, fromTab: true }
    ])
    expect(s.map((x) => x.port)).toEqual([8000, 3000, 9000])
  })
})

describe('erreurs d appairage', () => {
  it('one message key per cause', () => {
    expect(pairErrorKey('expired')).toBe('expired')
    expect(pairErrorKey('bad-code')).toBe('used')
    expect(pairErrorKey('pin-mismatch')).toBe('pin')
    expect(pairErrorKey('timeout')).toBe('unreachable')
    expect(pairErrorKey('invalid')).toBe('invalid')
  })
})

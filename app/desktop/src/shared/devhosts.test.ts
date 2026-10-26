import { describe, expect, it } from 'vitest'
import { aliasFor, aliasTarget, isAliasName, isLocalDevUrl, isLocalHost, resolveAlias } from './devhosts'

describe('isLocalHost', () => {
  it('recognizes the machine and reserved names', () => {
    for (const h of ['localhost', 'LOCALHOST', 'app.localhost', 'nas.local', 'site.test', '::1', '[::1]', '0.0.0.0']) {
      expect(isLocalHost(h), h).toBe(true)
    }
  })

  it('recognizes private networks', () => {
    for (const h of ['127.0.0.1', '10.1.2.3', '192.168.0.10', '172.16.0.1', '172.31.255.1']) expect(isLocalHost(h), h).toBe(true)
  })

  it('rejects the rest', () => {
    for (const h of ['example.com', '8.8.8.8', '172.15.0.1', '172.32.0.1', '192.169.0.1', 'notlocalhost', 'localhost.evil.com']) {
      expect(isLocalHost(h), h).toBe(false)
    }
  })
})

describe('isLocalDevUrl', () => {
  it('accepte http(s) local seulement', () => {
    expect(isLocalDevUrl('http://localhost:3000')).toBe(true)
    expect(isLocalDevUrl('https://192.168.1.2/x')).toBe(true)
    expect(isLocalDevUrl('ftp://localhost')).toBe(false)
    expect(isLocalDevUrl('https://example.com')).toBe(false)
    expect(isLocalDevUrl('http://')).toBe(false)
  })
})

describe('isAliasName', () => {
  it('accepte minuscules, chiffres, tirets et suffixe .test/.localhost', () => {
    for (const n of ['monsite', 'my-app', 'a', 'app1.test', 'app.localhost']) expect(isAliasName(n), n).toBe(true)
  })

  it('rejects the rest', () => {
    for (const n of ['', 'Monsite', '-a', 'a-', 'a_b', 'a.com', 'a.b.test', 'a'.repeat(64)]) expect(isAliasName(n), n).toBe(false)
  })
})

describe('aliasTarget', () => {
  it('completes a port number', () => {
    expect(aliasTarget('7353')).toBe('http://localhost:7353')
  })

  it('completes a host without a scheme, removes the trailing /, the query and the fragment', () => {
    expect(aliasTarget('localhost:7353')).toBe('http://localhost:7353')
    expect(aliasTarget('127.0.0.1:3000/app/')).toBe('http://127.0.0.1:3000/app')
    expect(aliasTarget('http://localhost:3000/a?x=1#y')).toBe('http://localhost:3000/a')
    expect(aliasTarget('  localhost  ')).toBe('http://localhost')
  })

  it('rejects a non-local or invalid address', () => {
    expect(aliasTarget('example.com')).toBeNull()
    expect(aliasTarget('https://example.com:3000')).toBeNull()
    expect(aliasTarget('')).toBeNull()
  })
})

describe('resolveAlias / aliasFor', () => {
  const aliases = [
    { name: 'monsite', target: 'http://localhost:3000' },
    { name: 'api.test', target: 'http://localhost:8080/v1' }
  ]

  it('resolves a bare name, with a path, query or scheme', () => {
    expect(resolveAlias('monsite', aliases)).toBe('http://localhost:3000')
    expect(resolveAlias('MonSite/admin?x=1', aliases)).toBe('http://localhost:3000/admin?x=1')
    expect(resolveAlias('http://api.test/users', aliases)).toBe('http://localhost:8080/v1/users')
    expect(resolveAlias('monsite?q=1', aliases)).toBe('http://localhost:3000?q=1')
  })

  it('returns null without a match or without an alias', () => {
    expect(resolveAlias('autre', aliases)).toBeNull()
    expect(resolveAlias('monsite', [])).toBeNull()
    expect(resolveAlias('monsite', undefined)).toBeNull()
    expect(resolveAlias('mon site', aliases)).toBeNull()
  })

  it('aliasFor finds the name of an address (same origin, path under the target)', () => {
    expect(aliasFor('http://localhost:3000', aliases)?.name).toBe('monsite')
    expect(aliasFor('http://localhost:3000/a/b', aliases)?.name).toBe('monsite')
    expect(aliasFor('http://localhost:3000?x=1', aliases)?.name).toBe('monsite')
    expect(aliasFor('http://localhost:3000#h', aliases)?.name).toBe('monsite')
    expect(aliasFor('http://localhost:8080/v1/users', aliases)?.name).toBe('api.test')
  })

  it('aliasFor does not confuse two ports or two neighboring paths', () => {
    expect(aliasFor('http://localhost:30001', aliases)).toBeNull()
    expect(aliasFor('http://localhost:8080/v10', aliases)).toBeNull()
    expect(aliasFor('ftp://localhost:3000', aliases)).toBeNull()
    expect(aliasFor('http://localhost:3000', undefined)).toBeNull()
  })
})

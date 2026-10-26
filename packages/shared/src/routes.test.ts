import { describe, expect, it } from 'vitest'
import type { SpaceRoute } from './types'
import { matchPattern, matchRoute, normalizePattern } from './routes'

describe('normalizePattern', () => {
  it('strips scheme, case and trailing slash', () => {
    expect(normalizePattern('  HTTPS://GitHub.com/Foo// ')).toBe('github.com/foo')
    expect(normalizePattern('example.com')).toBe('example.com')
  })
})

describe('matchPattern', () => {
  it('exact host and subdomains', () => {
    expect(matchPattern('github.com', 'https://github.com/x')).toBe(true)
    expect(matchPattern('github.com', 'https://gist.github.com/x')).toBe(true)
    expect(matchPattern('github.com', 'https://notgithub.com')).toBe(false)
    expect(matchPattern('github.com', 'https://github.com.evil.org')).toBe(false)
  })

  it('ignores case', () => {
    expect(matchPattern('GitHub.com', 'https://GITHUB.com/')).toBe(true)
  })

  it('wildcard * without crossing /', () => {
    expect(matchPattern('*.example.com', 'https://a.example.com')).toBe(true)
    expect(matchPattern('docs.*', 'https://docs.example.com')).toBe(true) // * also covers the dots of the host
    expect(matchPattern('example.*', 'https://example.com')).toBe(true)
  })

  it('chemin', () => {
    expect(matchPattern('github.com/foo', 'https://github.com/foo')).toBe(true)
    expect(matchPattern('github.com/foo', 'https://github.com/foo/bar')).toBe(true)
    expect(matchPattern('github.com/foo', 'https://github.com/foobar')).toBe(false)
    expect(matchPattern('github.com/foo', 'https://github.com/')).toBe(false)
    expect(matchPattern('github.com/foo/*', 'https://github.com/foo/bar')).toBe(true)
  })

  it('escapes special characters', () => {
    expect(matchPattern('a+b.com', 'https://a+b.com')).toBe(true)
    expect(matchPattern('a.com', 'https://axcom.com')).toBe(false)
  })

  it('only http(s) and valid URLs', () => {
    expect(matchPattern('example.com', 'ftp://example.com')).toBe(false)
    expect(matchPattern('example.com', 'not a url')).toBe(false)
    expect(matchPattern('', 'https://example.com')).toBe(false)
  })
})

describe('matchRoute', () => {
  const routes = [
    { id: '1', pattern: 'github.com', spaceId: 'a' },
    { id: '2', pattern: 'github.com/foo', spaceId: 'b' }
  ] as unknown as SpaceRoute[]
  it('first matching rule', () => {
    expect(matchRoute('https://github.com/foo', routes)?.pattern).toBe('github.com')
    expect(matchRoute('https://example.com', routes)).toBeUndefined()
  })
})

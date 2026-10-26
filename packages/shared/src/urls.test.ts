import { describe, expect, it } from 'vitest'
import { fold, httpFallback, isHttpsFallbackError, isUiAddress, isUpgradedInput, searchUrl, setSearchLanguage, suggestRegion, toUrl } from './urls'

describe('toUrl', () => {
  it('rejects empty and inputs with spaces', () => {
    expect(toUrl('')).toBeNull()
    expect(toUrl('   ')).toBeNull()
    expect(toUrl('hello world')).toBeNull()
    expect(toUrl('example.com foo')).toBeNull()
  })

  it('keeps URLs with a known scheme', () => {
    expect(toUrl('https://example.com/a?b=1')).toBe('https://example.com/a?b=1')
    expect(toUrl('HTTP://example.com')).toBe('HTTP://example.com')
    expect(toUrl('file:///tmp/a.html')).toBe('file:///tmp/a.html')
    expect(toUrl('about:blank')).toBe('about:blank')
    expect(toUrl('view-source:https://example.com')).toBe('view-source:https://example.com')
  })

  it('keeps other schemes with //', () => {
    expect(toUrl('chrome-extension://abc/page.html')).toBe('chrome-extension://abc/page.html')
    expect(toUrl('ssh://host')).toBe('ssh://host')
  })

  it('normalizes signet: to signet://', () => {
    expect(toUrl('signet:settings')).toBe('signet://settings')
    expect(toUrl('signet://settings')).toBe('signet://settings')
    expect(toUrl('SIGNET:history')).toBe('signet://history')
  })

  it('refuses javascript: even when preceded by control characters', () => {
    expect(toUrl('javascript:alert(1)')).toBeNull()
    expect(toUrl('JavaScript:alert(1)')).toBeNull()
    expect(toUrl('\u0001javascript:alert(1)')).toBeNull()
  })

  it('refuse signetui:', () => {
    expect(toUrl('signetui://app/index.html')).toBeNull()
    expect(toUrl('SIGNETUI:x')).toBeNull()
  })

  it('adds http:// to local hosts and host:port', () => {
    expect(toUrl('localhost')).toBe('http://localhost')
    expect(toUrl('localhost:3000/x')).toBe('http://localhost:3000/x')
    expect(toUrl('192.168.1.10:8080')).toBe('http://192.168.1.10:8080')
    expect(toUrl('nas:8080')).toBe('http://nas:8080')
  })

  it('adds https:// to domains', () => {
    expect(toUrl('example.com')).toBe('https://example.com')
    expect(toUrl('sub.example.co.uk/path?q=1')).toBe('https://sub.example.co.uk/path?q=1')
    expect(toUrl('example.com:8443')).toBe('https://example.com:8443')
  })

  it('returns null for a single word or a fake domain', () => {
    expect(toUrl('hello')).toBeNull()
    expect(toUrl('foo.')).toBeNull()
    expect(toUrl('a.b')).toBeNull()
  })

  it('ignores surrounding spaces', () => {
    expect(toUrl('  example.com  ')).toBe('https://example.com')
  })
})

describe('isUiAddress', () => {
  it('recognizes signetui: with or without leading spaces', () => {
    expect(isUiAddress('signetui://app')).toBe(true)
    expect(isUiAddress('  SIGNETUI:x')).toBe(true)
    expect(isUiAddress('https://signetui.com')).toBe(false)
  })
})

describe('repli http', () => {
  it('isHttpsFallbackError', () => {
    expect(isHttpsFallbackError(-200)).toBe(true)
    expect(isHttpsFallbackError(-299)).toBe(true)
    expect(isHttpsFallbackError(-102)).toBe(true)
    expect(isHttpsFallbackError(-105)).toBe(false) // DNS
    expect(isHttpsFallbackError(-3)).toBe(false)
    expect(isHttpsFallbackError(0)).toBe(false)
  })

  it('isUpgradedInput', () => {
    expect(isUpgradedInput('example.com', 'https://example.com')).toBe(true)
    expect(isUpgradedInput('https://example.com', 'https://example.com')).toBe(false)
    expect(isUpgradedInput('localhost', 'http://localhost')).toBe(false)
    expect(isUpgradedInput('x', null)).toBe(false)
  })

  it('httpFallback', () => {
    expect(httpFallback('https://example.com/a', 'https://example.com/a')).toBe('http://example.com/a')
    expect(httpFallback('https://example.com/a', 'https://other.com/a')).toBeNull()
    expect(httpFallback('https://example.com/a', 'https://example.com/b')).toBeNull()
    expect(httpFallback('http://example.com/a', 'http://example.com/a')).toBeNull()
    expect(httpFallback('nope', 'https://example.com')).toBeNull()
  })
})

describe('recherche', () => {
  it('searchUrl without language', () => {
    setSearchLanguage('')
    expect(searchUrl('a b&c')).toBe('https://www.google.com/search?q=a%20b%26c')
  })

  it('searchUrl with base language', () => {
    setSearchLanguage('fr-FR')
    expect(searchUrl('x')).toBe('https://www.google.com/search?q=x&hl=fr')
    setSearchLanguage(undefined)
    expect(searchUrl('x')).toBe('https://www.google.com/search?q=x')
  })

  it('keeps the region for zh and pt', () => {
    setSearchLanguage('zh-TW')
    expect(searchUrl('x')).toBe('https://www.google.com/search?q=x&hl=zh-TW')
    setSearchLanguage('pt-BR')
    expect(searchUrl('x')).toContain('&hl=pt-BR')
    setSearchLanguage('')
  })

  it('suggestRegion', () => {
    setSearchLanguage('fr-FR')
    expect(suggestRegion()).toBe('fr-fr')
    setSearchLanguage('en-GB')
    expect(suggestRegion()).toBe('uk-en')
    setSearchLanguage('en-US')
    expect(suggestRegion()).toBe('us-en')
    setSearchLanguage('fr')
    expect(suggestRegion()).toBe('')
    setSearchLanguage('')
  })
})

describe('fold', () => {
  it('strips accents and case', () => {
    expect(fold('Éléphant ÇA')).toBe('elephant ca')
  })
})

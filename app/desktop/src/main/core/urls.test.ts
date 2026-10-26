import { beforeEach, describe, expect, it, vi } from 'vitest'

vi.mock('electron', () => ({}))

import { setSearchLanguage } from '@signet/shared/urls'
import { engineSearchUrl, engineSuggestions, setSearchEngine } from './urls'

describe('search engine', () => {
  beforeEach(() => {
    setSearchEngine(undefined)
    setSearchLanguage('fr-FR')
  })

  it('Google by default, with the page language', () => {
    expect(engineSearchUrl('a b')).toBe('https://www.google.com/search?q=a%20b&hl=fr')
  })

  it('other engine: its template, without hl', () => {
    setSearchEngine('duckduckgo')
    expect(engineSearchUrl('a b')).toBe('https://duckduckgo.com/?q=a%20b')
    setSearchEngine('bing')
    expect(engineSearchUrl('x')).toBe('https://www.bing.com/search?q=x')
  })

  it('unknown value: falls back to Google', () => {
    setSearchEngine('bing')
    setSearchEngine('nope')
    expect(engineSearchUrl('x')).toContain('google.com')
  })

  it('suggestions: chosen engine, kl region for DuckDuckGo only', () => {
    expect(engineSuggestions('a').id).toBe('google')
    setSearchEngine('duckduckgo')
    expect(engineSuggestions('a').url).toBe('https://duckduckgo.com/ac/?q=a&type=list&kl=fr-fr')
    setSearchEngine('brave')
    expect(engineSuggestions('a').url).toBe('https://search.brave.com/api/suggest?q=a')
  })
})

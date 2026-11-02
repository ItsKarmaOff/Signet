import { beforeEach, describe, expect, it, vi } from 'vitest'

vi.mock('electron', () => ({}))

import { setSearchLanguage } from '@signet/shared/urls'
import { engineSearchUrl, engineSuggestions, keywordSearch, setSearchEngine, setSearchEngines, setSearchLocale } from './urls'

describe('search engine', () => {
  beforeEach(() => {
    setSearchEngine(undefined)
    setSearchEngines({})
    setSearchLocale('en')
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
    expect(engineSuggestions('a')?.id).toBe('google')
    setSearchEngine('duckduckgo')
    expect(engineSuggestions('a')?.url).toBe('https://duckduckgo.com/ac/?q=a&type=list&kl=fr-fr')
    setSearchEngine('brave')
    expect(engineSuggestions('a')?.url).toBe('https://search.brave.com/api/suggest?q=a')
  })

  const docs = { id: 'custom-abc123', name: 'Docs', keyword: 'd', template: 'https://docs.example/s?q=%s', suggest: 'https://docs.example/sg?q=%s' }

  it('custom engine: default, search URL and suggestions', () => {
    setSearchEngines({ searchEngines: [docs] })
    setSearchEngine(docs.id)
    expect(engineSearchUrl('a b')).toBe('https://docs.example/s?q=a%20b')
    expect(engineSuggestions('a b')).toEqual({ id: docs.id, url: 'https://docs.example/sg?q=a%20b' })
    // deleted: back to Google
    setSearchEngines({})
    expect(engineSearchUrl('x')).toContain('google.com')
  })

  it('custom engine without suggestions address: none', () => {
    setSearchEngines({ searchEngines: [{ ...docs, suggest: undefined }] })
    setSearchEngine(docs.id)
    expect(engineSuggestions('a')).toBeNull()
  })

  it('keyword search: engine URL, Wikipedia in the UI language, lone keyword ignored', () => {
    expect(keywordSearch('yt cats')?.url).toBe('https://www.youtube.com/results?search_query=cats')
    expect(keywordSearch('w paris')?.url).toBe('https://en.wikipedia.org/wiki/Special:Search?search=paris')
    setSearchLocale('fr')
    expect(keywordSearch('w paris')?.url).toBe('https://fr.wikipedia.org/wiki/Special:Search?search=paris')
    expect(keywordSearch('w')).toBeNull()
    expect(keywordSearch('w.org')).toBeNull()
    setSearchEngines({ searchKeywords: { youtube: { keyword: 'vid' } } })
    expect(keywordSearch('yt cats')).toBeNull()
    expect(keywordSearch('vid cats')?.engine.id).toBe('youtube')
  })
})

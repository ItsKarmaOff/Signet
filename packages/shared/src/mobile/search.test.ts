import { describe, expect, it } from 'vitest'
import { parseSuggestions, resolveInput, SEARCH_ENGINES, searchUrlFor, suggestionsUrlFor } from './search'

describe('search', () => {
  it('each engine has a template with %s', () => {
    for (const e of SEARCH_ENGINES) expect(e.template).toContain('%s')
  })
  it('encodes the query', () => {
    expect(searchUrlFor('duckduckgo', 'a b&c')).toBe('https://duckduckgo.com/?q=a%20b%26c')
  })
  it('resolveInput: URL, search or nothing', () => {
    expect(resolveInput('example.com')).toBe('https://example.com')
    expect(resolveInput('chats mignons', 'bing')).toBe('https://www.bing.com/search?q=chats%20mignons')
    expect(resolveInput('   ')).toBeNull()
    expect(resolveInput('javascript:alert(1)')).toContain('google.com/search')
  })
})

describe('engine suggestions', () => {
  it('addresses: encoded query, one API per engine', () => {
    expect(suggestionsUrlFor('google', 'a b&c')).toBe('https://suggestqueries.google.com/complete/search?client=firefox&q=a%20b%26c')
    expect(suggestionsUrlFor('duckduckgo', 'x')).toBe('https://duckduckgo.com/ac/?q=x&type=list')
    for (const e of SEARCH_ENGINES) expect(suggestionsUrlFor(e.id, 'zz')).toMatch(/^https:\/\/.+zz/)
  })
  it('format OpenSearch : Google, Bing, DuckDuckGo, Brave, Ecosia', () => {
    const json = ['rea', ['react', 'react native', 'rea', 'React', 'reactive']]
    for (const id of ['google', 'bing', 'duckduckgo', 'brave', 'ecosia'] as const) {
      expect(parseSuggestions(id, json, 'rea')).toEqual(['react', 'react native', 'reactive'])
    }
  })
  it('DuckDuckGo without type=list: { phrase } objects', () => {
    expect(parseSuggestions('duckduckgo', [{ phrase: 'cats' }, { phrase: 'cat food' }], 'cat')).toEqual(['cats', 'cat food'])
  })
  it('Qwant : data.items[].value', () => {
    const json = { status: 'success', data: { items: [{ value: 'paris', suggestType: 1 }, { value: 'paris météo' }] } }
    expect(parseSuggestions('qwant', json, 'par')).toEqual(['paris', 'paris météo'])
  })
  it('limits, ignores noise and unexpected responses', () => {
    expect(parseSuggestions('google', ['q', ['a', 'b', 'c', 'd', 'e', 'f', 'g']], 'q')).toHaveLength(5)
    expect(parseSuggestions('google', ['q', ['a', 3, null, '', '  ', { x: 1 }, 'b']], 'q')).toEqual(['a', 'b'])
    for (const bad of [null, undefined, 'x', 42, {}, [], ['q'], ['q', 'nope'], { data: 1 }]) {
      for (const e of SEARCH_ENGINES) expect(parseSuggestions(e.id, bad, 'q')).toEqual([])
    }
  })
})

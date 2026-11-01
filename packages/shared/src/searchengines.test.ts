import { describe, expect, it } from 'vitest'
import {
  cleanDefaultEngine,
  cleanEngineConfig,
  expandTemplate,
  isSearchTemplate,
  keywordHint,
  keywordInfos,
  normalizeKeyword,
  parseKeyword,
  resolveEngines,
  validateEngineDraft,
  wikipediaHost
} from './searchengines'
import { parseSuggestions } from './mobile/search'

const engines = resolveEngines({}, 'en')
const custom = { id: 'custom-abc123', name: 'Docs', keyword: 'd', template: 'https://docs.example/s?q=%s', suggest: 'https://docs.example/sg?q=%s' }

describe('parseKeyword', () => {
  it('keyword + query', () => {
    expect(parseKeyword('yt cats', engines)).toMatchObject({ engine: { id: 'youtube' }, query: 'cats' })
  })
  it('keyword + space only: empty query', () => {
    expect(parseKeyword('yt ', engines)).toMatchObject({ engine: { id: 'youtube' }, query: '' })
  })
  it('lone keyword, unknown keyword and URL starting with a keyword are not searches', () => {
    expect(parseKeyword('yt', engines)).toBeNull()
    expect(parseKeyword('zz cats', engines)).toBeNull()
    expect(parseKeyword('w.org', engines)).toBeNull()
    expect(parseKeyword('w.org/page', engines)).toBeNull()
  })
  it('case and extra spaces', () => {
    expect(parseKeyword('  YT   many   cats  ', engines)).toMatchObject({ engine: { id: 'youtube' }, query: 'many   cats' })
  })
  it('disabled preset does not trigger', () => {
    const off = resolveEngines({ searchKeywords: { youtube: { disabled: true } } })
    expect(parseKeyword('yt cats', off)).toBeNull()
    expect(keywordHint('yt', off)).toBeNull()
  })
  it('hint on an exact keyword only', () => {
    expect(keywordHint(' W ', engines)?.id).toBe('wikipedia')
    expect(keywordHint('w cats', engines)).toBeNull()
    expect(keywordInfos(engines).find((k) => k.id === 'github')?.favicon).toBe('https://github.com/favicon.ico')
  })
})

describe('templates', () => {
  it('expands and encodes', () => {
    expect(expandTemplate('https://x.y/?q=%s&r=%s', 'a b&c')).toBe('https://x.y/?q=a%20b%26c&r=a%20b%26c')
    expect(expandTemplate('https://x.y/?q=%s', '$&%s')).toBe('https://x.y/?q=%24%26%25s')
  })
  it('validates', () => {
    expect(isSearchTemplate('https://a.b/?q=%s')).toBe(true)
    expect(isSearchTemplate('http://a.b/%s')).toBe(true)
    expect(isSearchTemplate('https://a.b/')).toBe(false)
    expect(isSearchTemplate('ftp://a.b/%s')).toBe(false)
    expect(isSearchTemplate('javascript:%s')).toBe(false)
    expect(isSearchTemplate(3)).toBe(false)
  })
  it('Wikipedia follows the UI language', () => {
    expect(wikipediaHost('fr')).toBe('fr.wikipedia.org')
    expect(wikipediaHost('fr-FR')).toBe('fr.wikipedia.org')
    expect(wikipediaHost('en')).toBe('en.wikipedia.org')
    expect(wikipediaHost(undefined)).toBe('en.wikipedia.org')
    expect(resolveEngines({}, 'fr').find((e) => e.id === 'wikipedia')?.template).toBe('https://fr.wikipedia.org/wiki/Special:Search?search=%s')
  })
  it('built-in defaults', () => {
    expect(engines.filter((e) => e.source === 'builtin').map((e) => e.keyword)).toEqual(['g', 'ddg', 'b', 'eco', 'br', 'q'])
    expect(engines.find((e) => e.id === 'github')?.template).toBe('https://github.com/search?q=%s')
  })
})

describe('cleaning', () => {
  it('keeps a valid custom engine, drops invalid ones', () => {
    const out = cleanEngineConfig({
      searchEngines: [
        custom,
        { ...custom, id: 'bad id' },
        { ...custom, id: 'custom-zzz999', keyword: 'has space' },
        { ...custom, id: 'custom-nnn111', template: 'https://x.y' },
        null,
        4
      ]
    })
    expect(out.searchEngines).toEqual([custom])
  })
  it('invalid suggestions address only drops the suggestions', () => {
    expect(cleanEngineConfig({ searchEngines: [{ ...custom, suggest: 'nope' }] }).searchEngines[0].suggest).toBeUndefined()
  })
  it('duplicate keywords: the first wins', () => {
    const out = cleanEngineConfig({
      searchEngines: [
        { ...custom, keyword: 'yt' },
        { ...custom, id: 'custom-two222', keyword: 'D' },
        { ...custom, id: 'custom-thr333', keyword: 'd' }
      ],
      searchKeywords: { bing: { keyword: 'G' }, brave: { keyword: 'Brv' }, github: { keyword: 'yt' } }
    })
    expect(out.searchEngines.map((c) => c.id)).toEqual(['custom-two222'])
    expect(out.searchEngines[0].keyword).toBe('d')
    expect(out.searchKeywords).toEqual({ brave: { keyword: 'brv' } })
  })
  it('overrides: disabled for presets only, default keyword is not stored, unknown ids ignored', () => {
    const out = cleanEngineConfig({ searchKeywords: { google: { disabled: true, keyword: 'g' }, youtube: { disabled: true }, nope: { keyword: 'x' } } })
    expect(out.searchKeywords).toEqual({ youtube: { disabled: true } })
    expect(cleanEngineConfig({ searchKeywords: 'x', searchEngines: 'y' })).toEqual({ searchEngines: [], searchKeywords: {} })
  })
  it('keyword normalization: no spaces, at most 32 characters', () => {
    expect(normalizeKeyword(' Yt ')).toBe('yt')
    expect(normalizeKeyword('a b')).toBeNull()
    expect(normalizeKeyword('')).toBeNull()
    expect(normalizeKeyword('a'.repeat(32))).not.toBeNull()
    expect(normalizeKeyword('a'.repeat(33))).toBeNull()
  })
  it('draft validation', () => {
    const all = resolveEngines({ searchEngines: [custom] })
    const ok = { name: 'X', keyword: 'x', template: 'https://x.y/?q=%s', suggest: '' }
    expect(validateEngineDraft(ok, all)).toBeNull()
    expect(validateEngineDraft({ ...ok, name: ' ' }, all)).toBe('name')
    expect(validateEngineDraft({ ...ok, keyword: 'a b' }, all)).toBe('keyword')
    expect(validateEngineDraft({ ...ok, keyword: 'YT' }, all)).toBe('keywordTaken')
    expect(validateEngineDraft({ ...ok, keyword: 'd' }, all, custom.id)).toBeNull()
    expect(validateEngineDraft({ ...ok, template: 'https://x.y' }, all)).toBe('template')
    expect(validateEngineDraft({ ...ok, suggest: 'x' }, all)).toBe('suggest')
  })
})

describe('default engine and suggestions', () => {
  it('falls back (undefined = Google) when the engine is gone or disabled', () => {
    const all = resolveEngines({ searchEngines: [custom], searchKeywords: { youtube: { disabled: true } } })
    expect(cleanDefaultEngine(custom.id, all)).toBe(custom.id)
    expect(cleanDefaultEngine('bing', all)).toBe('bing')
    expect(cleanDefaultEngine('youtube', all)).toBeUndefined()
    expect(cleanDefaultEngine('custom-gone', all)).toBeUndefined()
    expect(cleanDefaultEngine(3, all)).toBeUndefined()
  })
  it('custom engine suggestions are read as OpenSearch', () => {
    expect(parseSuggestions(custom.id, ['re', ['react', 'redux', 're']], 're')).toEqual(['react', 'redux'])
    expect(parseSuggestions(custom.id, { nope: 1 }, 're')).toEqual([])
  })
})

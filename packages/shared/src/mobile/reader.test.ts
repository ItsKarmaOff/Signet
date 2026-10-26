import { describe, expect, it } from 'vitest'
import { READER_EXTRACT_SCRIPT, buildReaderHtml, defaultReaderPrefs, escapeHtml, parseReaderMessage, readingMinutes, sanitizeReaderPrefs } from './reader'

const article = { title: 'A <b>title</b>', byline: 'Jane', siteName: 'Example', html: '<p>Hello</p>', url: 'https://example.com/a', length: 2400 }

describe('escapeHtml', () => {
  it('escapes the five characters', () => {
    expect(escapeHtml(`<a href="x">&'</a>`)).toBe('&lt;a href=&quot;x&quot;&gt;&amp;&#39;&lt;/a&gt;')
  })
})

describe('buildReaderHtml', () => {
  it('escapes the title and keeps the article body', () => {
    const html = buildReaderHtml(article, defaultReaderPrefs('light'), '1 min')
    expect(html).toContain('A &lt;b&gt;title&lt;/b&gt;')
    expect(html).toContain('<p>Hello</p>')
    expect(html).toContain('Example · Jane · 1 min')
    expect(html).toContain("default-src 'none'")
  })
  it('applies the size and theme', () => {
    const html = buildReaderHtml(article, { size: 24, theme: 'sepia', serif: false })
    expect(html).toContain('24px/1.6')
    expect(html).toContain('#f4ecd8')
    expect(html).not.toContain('Georgia')
  })
})

describe('sanitizeReaderPrefs', () => {
  const base = defaultReaderPrefs('dark')
  it('clamps the size and ignores invalid values', () => {
    expect(sanitizeReaderPrefs({ size: 99, theme: 'neon', serif: 'yes' }, base)).toEqual({ ...base, size: 28 })
    expect(sanitizeReaderPrefs({ size: 2 }, base).size).toBe(14)
    expect(sanitizeReaderPrefs(null, base)).toEqual(base)
  })
  it('dark theme by default in dark mode', () => {
    expect(defaultReaderPrefs('dark').theme).toBe('dark')
    expect(defaultReaderPrefs('light').theme).toBe('light')
  })
})

describe('parseReaderMessage', () => {
  it('reads an article or its absence', () => {
    expect(parseReaderMessage(JSON.stringify({ type: 'reader', article }))?.article?.title).toBe(article.title)
    expect(parseReaderMessage('{"type":"reader","article":null}')).toEqual({ article: null })
    expect(parseReaderMessage('{"type":"find"}')).toBeNull()
    expect(parseReaderMessage('nope')).toBeNull()
  })
})

describe('readingMinutes', () => {
  it('at least one minute', () => {
    expect(readingMinutes(10)).toBe(1)
    expect(readingMinutes(6 * 220 * 5)).toBe(5)
  })
})

describe('READER_EXTRACT_SCRIPT', () => {
  it('is valid JavaScript', () => {
    expect(() => new Function(READER_EXTRACT_SCRIPT)).not.toThrow()
  })
})

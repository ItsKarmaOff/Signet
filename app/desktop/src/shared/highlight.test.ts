import { describe, expect, it } from 'vitest'
import { highlightRange } from './highlight'

const cut = (text: string, q: string) => {
  const r = highlightRange(text, q)
  return r && text.slice(r[0], r[1])
}

describe('highlightRange', () => {
  it('is case insensitive and keeps the original text', () => {
    expect(cut('YouTube', 'you')).toBe('You')
  })
  it('maps folded indices back to the original (accents)', () => {
    expect(cut('Écoles', 'eco')).toBe('Éco')
    expect(cut('Café du coin', 'cafe')).toBe('Café')
    expect(cut('café noir', 'cafe')).toBe('café')
  })
  it('prefers a word start over the middle of a word', () => {
    expect(cut('Unfoo foo', 'foo')).toBe('foo')
    expect(highlightRange('Unfoo foo', 'foo')).toEqual([6, 9])
  })
  it('falls back to the first occurrence, null without match', () => {
    expect(cut('Unfoo', 'foo')).toBe('foo')
    expect(highlightRange('abc', 'x')).toBeNull()
    expect(highlightRange('abc', '  ')).toBeNull()
  })
})

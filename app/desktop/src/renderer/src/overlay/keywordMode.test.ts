import { describe, expect, it } from 'vitest'
import type { KeywordInfo } from '@signet/shared/searchengines'
import { keywordOnly, keywordPrefix, keywordQuery } from './keywordMode'

const kws: KeywordInfo[] = [
  { id: 'youtube', keyword: 'yt', name: 'YouTube' },
  { id: 'wikipedia', keyword: 'w', name: 'Wikipedia' }
]

describe('keyword mode', () => {
  it('keyword followed by a space enters the mode', () => {
    expect(keywordPrefix('yt ', kws)).toMatchObject({ info: { id: 'youtube' }, rest: '' })
    expect(keywordPrefix('yt cats', kws)?.rest).toBe('cats')
    expect(keywordPrefix('YT  cats', kws)?.rest).toBe('cats')
    expect(keywordPrefix('  w\tfoo', kws)?.info.id).toBe('wikipedia')
  })

  it('no mode without whitespace, or for another word', () => {
    expect(keywordPrefix('yt', kws)).toBeNull()
    expect(keywordPrefix('w.org', kws)).toBeNull()
    expect(keywordPrefix('w.org x', kws)).toBeNull()
    expect(keywordPrefix('ytx cats', kws)).toBeNull()
  })

  it('Tab only on a lone keyword', () => {
    expect(keywordOnly('yt', kws)?.id).toBe('youtube')
    expect(keywordOnly(' W ', kws)?.id).toBe('wikipedia')
    expect(keywordOnly('yt cats', kws)).toBeNull()
    expect(keywordOnly('w.org', kws)).toBeNull()
    expect(keywordOnly('', kws)).toBeNull()
  })

  it('query sent to main', () => {
    expect(keywordQuery(kws[0], 'cats')).toBe('yt cats')
    expect(keywordQuery(kws[0], '')).toBe('yt ')
  })
})

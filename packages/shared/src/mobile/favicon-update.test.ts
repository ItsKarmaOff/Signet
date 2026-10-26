import { describe, expect, it } from 'vitest'
import { setBookmarkFavicon } from './bookmarks'
import { setHistoryFavicon } from './history'
import type { Bookmark, HistoryEntry } from '../types'

describe('favicon updates', () => {
  it('sets the icon on a history entry, ignoring the fragment', () => {
    const list: HistoryEntry[] = [{ url: 'https://a.test/p', title: 'A', visits: 1, lastVisit: 1 }]
    const next = setHistoryFavicon(list, 'https://a.test/p#top', 'https://a.test/i.png')
    expect(next[0].favicon).toBe('https://a.test/i.png')
    expect(setHistoryFavicon(next, 'https://a.test/p', 'https://a.test/i.png')).toBe(next)
    expect(setHistoryFavicon(list, 'https://b.test', 'x')).toBe(list)
  })

  it('sets the icon on bookmarks, including inside folders', () => {
    const tree: Bookmark[] = [
      { kind: 'link', id: '1', title: 'A', url: 'https://a.test' },
      { kind: 'folder', id: 'f', name: 'F', items: [{ kind: 'link', id: '2', title: 'A2', url: 'https://a.test' }] }
    ]
    const next = setBookmarkFavicon(tree, 'https://a.test', 'https://a.test/i.png')
    expect(JSON.stringify(next)).toContain('"id":"2"')
    expect((next[0] as { favicon?: string }).favicon).toBe('https://a.test/i.png')
    expect(setBookmarkFavicon(next, 'https://a.test', 'https://a.test/i.png')).toBe(next)
    expect(setBookmarkFavicon(tree, 'https://zzz.test', 'x')).toBe(tree)
  })
})

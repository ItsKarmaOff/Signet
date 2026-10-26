import { describe, expect, it, vi } from 'vitest'

vi.mock('electron', () => ({
  app: { getPreferredSystemLanguages: () => ['en-US'], getPath: () => '' },
  webContents: { getAllWebContents: () => [] }
}))

import type { Space } from '@shared/types'
import { newSpace, type State } from '../../core/store'
import { applyBookmarks, applySpaces, MAX_IMPORTED_TABS } from './apply'
import type { ImportedNode, SpacesImport } from './types'

const tab = (url: string, title = url): ImportedNode => ({ kind: 'tab', url, title })

function state(): State {
  const space = newSpace({ name: 'S', icon: 'i', hue: 1 })
  return { tabs: {}, favorites: [], spaces: [space], activeSpaceId: space.id } as unknown as State
}

describe('applyBookmarks', () => {
  const folder: ImportedNode = {
    kind: 'folder',
    name: 'Chrome',
    items: [tab('https://a.com', 'A'), { kind: 'folder', name: 'Sous', items: [tab('https://b.com', 'B')] }]
  }

  it('empty bar: content goes directly into the bar', () => {
    const space = newSpace({ name: 'S', icon: 'i', hue: 1 })
    const r = applyBookmarks(space, folder)
    expect(r).toMatchObject({ bookmarks: 2, inFolder: false, truncated: false })
    expect(space.bookmarks!.map((b) => b.kind)).toEqual(['link', 'folder'])
    expect(space.bookmarks![0]).toMatchObject({ title: 'A', url: 'https://a.com', favicon: 'https://icons.duckduckgo.com/ip3/a.com.ico' })
  })

  it('non-empty bar: a folder named after the browser, at the end', () => {
    const space: Space = { ...newSpace({ name: 'S', icon: 'i', hue: 1 }), bookmarks: [{ kind: 'link', id: 'x', url: 'https://x.com', title: 'x' }] }
    const r = applyBookmarks(space, folder)
    expect(r.inFolder).toBe(true)
    expect(space.bookmarks).toHaveLength(2)
    expect(space.bookmarks![1]).toMatchObject({ kind: 'folder', name: 'Chrome' })
  })

  it('a split view gives one bookmark per tab; invalid URL without favicon', () => {
    const space = newSpace({ name: 'S', icon: 'i', hue: 1 })
    const r = applyBookmarks(space, {
      kind: 'folder',
      name: 'F',
      items: [
        {
          kind: 'split',
          tabs: [
            { url: 'https://a.com', title: 'a' },
            { url: 'pas une url', title: 'b' }
          ]
        }
      ]
    })
    expect(r.bookmarks).toBe(2)
    expect(space.bookmarks![1]).not.toHaveProperty('favicon')
  })
})

describe('applySpaces', () => {
  const data: SpacesImport = {
    spaces: [
      {
        name: 'Arc',
        icon: '🌈',
        pinned: [
          tab('https://p.com'),
          { kind: 'folder', name: 'F', items: [tab('https://q.com')] },
          {
            kind: 'split',
            tabs: [
              { url: 'https://s1.com', title: 's1' },
              { url: 'https://s2.com', title: 's2' }
            ]
          }
        ],
        today: [tab('https://t.com')]
      }
    ],
    favorites: [{ url: 'https://fav.com', title: 'Fav' }]
  }

  it('an empty state is replaced by the imported spaces', () => {
    const s = state()
    const c = applySpaces(s, data)
    expect(c).toMatchObject({ spaces: 1, pinned: 4, today: 1, favorites: 1, truncated: false })
    expect(s.spaces).toHaveLength(1)
    expect(s.spaces[0].name).toBe('Arc')
    expect(s.activeSpaceId).toBe(s.spaces[0].id)
    expect(Object.keys(s.tabs)).toHaveLength(6)
    const pinned = s.spaces[0].pinned
    expect(pinned.map((i) => i.kind)).toEqual(['tab', 'folder', 'split'])
    expect(Object.values(s.tabs).find((t) => t.url === 'https://p.com')).toMatchObject({ pinnedUrl: 'https://p.com' })
    expect(Object.values(s.tabs).find((t) => t.url === 'https://t.com')!.pinnedUrl).toBeUndefined()
    expect(s.favorites).toHaveLength(1)
  })

  it('a non-empty state receives the spaces in addition', () => {
    const s = state()
    s.spaces[0].today.push({ kind: 'tab', tabId: 'z' })
    const first = s.activeSpaceId
    applySpaces(s, data)
    expect(s.spaces).toHaveLength(2)
    expect(s.activeSpaceId).toBe(first)
  })

  it('favorites already present ignored', () => {
    const s = state()
    applySpaces(s, data)
    const again = applySpaces(s, { spaces: [], favorites: data.favorites })
    expect(again.favorites).toBe(0)
    expect(s.favorites).toHaveLength(1)
  })

  it('caps the number of imported tabs', () => {
    const many: SpacesImport = {
      spaces: [{ name: 'Big', icon: 'i', pinned: [], today: Array.from({ length: MAX_IMPORTED_TABS + 50 }, (_, i) => tab(`https://x.com/${i}`)) }],
      favorites: [{ url: 'https://f.com', title: 'f' }]
    }
    const s = state()
    const c = applySpaces(s, many)
    expect(c.truncated).toBe(true)
    expect(Object.keys(s.tabs)).toHaveLength(MAX_IMPORTED_TABS)
    expect(c.favorites).toBe(0)
  })
})

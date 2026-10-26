import { describe, expect, it, vi } from 'vitest'

vi.mock('electron', () => ({
  app: { getPreferredSystemLanguages: () => ['en-US'], getPath: () => '' },
  webContents: { getAllWebContents: () => [] }
}))

import { parseSpacesFile } from './arc'

const tabItem = (id: string, savedURL: string, title = '') => ({ id, title, data: { tab: { savedURL, savedTitle: 'saved ' + id } } })

/** Minimal file: one space, two roots (pinned / unpinned), global favorites. */
function file(items: unknown[], space: object = {}) {
  return {
    sidebar: {
      containers: [
        { global: {} },
        {
          spaces: [
            {
              id: 'sp1',
              title: 'Travail',
              customInfo: { iconType: { emoji_v2: '💼' } },
              containerIDs: ['pinned', 'rootP', 'unpinned', 'rootU'],
              ...space
            }
          ],
          items,
          topAppsContainerIDs: [{ default: true }, 'rootTop']
        }
      ]
    }
  }
}

describe('parseSpacesFile', () => {
  const items = [
    { id: 'rootP', data: { itemContainer: {} }, childrenIds: ['t1', 'f1', 'sv'] },
    tabItem('t1', 'https://one.com', 'Un'),
    { id: 'f1', title: 'Folder', data: { list: {} }, childrenIds: ['t2', 't3'] },
    tabItem('t2', 'https://two.com'),
    tabItem('t3', 'javascript:alert(1)'),
    { id: 'sv', data: { splitView: {} }, childrenIds: ['s1', 's2', 's3', 's4', 's5'] },
    tabItem('s1', 'https://s1.com'),
    tabItem('s2', 'https://s2.com'),
    tabItem('s3', 'https://s3.com'),
    tabItem('s4', 'https://s4.com'),
    tabItem('s5', 'https://s5.com'),
    { id: 'rootU', data: { itemContainer: {} }, childrenIds: ['f2', 't9'] },
    { id: 'f2', title: 'Jour', data: { list: {} }, childrenIds: ['t8'] },
    tabItem('t8', 'https://eight.com'),
    tabItem('t9', 'https://nine.com'),
    { id: 'rootTop', data: { itemContainer: {} }, childrenIds: ['top1', 'top2', 'top3'] },
    tabItem('top1', 'https://fav.com'),
    tabItem('top2', 'https://fav.com'),
    tabItem('top3', 'https://fav2.com')
  ]

  it('error if the format is unexpected', () => {
    expect(() => parseSpacesFile({})).toThrow()
    expect(() => parseSpacesFile(null)).toThrow()
    expect(() => parseSpacesFile({ sidebar: { containers: [{ spaces: [] }] } })).toThrow()
  })

  it('space: name, emoji, pinned with folders', () => {
    const { spaces } = parseSpacesFile(file(items))
    expect(spaces).toHaveLength(1)
    expect(spaces[0]).toMatchObject({ name: 'Travail', icon: '💼' })
    const [t1, folder, split, ...rest] = spaces[0].pinned
    expect(t1).toEqual({ kind: 'tab', url: 'https://one.com', title: 'Un' })
    expect(folder).toEqual({ kind: 'folder', name: 'Folder', items: [{ kind: 'tab', url: 'https://two.com', title: 'saved t2' }] })
    // split view limited to 3 tabs, the rest as single tabs
    expect(split).toMatchObject({ kind: 'split', tabs: [{ url: 'https://s1.com' }, { url: 'https://s2.com' }, { url: 'https://s3.com' }] })
    expect(rest.map((n) => (n.kind === 'tab' ? n.url : ''))).toEqual(['https://s4.com', 'https://s5.com'])
  })

  it('today tabs: folders flattened', () => {
    const { spaces } = parseSpacesFile(file(items))
    expect(spaces[0].today.map((n) => (n.kind === 'tab' ? n.url : n.kind))).toEqual(['https://eight.com', 'https://nine.com'])
  })

  it('global favorites deduplicated by address', () => {
    const { favorites } = parseSpacesFile(file(items))
    expect(favorites.map((f) => f.url)).toEqual(['https://fav.com', 'https://fav2.com'])
  })

  it('default values: numbered name, emoji ✨', () => {
    const { spaces } = parseSpacesFile(file(items, { title: '  ', customInfo: {} }))
    expect(spaces[0].name).toBe('Space 1')
    expect(spaces[0].icon).toBe('✨')
  })

  it('emoji given as Unicode code', () => {
    const { spaces } = parseSpacesFile(file(items, { customInfo: { iconType: { emoji: 0x1f600 } } }))
    expect(spaces[0].icon).toBe('😀')
  })

  it('cycles and missing references do not loop', () => {
    const loop = [
      { id: 'rootP', data: { itemContainer: {} }, childrenIds: ['rootP', 'ghost', 'f'] },
      { id: 'f', data: { list: {} }, childrenIds: ['rootP', 'f'] }
    ]
    expect(parseSpacesFile(file(loop)).spaces[0].pinned).toEqual([{ kind: 'folder', name: 'Folder', items: [] }])
  })

  it('arrays alternating identifier and object', () => {
    const f = file(items) as { sidebar: { containers: { items?: unknown[] }[] } }
    const useful = f.sidebar.containers[1]
    useful.items = (useful.items as { id: string }[]).flatMap((it) => [it.id, it]) // strings are ignored
    expect(parseSpacesFile(f).spaces[0].pinned).toHaveLength(5)
  })
})

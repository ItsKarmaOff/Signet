import { describe, expect, it } from 'vitest'
import type { Item, Space, TabInfo } from './types'

import {
  findFolderSlot,
  findSplitSlot,
  findTabSlot,
  folderContains,
  folderPaths,
  GROUP_HUES,
  groupRange,
  insertAt,
  insertToday,
  itemTabIds,
  leavesOf,
  newFolder,
  normalizeGroups,
  outsideGroups,
  pickGroupHue,
  pruneItems,
  removeAt,
  stripGroups,
  syncGroups,
  tabItem,
  walk,
  type Folder
} from './tree'

const tab = (tabId: string, group?: string): Item => ({ kind: 'tab', tabId, ...(group ? { group } : {}) })
const folder = (id: string, name: string, items: Item[] = []): Folder => ({ kind: 'folder', id, name, open: true, items })
const split = (id: string, tabIds: string[]): Item => ({ kind: 'split', id, tabIds })

/** a, [F1: b, [F2: c], d], e */
function sample(): Item[] {
  return [tab('a'), folder('F1', 'One', [tab('b'), folder('F2', 'Two', [tab('c')]), split('S', ['d', 'x'])]), tab('e')]
}

describe('parcours', () => {
  it('itemTabIds descends into folders and split views', () => {
    expect(itemTabIds(tab('a'))).toEqual(['a'])
    expect(itemTabIds(split('S', ['x', 'y']))).toEqual(['x', 'y'])
    expect(itemTabIds(sample()[1])).toEqual(['b', 'c', 'd', 'x'])
  })

  it('walk visits depth-first in order and can stop', () => {
    const seen: string[] = []
    walk(sample(), ({ item }) => {
      seen.push(item.kind === 'folder' ? item.id : item.kind === 'tab' ? item.tabId : item.id)
    })
    expect(seen).toEqual(['a', 'F1', 'b', 'F2', 'c', 'S', 'e'])
    const stop: string[] = []
    const r = walk(sample(), ({ item }) => {
      stop.push(item.kind)
      return item.kind === 'folder'
    })
    expect(r).toBe(true)
    expect(stop).toEqual(['tab', 'folder'])
  })

  it('walk gives parent and path', () => {
    let slot
    walk(sample(), (s) => {
      if (s.item.kind === 'tab' && s.item.tabId === 'c') slot = s
    })
    expect(slot).toMatchObject({ index: 0, parent: { id: 'F2' } })
    expect((slot as unknown as { path: Folder[] }).path.map((f) => f.id)).toEqual(['F1', 'F2'])
  })

  it('findTabSlot / findFolderSlot / findSplitSlot', () => {
    const items = sample()
    expect(findTabSlot(items, 'c')?.parent?.id).toBe('F2')
    expect(findTabSlot(items, 'x')?.item).toMatchObject({ kind: 'split', id: 'S' })
    expect(findTabSlot(items, 'zzz')).toBeNull()
    expect(findFolderSlot(items, 'F2')?.index).toBe(1)
    expect(findFolderSlot(items, 'nope')).toBeNull()
    expect(findSplitSlot(items, 'S')?.list).toBe((items[1] as Folder).items)
    expect(findSplitSlot(items, 'F1')).toBeNull()
  })

  it('folderContains', () => {
    const f = sample()[1] as Folder
    expect(folderContains(f, 'F1')).toBe(true)
    expect(folderContains(f, 'F2')).toBe(true)
    expect(folderContains(f, 'a')).toBe(false)
  })

  it('leavesOf flattens folders', () => {
    expect(leavesOf(sample()).map((l) => (l.kind === 'tab' ? l.tabId : l.id))).toEqual(['a', 'b', 'c', 'S', 'e'])
  })

  it('folderPaths gives "Parent / Child" labels', () => {
    expect(folderPaths(sample()).map((p) => p.label)).toEqual(['One', 'One / Two'])
  })

  it('removeAt / insertAt clamp the index', () => {
    const l = [tab('a'), tab('b')]
    expect(removeAt(l, 0)).toEqual(tab('a'))
    insertAt(l, 99, tab('z'))
    insertAt(l, -5, tab('y'))
    expect(l.map(itemTabIds).flat()).toEqual(['y', 'b', 'z'])
  })

  it('newFolder has a translated default name and a unique id', () => {
    const a = newFolder('New folder')
    const b = newFolder('Mon dossier')
    expect(a).toMatchObject({ kind: 'folder', name: 'New folder', open: true, items: [] })
    expect(b.name).toBe('Mon dossier')
    expect(a.id).not.toBe(b.id)
  })

  it('tabItem', () => {
    expect(tabItem('q')).toEqual({ kind: 'tab', tabId: 'q' })
  })
})

describe('groupes', () => {
  it('groupRange gives the bounds of the run', () => {
    const l = [tab('a'), tab('b', 'g'), tab('c', 'g'), tab('d')]
    expect(groupRange(l, 'g')).toEqual({ start: 1, end: 3 })
    expect(groupRange(l, 'nope')).toBeNull()
  })

  it('normalizeGroups removes single-item groups and detached runs', () => {
    const l = [tab('a', 'g'), tab('b'), tab('c', 'g'), tab('d', 'h'), tab('e', 'h')]
    normalizeGroups(l)
    // g: two runs separated, one item each -> the 1st is too short, the 2nd as well
    expect(l.map((i) => (i as { group?: string }).group)).toEqual([undefined, undefined, undefined, 'h', 'h'])
  })

  it('normalizeGroups keeps the first run and removes the second of the same group', () => {
    const l = [tab('a', 'g'), tab('b', 'g'), tab('c'), tab('d', 'g'), tab('e', 'g')]
    normalizeGroups(l)
    expect(l.map((i) => (i as { group?: string }).group)).toEqual(['g', 'g', undefined, undefined, undefined])
  })

  it('pickGroupHue takes the first free hue, then wraps around', () => {
    expect(pickGroupHue({})).toBe(GROUP_HUES[0])
    expect(pickGroupHue({ a: { name: '', hue: GROUP_HUES[0], open: true } })).toBe(GROUP_HUES[1])
    const all = Object.fromEntries(GROUP_HUES.map((hue, i) => [String(i), { name: '', hue, open: true }]))
    expect(pickGroupHue(all)).toBe(GROUP_HUES[0])
  })

  it('outsideGroups moves past a group if we land in the middle', () => {
    const l = [tab('a'), tab('b', 'g'), tab('c', 'g'), tab('d', 'g'), tab('e')]
    expect(outsideGroups(l, 2)).toBe(4)
    expect(outsideGroups(l, 1)).toBe(1)
    expect(outsideGroups(l, 4)).toBe(4)
    expect(outsideGroups(l, 99)).toBe(5)
  })

  it('insertToday joins the neighboring group or the surrounding one', () => {
    const l = [tab('a', 'g'), tab('b', 'g'), tab('c')]
    insertToday(l, 1, tab('x'))
    expect((l[1] as { group?: string }).group).toBe('g')
    insertToday(l, 4, tab('y'))
    expect((l[4] as { group?: string }).group).toBeUndefined()
    insertToday(l, 3, tab('z', 'old'), 'g')
    expect((l[3] as { group?: string }).group).toBe('g')
    insertToday(l, 5, tab('w', 'g'), 'h')
    expect((l[5] as { group?: string }).group).toBeUndefined()
  })

  it('stripGroups removes all labels, even inside folders', () => {
    const l: Item[] = [tab('a', 'g'), folder('F', 'F', [tab('b', 'g')])]
    stripGroups(l)
    expect(JSON.stringify(l)).not.toContain('group')
  })

  it('syncGroups creates a header named after the first tab and removes orphans', () => {
    const tabs = {
      a: { id: 'a', url: 'https://www.example.com/x', title: '', customTitle: '' },
      b: { id: 'b', url: 'https://b.org', title: 'B' }
    } as unknown as Record<string, TabInfo>
    const space = {
      id: 's',
      today: [tab('a', 'g'), tab('b', 'g'), tab('c', 'solo')],
      pinned: [],
      groups: { old: { name: 'x', hue: 1, open: true } }
    } as unknown as Space
    syncGroups(space, tabs)
    expect(Object.keys(space.groups!)).toEqual(['g'])
    expect(space.groups!.g).toMatchObject({ name: 'example.com', hue: GROUP_HUES[0], open: true })
    expect((space.today[2] as { group?: string }).group).toBeUndefined()
  })

  it('syncGroups deletes `groups` if empty and repairs an unreadable group', () => {
    const space = { id: 's', today: [tab('a')], pinned: [], groups: { g: { name: 3, hue: 'x' } } } as unknown as Space
    syncGroups(space, {})
    expect(space.groups).toBeUndefined()
    const space2 = { id: 's', today: [tab('a', 'g'), tab('b', 'g')], pinned: [], groups: { g: { name: 3 } } } as unknown as Space
    syncGroups(space2, { a: { id: 'a', url: '', title: 'Titre' } as TabInfo })
    expect(space2.groups!.g).toMatchObject({ name: 'Titre', open: true })
  })
})

describe('pruneItems', () => {
  const known = (id: string) => ['a', 'b', 'c'].includes(id)

  it('returns [] for a value that is not a list', () => {
    expect(pruneItems(undefined, known, true, 'Folder')).toEqual([])
    expect(pruneItems({}, known, true, 'Folder')).toEqual([])
  })

  it('removes unknown tabs and invalid items', () => {
    const out = pruneItems([tab('a'), tab('zzz'), null, 3, { kind: 'bidule' }], known, true, 'Folder')
    expect(out).toEqual([tab('a')])
  })

  it('split view: filters, 1 tab -> tab, 0 -> removed', () => {
    expect(pruneItems([split('S', ['zzz'])], known, true, 'Folder')).toEqual([])
    expect(pruneItems([split('S', ['a', 'zzz'])], known, true, 'Folder')).toEqual([tab('a')])
    expect(pruneItems([{ kind: 'split', id: 'S', tabIds: ['a', 'zzz'], group: 'g' }], known, true, 'Folder')).toEqual([{ kind: 'tab', tabId: 'a', group: 'g' }])
    expect(pruneItems([{ kind: 'split', id: 'S' }], known, true, 'Folder')).toEqual([])
  })

  it('split view: caps at 4 tabs and sanitizes layout/sizes', () => {
    const known5 = () => true
    const [s] = pruneItems(
      [{ kind: 'split', id: 'S', tabIds: ['1', '2', '3', '4', '5'], layout: 'columns', sizes: [1, 1] }],
      known5,
      true,
      'Folder'
    ) as Extract<Item, { kind: 'split' }>[]
    expect(s.tabIds).toEqual(['1', '2', '3', '4'])
    expect(s.layout).toBeUndefined() // columns = default
    expect(s.sizes).toBeUndefined() // inconsistent size
  })

  it('folder: normalized with default values', () => {
    const [f] = pruneItems([{ kind: 'folder', items: [tab('a'), tab('zzz')], icon: '😀😀😀😀😀😀😀😀😀', hue: 400.4 }], known, true, 'Folder') as Folder[]
    expect(f.name).toBe('Folder')
    expect(f.id).toMatch(/^[0-9a-f-]{36}$/)
    expect(f.open).toBe(true)
    expect(f.items).toEqual([tab('a')])
    expect(f.icon!.length).toBeLessThanOrEqual(16)
    expect(f.hue).toBe(40)
  })

  it('folder: keeps id, name, open=false', () => {
    const [f] = pruneItems([{ kind: 'folder', id: 'X', name: 'N', open: false, items: [] }], known, true, 'Folder') as Folder[]
    expect(f).toMatchObject({ id: 'X', name: 'N', open: false })
  })

  it('live folder: valid source only', () => {
    const live = { source: { kind: 'feed', url: 'https://e.com/rss' }, refreshing: true, refreshedAt: 5 }
    const [f] = pruneItems([{ kind: 'folder', id: 'X', name: 'N', items: [], live }], known, true, 'Folder') as Folder[]
    expect(f.live).toEqual({ source: { kind: 'feed', url: 'https://e.com/rss' }, refreshedAt: 5 })
    const [g] = pruneItems(
      [{ kind: 'folder', id: 'X', name: 'N', items: [], live: { source: { kind: 'feed', url: 'javascript:1' } } }],
      known,
      true,
      'Folder'
    ) as Folder[]
    expect(g.live).toBeUndefined()
  })

  it('folders=false flattens folders', () => {
    const out = pruneItems([tab('a'), folder('F', 'F', [tab('b'), folder('G', 'G', [tab('c')])])], known, false, 'Folder')
    expect(out).toEqual([tab('a'), tab('b'), tab('c')])
  })
})

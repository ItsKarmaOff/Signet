import { describe, expect, it } from 'vitest'
import type { Item } from '@shared/types'
import { activeSpace, displayedSplit, displayedTabIds, findFolder, findSplit, locate, reachable, spaceOfGroup } from './locate'
import { folder, split, tab, world } from './testkit'

describe('locate', () => {
  const w = world({
    tabs: ['fav', 'p1', 'p2', 't1', 't2', 'in'],
    favorites: ['fav'],
    spaces: [{ pinned: [tab('p1'), folder('f', [tab('in')])], today: [tab('t1'), split('sp', ['p2', 't2'])] }, { today: [] }]
  })

  it('a favorite: favorites zone, no space', () => {
    const loc = locate(w.state, 'fav')!
    expect(loc.zone).toBe('favorites')
    expect(loc.space).toBeNull()
    expect(loc.index).toBe(0)
  })

  it('a pinned tab and a day tab: zone, space and rank', () => {
    expect(locate(w.state, 'p1')).toMatchObject({ zone: 'pinned', index: 0 })
    expect(locate(w.state, 't1')).toMatchObject({ zone: 'today', index: 0 })
    expect(locate(w.state, 't1')!.space).toBe(w.space('s1'))
  })

  it("a tab in a folder: the list is the folder's", () => {
    const loc = locate(w.state, 'in')!
    expect(loc.zone).toBe('pinned')
    expect(loc.list).toBe((w.space().pinned[1] as Extract<Item, { kind: 'folder' }>).items)
  })

  it('a tab of a split view returns the split view', () => {
    const loc = locate(w.state, 't2')!
    expect(loc.item.kind).toBe('split')
    expect(loc.index).toBe(1)
  })

  it('onglet inconnu : null', () => {
    expect(locate(w.state, 'nope')).toBeNull()
  })

  it('findSplit et findFolder', () => {
    expect(findSplit(w.state, 'sp')?.item.kind).toBe('split')
    expect(findSplit(w.state, 'x')).toBeNull()
    expect(findFolder(w.state, 'f')?.zone).toBe('pinned')
    expect(findFolder(w.state, 'x')).toBeNull()
  })

  it('reachable: a favorite is reachable from everywhere, a tab of another space is not', () => {
    expect(reachable(w.state, w.space('s2'), 'fav')).toBe(true)
    expect(reachable(w.state, w.space('s1'), 't1')).toBe(true)
    expect(reachable(w.state, w.space('s2'), 't1')).toBe(false)
  })

  it('spaceOfGroup: the space whose day tabs carry the group', () => {
    const g = world({ tabs: ['a', 'b'], spaces: [{}, { today: [tab('a', 'g'), tab('b', 'g')] }] })
    expect(spaceOfGroup(g.state, 'g')).toBe(g.space('s2'))
    expect(spaceOfGroup(g.state, 'zz')).toBeUndefined()
  })
})

describe('active space and displayed tabs', () => {
  it('the active space is the one in activeSpaceId, otherwise the first', () => {
    const w = world({ spaces: [{}, {}], active: 's2' })
    expect(activeSpace(w.state).id).toBe('s2')
    w.state.activeSpaceId = 'disparu'
    expect(activeSpace(w.state).id).toBe('s1')
  })

  it('the active tab alone, or all those of its split view', () => {
    const w = world({ tabs: ['a', 'b', 'c'], spaces: [{ today: [tab('a'), split('sp', ['b', 'c'])], activeTabId: 'a' }] })
    expect(displayedTabIds(w.state, w.space())).toEqual(['a'])
    expect(displayedSplit(w.state, w.space())).toBeNull()
    w.space().activeTabId = 'c'
    expect(displayedTabIds(w.state, w.space())).toEqual(['b', 'c'])
    expect(displayedSplit(w.state, w.space())?.id).toBe('sp')
  })

  it("an active favorite (outside the space's lists): it alone", () => {
    const w = world({ tabs: ['fav'], favorites: ['fav'], spaces: [{ activeTabId: 'fav' }] })
    expect(displayedTabIds(w.state, w.space())).toEqual(['fav'])
  })

  it('aucun onglet actif, ou onglet disparu : rien', () => {
    const w = world({ tabs: ['a'], spaces: [{ today: [tab('a')], activeTabId: null }] })
    expect(displayedTabIds(w.state, w.space())).toEqual([])
    w.space().activeTabId = 'disparu'
    expect(displayedTabIds(w.state, w.space())).toEqual([])
  })
})

import { describe, expect, it } from 'vitest'
import { folder, split, tab, world } from './testkit'

describe('activation.setActive', () => {
  it("sets the space's active tab, dates its activation and notifies", () => {
    const w = world({ tabs: ['a'], spaces: [{ today: [tab('a')] }] })
    w.clock.now = 5000
    w.model.activation.setActive(w.space(), 'a')
    expect(w.space().activeTabId).toBe('a')
    expect(w.state.tabs.a.lastActive).toBe(5000)
    expect(w.effects.activated).toEqual([['s1', 'a']])
  })

  it('no tab: only the field changes, nobody is notified', () => {
    const w = world({ tabs: ['a'], spaces: [{ today: [tab('a')], activeTabId: 'a' }] })
    w.model.activation.setActive(w.space(), null)
    expect(w.space().activeTabId).toBeNull()
    expect(w.effects.activated).toEqual([])
  })
})

describe('activation.activate', () => {
  it("switches to the tab's space and returns the tab that was active", () => {
    const w = world({ tabs: ['a', 'b'], spaces: [{ today: [tab('a')], activeTabId: 'a' }, { today: [tab('b')] }] })
    expect(w.model.activation.activate('b')).toEqual({ before: 'a' })
    expect(w.state.activeSpaceId).toBe('s2')
    expect(w.space('s2').activeTabId).toBe('b')
  })

  it('a favorite keeps the current space', () => {
    const w = world({ tabs: ['fav'], favorites: ['fav'], spaces: [{}, {}], active: 's2' })
    w.model.activation.activate('fav')
    expect(w.state.activeSpaceId).toBe('s2')
    expect(w.space('s2').activeTabId).toBe('fav')
  })

  it('unknown tab: nothing changes', () => {
    const w = world({ tabs: ['a'], spaces: [{ today: [tab('a')], activeTabId: 'a' }] })
    expect(w.model.activation.activate('x')).toBeUndefined()
    expect(w.space().activeTabId).toBe('a')
  })
})

describe('activation.pickNext', () => {
  const w = world({ tabs: ['a', 'b', 'c', 'd', 'p'], spaces: [{ pinned: [tab('p')], today: [tab('a'), tab('b'), tab('c'), tab('d')] }] })
  const space = w.space()
  const pick = (index: number, removed: boolean, skip: string[]) => w.model.activation.pickNext(space, space.today, index, removed, new Set(skip))

  it('after removing the item: the one that takes its place (the next)', () => {
    expect(pick(1, true, ['b'])).toBe('c')
  })

  it('at the end of the list: the previous one, going back up', () => {
    expect(pick(4, true, ['d'])).toBe('c')
  })

  it('without removal: the item itself first, unless it is to be skipped', () => {
    expect(pick(1, false, [])).toBe('b')
    expect(pick(1, false, ['b'])).toBe('c')
  })

  it('list exhausted: fall back to the pinned tabs', () => {
    expect(pick(0, true, ['a', 'b', 'c', 'd'])).toBe('p')
  })

  it('nothing available: null', () => {
    expect(pick(0, true, ['a', 'b', 'c', 'd', 'p'])).toBeNull()
  })

  it('a split view with a single skipped tab provides the other', () => {
    const s = world({ tabs: ['x', 'y'], spaces: [{ today: [split('sp', ['x', 'y'])] }] })
    expect(s.model.activation.pickNext(s.space(), s.space().today, 0, true, new Set(['x']))).toBe('y')
  })
})

describe('activation.afterFavorite', () => {
  it('the most recently used loaded tab, favorites included', () => {
    const w = world({
      tabs: { fav: { loaded: true, lastActive: 50 }, a: { loaded: true, lastActive: 10 }, b: { loaded: false, lastActive: 90 } },
      favorites: ['fav'],
      spaces: [{ today: [tab('a'), tab('b')] }]
    })
    expect(w.model.activation.afterFavorite(w.space(), new Set())).toBe('fav')
  })

  it('a removed favorite is ignored', () => {
    const w = world({
      tabs: { fav: { loaded: true, lastActive: 50 }, a: { loaded: true, lastActive: 10 } },
      favorites: ['fav'],
      spaces: [{ today: [tab('a')] }]
    })
    expect(w.model.activation.afterFavorite(w.space(), new Set(['fav']))).toBe('a')
  })

  it('no loaded tab: the first today tab', () => {
    const w = world({ tabs: ['a', 'b', 'p'], spaces: [{ pinned: [tab('p')], today: [tab('a'), tab('b')] }] })
    expect(w.model.activation.afterFavorite(w.space(), new Set())).toBe('a')
  })

  it('an unloaded favorite is never woken up', () => {
    const w = world({ tabs: { fav: { loaded: false } }, favorites: ['fav'], spaces: [{}] })
    expect(w.model.activation.afterFavorite(w.space(), new Set())).toBeNull()
  })
})

describe('activation.repick', () => {
  it('the active tab removed from the list: the neighbor takes over', () => {
    const w = world({ tabs: ['a', 'b', 'c'], spaces: [{ today: [tab('a'), tab('b'), tab('c')], activeTabId: 'b' }] })
    const loc = w.model.lists.extract('b', false)!
    w.model.activation.repick(loc, ['b'], true)
    expect(w.space().activeTabId).toBe('c')
  })

  it('another active tab is not touched', () => {
    const w = world({ tabs: ['a', 'b'], spaces: [{ today: [tab('a'), tab('b')], activeTabId: 'a' }] })
    const loc = w.model.lists.extract('b', false)!
    w.model.activation.repick(loc, ['b'], true)
    expect(w.space().activeTabId).toBe('a')
  })

  it('a tab still reachable from the space (moved to a favorite) stays active', () => {
    const w = world({ tabs: ['a'], spaces: [{ today: [], activeTabId: 'a' }], favorites: ['a'] })
    w.model.activation.repick({ zone: 'today', space: w.space(), index: 0, list: w.space().today, item: tab('a') }, ['a'], true)
    expect(w.space().activeTabId).toBe('a')
  })

  it('another space that pointed to the removed tab falls back to its pinned tabs', () => {
    const w = world({
      tabs: ['m', 'p2'],
      spaces: [{ today: [tab('m')] }, { pinned: [tab('p2')], activeTabId: 'm' }]
    })
    const loc = w.model.lists.extract('m', false)!
    w.model.activation.repick(loc, ['m'], true)
    expect(w.space('s2').activeTabId).toBe('p2')
  })

  it('removing an active favorite: the most recent of the open tabs', () => {
    const w = world({
      tabs: { fav: { loaded: true }, a: { loaded: true, lastActive: 5 }, b: { loaded: true, lastActive: 9 } },
      favorites: ['fav'],
      spaces: [{ today: [tab('a'), tab('b')], activeTabId: 'fav' }]
    })
    const loc = w.model.lists.extract('fav', false)!
    w.model.activation.repick(loc, ['fav'], true)
    expect(w.space().activeTabId).toBe('b')
  })

  it('unload (removed = false): the next one, even if the tab stays in the list', () => {
    const w = world({ tabs: ['a', 'b'], spaces: [{ today: [tab('a'), tab('b')], activeTabId: 'a' }] })
    w.model.activation.repick(w.model.locate('a')!, ['a'], false)
    expect(w.space().activeTabId).toBe('b')
  })
})

describe('a folder counts as its tabs', () => {
  it('pickNext descends into folders', () => {
    const w = world({ tabs: ['a', 'in'], spaces: [{ pinned: [folder('f', [tab('in')])], today: [tab('a')] }] })
    expect(w.model.activation.pickNext(w.space(), w.space().today, 0, true, new Set(['a']))).toBe('in')
  })
})

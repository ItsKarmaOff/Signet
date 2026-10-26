import { describe, expect, it } from 'vitest'
import { CLOSED_MAX } from './closed'
import { folder, shape, tab, world } from './testkit'

describe('closed.remember', () => {
  it('keeps the address, title, original place and history', () => {
    const w = world({ tabs: { a: { title: 'A', favicon: 'ico', customTitle: 'Perso' }, b: {} }, spaces: [{ today: [tab('b'), tab('a')] }] })
    w.data.histories.set('a', { entries: [{ url: 'https://a.test/', title: 'A' }], index: 0 })
    w.model.closed.remember('a', w.model.locate('a'))
    expect(w.data.closed[0]).toMatchObject({
      url: 'https://a.test/',
      title: 'A',
      favicon: 'ico',
      customTitle: 'Perso',
      spaceId: 's1',
      zone: 'today',
      index: 1,
      history: { index: 0 }
    })
    expect(w.effects.saved).toEqual(['a'])
  })

  it('a tab in a folder: remembers the folder', () => {
    const w = world({ tabs: ['a', 'b'], spaces: [{ pinned: [folder('f', [tab('a'), tab('b')])] }] })
    w.model.closed.remember('b', w.model.locate('b'))
    expect(w.data.closed[0]).toMatchObject({ zone: 'pinned', folderId: 'f', index: 1 })
  })

  it('a tab from a group: remembers the group; `batch` and header are optional', () => {
    const w = world({ tabs: ['a'], spaces: [{ today: [tab('a', 'g')] }] })
    w.model.closed.remember('a', w.model.locate('a'), 7, { name: 'Groupe', hue: 1, open: true })
    expect(w.data.closed[0]).toMatchObject({ groupId: 'g', batch: 7, group: { name: 'Groupe' } })
  })

  it('without a known location: today tab of the active space, rank 0', () => {
    const w = world({ tabs: ['a'], spaces: [{}] })
    w.model.closed.remember('a', null)
    expect(w.data.closed[0]).toMatchObject({ spaceId: 's1', zone: 'today', index: 0 })
  })

  it('unknown tab: nothing', () => {
    const w = world()
    w.model.closed.remember('x', null)
    expect(w.data.closed).toEqual([])
  })

  it(`the stack is capped at ${CLOSED_MAX}, most recent first`, () => {
    const ids = Array.from({ length: CLOSED_MAX + 5 }, (_, i) => `t${i}`)
    const w = world({ tabs: ids })
    for (const id of ids) w.model.closed.remember(id, null)
    expect(w.data.closed).toHaveLength(CLOSED_MAX)
    expect(w.data.closed[0].url).toBe(`https://${ids[ids.length - 1]}.test/`)
  })
})

describe('closed.reopen', () => {
  it('empty stack: null', () => {
    expect(world().model.closed.reopen()).toBeNull()
  })

  it('a today tab comes back at its original rank, active, with its history', () => {
    const w = world({ tabs: ['a', 'b', 'c'], spaces: [{ today: [tab('a'), tab('b'), tab('c')] }] })
    w.data.histories.set('b', { entries: [{ url: 'https://b.test/', title: 'b' }], index: 0 })
    w.model.closed.remember('b', w.model.locate('b'))
    w.model.removal.remove('b')
    expect(shape(w.space().today)).toEqual(['a', 'c'])
    expect(w.model.closed.reopen()).toEqual({ more: 0 })
    expect(shape(w.space().today)).toEqual(['a', expect.any(String), 'c'])
    const reopened = (w.space().today[1] as { tabId: string }).tabId
    expect(w.state.tabs[reopened].url).toBe('https://b.test/')
    expect(w.data.histories.get(reopened)?.index).toBe(0)
    expect(w.space().activeTabId).toBe(reopened)
  })

  it('in another space: goes back there and makes it the active space', () => {
    const w = world({ tabs: ['a', 'x'], spaces: [{ today: [tab('a')] }, { today: [tab('x')] }], active: 's2' })
    w.model.closed.remember('a', w.model.locate('a'))
    w.model.removal.remove('a')
    w.model.closed.reopen()
    expect(w.state.activeSpaceId).toBe('s1')
    expect(w.space('s1').today).toHaveLength(1)
  })

  it('a pinned tab comes back in its folder, at its rank, with its "home" address', () => {
    const w = world({
      tabs: { a: { pinnedUrl: 'https://home.test/' }, b: { pinnedUrl: 'https://b.test/' } },
      spaces: [{ pinned: [folder('f', [tab('a'), tab('b')])] }]
    })
    w.model.closed.remember('a', w.model.locate('a'))
    w.model.removal.remove('a')
    w.model.closed.reopen()
    const f = w.space().pinned[0] as { items: { tabId: string }[] }
    expect(f.items).toHaveLength(2)
    expect(f.items[0].tabId).not.toBe('b')
    expect(w.state.tabs[f.items[0].tabId].pinnedUrl).toBe('https://home.test/')
  })

  it('folder gone: at the end of the pinned tabs', () => {
    const w = world({ tabs: ['a', 'p'], spaces: [{ pinned: [tab('p'), folder('f', [tab('a')])] }] })
    w.model.closed.remember('a', w.model.locate('a'))
    w.space().pinned.pop()
    w.model.closed.reopen()
    expect(w.space().pinned).toHaveLength(2)
    expect(w.space().pinned[1]).toMatchObject({ kind: 'tab' })
  })

  it("space deleted: a pinned tab comes back among the current space's today tabs", () => {
    const w = world({ tabs: ['a', 'x'], spaces: [{ pinned: [tab('a')] }, { today: [tab('x')] }] })
    w.model.closed.remember('a', w.model.locate('a'))
    w.state.spaces.shift()
    w.state.activeSpaceId = 's2'
    w.model.closed.reopen()
    expect(w.space('s2').today).toHaveLength(2)
  })

  it('a favorite comes back among the favorites and keeps the current space', () => {
    const w = world({ tabs: ['f1', 'f2'], favorites: ['f1', 'f2'], spaces: [{}, {}], active: 's2' })
    w.model.closed.remember('f1', w.model.locate('f1'))
    w.model.removal.remove('f1')
    w.model.closed.reopen()
    expect(w.state.favorites).toHaveLength(2)
    expect(w.state.activeSpaceId).toBe('s2')
    expect(w.space('s2').activeTabId).toBe(w.state.favorites[0])
  })

  it('a group closed at once comes back whole, grouped, with its header', () => {
    const w = world({
      tabs: ['a', 'b', 'c'],
      spaces: [{ today: [tab('a', 'g'), tab('b', 'g'), tab('c')], groups: { g: { name: 'Mon groupe', hue: 4, open: true } } }]
    })
    const space = w.space()
    expect(w.model.groups.close(space, 'g')).not.toBeNull()
    expect(shape(space.today)).toEqual(['c'])
    expect(w.model.closed.reopen()).toEqual({ more: 0 })
    expect(space.today).toHaveLength(3)
    const grouped = space.today.slice(0, 2) as { group: string }[]
    expect(grouped[0].group).toBe(grouped[1].group)
    expect(space.groups?.[grouped[0].group]).toMatchObject({ name: 'Mon groupe' })
    expect(space.activeTabId).toBe((space.today[0] as { tabId: string }).tabId)
  })

  it('a batch without header (folder deleted with its tabs) comes back tab by tab', () => {
    const w = world({ tabs: ['a', 'b'], spaces: [{ pinned: [folder('f', [tab('a'), tab('b')])] }] })
    w.model.folders.delete('f', true)
    expect(w.data.closed).toHaveLength(2)
    const first = w.model.closed.reopen()
    expect(first).toEqual({ more: 1 })
    expect(w.data.closed).toHaveLength(1)
    expect(w.data.closed[0].batch).toBeUndefined()
  })
})

describe('closed: copies and batches', () => {
  it('snapshotTabs / restoreTabs: puts back the vanished tabs, without touching the present ones', () => {
    const w = world({ tabs: ['a', 'b'] })
    w.data.histories.set('a', { entries: [], index: 0 })
    const snap = w.model.closed.snapshotTabs(['a', 'b', 'inconnu'])
    expect(snap.tabs.map((t) => t.id)).toEqual(['a', 'b'])
    delete w.state.tabs.a
    w.data.histories.delete('a')
    w.state.tabs.b.title = 'modified'
    w.model.closed.restoreTabs(snap)
    expect(w.state.tabs.a).toMatchObject({ loaded: false, loading: false, audible: false })
    expect(w.data.histories.has('a')).toBe(true)
    expect(w.state.tabs.b.title).toBe('modified')
  })

  it('dropBatch: removes only the batch tabs', () => {
    const w = world({ tabs: ['a', 'b', 'c'] })
    w.model.closed.remember('a', null, 1)
    w.model.closed.remember('b', null, 2)
    w.model.closed.remember('c', null)
    w.model.closed.dropBatch(1)
    expect(w.data.closed.map((c) => c.batch)).toEqual([undefined, 2])
  })

  it('nextBatch: increasing numbers', () => {
    const w = world()
    expect(w.model.closed.nextBatch()).toBe(1)
    expect(w.model.closed.nextBatch()).toBe(2)
  })
})

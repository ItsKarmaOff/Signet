import { describe, expect, it } from 'vitest'
import { folder, shape, split, tab, world } from './testkit'

const today = (index: number, extra = {}) => ({ zone: 'today' as const, spaceId: 's1', index, ...extra })

describe('lists.extract', () => {
  it('removes a tab from its list', () => {
    const w = world({ tabs: ['a', 'b'], spaces: [{ today: [tab('a'), tab('b')] }] })
    expect(w.model.lists.extract('a', false)?.index).toBe(0)
    expect(shape(w.space().today)).toEqual(['b'])
  })

  it('removes a favorite', () => {
    const w = world({ tabs: ['f'], favorites: ['f'] })
    expect(w.model.lists.extract('f', false)?.zone).toBe('favorites')
    expect(w.state.favorites).toEqual([])
  })

  it('in a split view of three: the view stays with two tabs (sizes reset)', () => {
    const w = world({ tabs: ['a', 'b', 'c'], spaces: [{ today: [{ kind: 'split', id: 'sp', tabIds: ['a', 'b', 'c'], sizes: [0.5, 0.25, 0.25] }] }] })
    const loc = w.model.lists.extract('b', false)!
    expect(loc.shrunk).toBe(true)
    expect(shape(w.space().today)).toEqual(['{sp: a c}'])
    expect(w.space().today[0]).not.toHaveProperty('sizes')
  })

  it('in a split view of two: the other tab keeps the position and the group', () => {
    const w = world({ tabs: ['a', 'b'], spaces: [{ today: [split('sp', ['a', 'b'], 'g')] }] })
    w.model.lists.extract('a', false)
    expect(shape(w.space().today)).toEqual(['b^g'])
  })

  it('`whole`: the entire split view goes', () => {
    const w = world({ tabs: ['a', 'b'], spaces: [{ today: [split('sp', ['a', 'b'])] }] })
    w.model.lists.extract('a', true)
    expect(w.space().today).toEqual([])
  })

  it('onglet inconnu : null', () => {
    expect(world().model.lists.extract('x', false)).toBeNull()
  })
})

describe('lists.moveItem : onglets', () => {
  it('in the same list downwards: the rank accounts for the removal', () => {
    const w = world({ tabs: ['a', 'b', 'c'], spaces: [{ today: [tab('a'), tab('b'), tab('c')] }] })
    expect(w.model.lists.moveItem({ kind: 'tab', tabId: 'a' }, today(3))).toBe(true)
    expect(shape(w.space().today)).toEqual(['b', 'c', 'a'])
  })

  it('in the same list upwards', () => {
    const w = world({ tabs: ['a', 'b', 'c'], spaces: [{ today: [tab('a'), tab('b'), tab('c')] }] })
    w.model.lists.moveItem({ kind: 'tab', tabId: 'c' }, today(0))
    expect(shape(w.space().today)).toEqual(['c', 'a', 'b'])
  })

  it('pinning: the tab keeps its address as its "home" address', () => {
    const w = world({ tabs: ['a'], spaces: [{ today: [tab('a')] }] })
    w.model.lists.moveItem({ kind: 'tab', tabId: 'a' }, { zone: 'pinned', spaceId: 's1', index: 0 })
    expect(shape(w.space().pinned)).toEqual(['a'])
    expect(w.state.tabs.a.pinnedUrl).toBe('https://a.test/')
  })

  it('unpinning: the "home" address goes away', () => {
    const w = world({ tabs: { a: { pinnedUrl: 'https://home.test/' } }, spaces: [{ pinned: [tab('a')] }] })
    w.model.lists.moveItem({ kind: 'tab', tabId: 'a' }, today(0))
    expect(shape(w.space().today)).toEqual(['a'])
    expect(w.state.tabs.a.pinnedUrl).toBeUndefined()
  })

  it('to favorites', () => {
    const w = world({ tabs: ['a', 'f'], favorites: ['f'], spaces: [{ today: [tab('a')] }] })
    w.model.lists.moveItem({ kind: 'tab', tabId: 'a' }, { zone: 'favorites', spaceId: 's1', index: 1 })
    expect(w.state.favorites).toEqual(['f', 'a'])
    expect(w.state.tabs.a.pinnedUrl).toBeDefined()
  })

  it('reorder favorites', () => {
    const w = world({ tabs: ['x', 'y', 'z'], favorites: ['x', 'y', 'z'] })
    w.model.lists.moveItem({ kind: 'tab', tabId: 'x' }, { zone: 'favorites', spaceId: 's1', index: 3 })
    expect(w.state.favorites).toEqual(['y', 'z', 'x'])
  })

  it('to another space: views are rechecked, the original active tab is replaced', () => {
    const w = world({ tabs: ['a', 'b'], spaces: [{ today: [tab('a'), tab('b')], activeTabId: 'a' }, {}] })
    w.model.lists.moveItem({ kind: 'tab', tabId: 'a' }, { zone: 'today', spaceId: 's2', index: 0 })
    expect(shape(w.space('s2').today)).toEqual(['a'])
    expect(w.effects.reconciled).toEqual([['a']])
    expect(w.space('s1').activeTabId).toBe('b')
  })

  it('into a folder', () => {
    const w = world({ tabs: ['a', 'in'], spaces: [{ pinned: [folder('f', [tab('in')])], today: [tab('a')] }] })
    w.model.lists.moveItem({ kind: 'tab', tabId: 'a' }, { zone: 'pinned', spaceId: 's1', folderId: 'f', index: 0 })
    expect(shape(w.space().pinned)).toEqual(['[f: a in]'])
  })

  it('a live folder receives nothing', () => {
    const w = world({ tabs: ['a'], spaces: [{ pinned: [folder('live', [], { live: { source: { kind: 'feed', url: 'x' } } })], today: [tab('a')] }] })
    expect(w.model.lists.moveItem({ kind: 'tab', tabId: 'a' }, { zone: 'pinned', spaceId: 's1', folderId: 'live', index: 0 })).toBe(false)
    expect(shape(w.space().today)).toEqual(['a'])
  })

  it('into a group (`groupId`): the tab joins the group', () => {
    const w = world({ tabs: ['a', 'b', 'c'], spaces: [{ today: [tab('a'), tab('b', 'g'), tab('c', 'g')] }] })
    w.model.lists.moveItem({ kind: 'tab', tabId: 'a' }, today(2, { groupId: 'g' }))
    expect(shape(w.space().today)).toEqual(['b^g', 'a^g', 'c^g'])
  })

  it('cible inconnue ou onglet inconnu : faux', () => {
    const w = world({ tabs: ['a'], spaces: [{ today: [tab('a')] }] })
    expect(w.model.lists.moveItem({ kind: 'tab', tabId: 'x' }, today(0))).toBe(false)
    expect(w.model.lists.moveItem({ kind: 'tab', tabId: 'a' }, { zone: 'pinned', spaceId: 's1', folderId: 'absent', index: 0 })).toBe(false)
  })
})

describe('lists.moveItem: split views, folders, groups', () => {
  it('a split view moves with all its tabs', () => {
    const w = world({ tabs: ['a', 'b', 'c'], spaces: [{ today: [split('sp', ['a', 'b']), tab('c')] }] })
    w.model.lists.moveItem({ kind: 'split', id: 'sp' }, { zone: 'pinned', spaceId: 's1', index: 0 })
    expect(shape(w.space().pinned)).toEqual(['{sp: a b}'])
    expect(w.state.tabs.a.pinnedUrl).toBeDefined()
    expect(w.state.tabs.b.pinnedUrl).toBeDefined()
  })

  it('moving a tab out of a split view leaves it in place', () => {
    const w = world({ tabs: ['a', 'b', 'c'], spaces: [{ today: [split('sp', ['a', 'b', 'c'])] }] })
    w.model.lists.moveItem({ kind: 'tab', tabId: 'a' }, today(1))
    expect(shape(w.space().today)).toEqual(['{sp: b c}', 'a'])
  })

  it('a folder cannot be dropped into itself or a descendant', () => {
    const w = world({ tabs: ['a'], spaces: [{ pinned: [folder('f', [folder('g', [tab('a')])])] }] })
    expect(w.model.lists.moveItem({ kind: 'folder', id: 'f' }, { zone: 'pinned', spaceId: 's1', folderId: 'g', index: 0 })).toBe(false)
    expect(w.model.lists.moveItem({ kind: 'folder', id: 'f' }, { zone: 'pinned', spaceId: 's1', folderId: 'f', index: 0 })).toBe(false)
  })

  it('a folder reorders among the pinned items', () => {
    const w = world({ tabs: ['a', 'b'], spaces: [{ pinned: [folder('f', [tab('a')]), tab('b')] }] })
    w.model.lists.moveItem({ kind: 'folder', id: 'f' }, { zone: 'pinned', spaceId: 's1', index: 2 })
    expect(shape(w.space().pinned)).toEqual(['b', '[f: a]'])
  })

  it('a folder goes neither to favorites nor to the day list (such a drag is the unpin)', () => {
    const w = world({ tabs: ['a'], spaces: [{ pinned: [folder('f', [tab('a')])] }] })
    expect(w.model.lists.moveItem({ kind: 'folder', id: 'f' }, { zone: 'favorites', spaceId: 's1', index: 0 })).toBe(false)
  })

  it('a split view does not go into favorites', () => {
    const w = world({ tabs: ['a', 'b'], spaces: [{ today: [split('sp', ['a', 'b'])] }] })
    expect(w.model.lists.moveItem({ kind: 'split', id: 'sp' }, { zone: 'favorites', spaceId: 's1', index: 0 })).toBe(false)
  })

  it('a group moves as a block, never into the middle of another group', () => {
    const w = world({
      tabs: ['a', 'b', 'c', 'd', 'e'],
      spaces: [{ today: [tab('a', 'g'), tab('b', 'g'), tab('c', 'h'), tab('d', 'h'), tab('e')], groups: { g: { name: 'g', hue: 1, open: true } } }]
    })
    w.model.lists.moveItem({ kind: 'group', id: 'g' }, today(3)) // in the middle of h: lands after it
    expect(shape(w.space().today)).toEqual(['c^h', 'd^h', 'a^g', 'b^g', 'e'])
  })

  it('a group changes space with its header', () => {
    const w = world({
      tabs: ['a', 'b'],
      spaces: [{ today: [tab('a', 'g'), tab('b', 'g')], groups: { g: { name: 'Mon groupe', hue: 1, open: true } } }, {}]
    })
    w.model.lists.moveItem({ kind: 'group', id: 'g' }, { zone: 'today', spaceId: 's2', index: 0 })
    expect(w.space('s1').groups?.g).toBeUndefined()
    expect(w.space('s2').groups?.g?.name).toBe('Mon groupe')
    expect(w.effects.reconciled).toEqual([['a', 'b']])
  })
})

describe('lists.pinGroup et unpinFolder', () => {
  it('pinning a group: a folder that takes over its name, tint and state', () => {
    const w = world({
      tabs: ['a', 'b'],
      spaces: [{ today: [tab('a', 'g'), tab('b', 'g')], groups: { g: { name: 'Lecture', hue: 33, open: false, icon: '📚' } } }]
    })
    expect(w.model.lists.pinGroup('g')).toBe(true)
    expect(w.space().today).toEqual([])
    expect(w.space().pinned[0]).toMatchObject({ kind: 'folder', name: 'Lecture', hue: 33, open: false, icon: '📚' })
    expect(shape(w.space().pinned)).toEqual([expect.stringMatching(/^\[id\d+: a b\]$/)])
    expect(w.state.tabs.a.pinnedUrl).toBe('https://a.test/')
  })

  it('without a group name: the title of the first tab, otherwise the default name', () => {
    const w = world({ tabs: { a: { title: 'Titre A' } }, spaces: [{ today: [tab('a', 'g'), tab('a', 'g')] }] })
    w.model.lists.pinGroup('g')
    expect((w.space().pinned[0] as { name: string }).name).toBe('Titre A')
  })

  it('groupe inconnu ou dossier cible inconnu : faux', () => {
    const w = world({ tabs: ['a'], spaces: [{ today: [tab('a', 'g')] }] })
    expect(w.model.lists.pinGroup('zz')).toBe(false)
    expect(w.model.lists.pinGroup('g', { zone: 'pinned', spaceId: 's1', folderId: 'absent', index: 0 })).toBe(false)
  })

  it('unpinning a folder: a group at the top of the day tabs, without a "home" address', () => {
    const w = world({
      tabs: { a: { pinnedUrl: 'x' }, b: { pinnedUrl: 'y' } },
      spaces: [{ pinned: [folder('f', [tab('a'), folder('sub', [tab('b')])], { hue: 12, open: false })], today: [] }]
    })
    expect(w.model.lists.unpinFolder('f')).toBe(true)
    expect(w.space().pinned).toEqual([])
    const [a, b] = w.space().today as { tabId: string; group: string }[]
    expect([a.tabId, b.tabId]).toEqual(['a', 'b'])
    expect(a.group).toBe(b.group)
    expect(w.space().groups?.[a.group]).toMatchObject({ name: 'f', hue: 12, open: false })
    expect(w.state.tabs.a.pinnedUrl).toBeUndefined()
  })

  it('dossier vide ou inconnu : faux', () => {
    const w = world({ spaces: [{ pinned: [folder('f', [])] }] })
    expect(w.model.lists.unpinFolder('f')).toBe(false)
    expect(w.model.lists.unpinFolder('zz')).toBe(false)
  })
})

describe('lists : commandes', () => {
  it('togglePin pins then unpins', () => {
    const w = world({ tabs: ['a', 'p'], spaces: [{ pinned: [tab('p')], today: [tab('a')] }] })
    expect(w.model.lists.togglePin('a')).toBe(true)
    expect(shape(w.space().pinned)).toEqual(['p', 'a'])
    expect(w.model.lists.togglePin('a')).toBe(true)
    expect(shape(w.space().today)).toEqual(['a'])
  })

  it('togglePin on a split view moves the whole view', () => {
    const w = world({ tabs: ['a', 'b'], spaces: [{ today: [split('sp', ['a', 'b'])] }] })
    w.model.lists.togglePin('a')
    expect(shape(w.space().pinned)).toEqual(['{sp: a b}'])
  })

  it('toggleFavorite favorites then puts back at the top of the day tabs', () => {
    const w = world({ tabs: ['a', 'b'], spaces: [{ today: [tab('a'), tab('b')] }] })
    w.model.lists.toggleFavorite('b')
    expect(w.state.favorites).toEqual(['b'])
    w.model.lists.toggleFavorite('b')
    expect(w.state.favorites).toEqual([])
    expect(shape(w.space().today)).toEqual(['b', 'a'])
  })

  it('moveTabStep: moves up or down one rank, not beyond the edges', () => {
    const w = world({ tabs: ['a', 'b', 'c'], spaces: [{ today: [tab('a'), tab('b'), tab('c')] }] })
    expect(w.model.lists.moveTabStep('b', -1)).toBe(true)
    expect(shape(w.space().today)).toEqual(['b', 'a', 'c'])
    expect(w.model.lists.moveTabStep('b', -1)).toBe(false)
    expect(w.model.lists.moveTabStep('c', 1)).toBe(false)
    expect(w.model.lists.moveTabStep('a', 1)).toBe(true)
    expect(shape(w.space().today)).toEqual(['b', 'c', 'a'])
  })

  it('moveTabStep in a folder stays in the folder', () => {
    const w = world({ tabs: ['a', 'b'], spaces: [{ pinned: [folder('f', [tab('a'), tab('b')])] }] })
    w.model.lists.moveTabStep('b', -1)
    expect(shape(w.space().pinned)).toEqual(['[f: b a]'])
  })

  it('moveTabToSpace: at the top of the day tabs of the target space', () => {
    const w = world({ tabs: ['a', 'x'], spaces: [{ today: [tab('a')] }, { today: [tab('x')] }] })
    w.model.lists.moveTabToSpace('a', 's2')
    expect(shape(w.space('s2').today)).toEqual(['a', 'x'])
  })
})

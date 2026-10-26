import { describe, expect, it } from 'vitest'
import { shape, split, tab, world } from './testkit'

const sp = (w: ReturnType<typeof world>, i = 0) => w.space().today[i] as Extract<ReturnType<typeof split>, { kind: 'split' }>

describe('splits.canSplit', () => {
  it('true for a tab or a split view of fewer than 4 tabs', () => {
    const w = world({ tabs: ['a', 'b', 'c', 'd'], spaces: [{ today: [tab('a'), split('sp', ['b', 'c', 'd'])], activeTabId: 'a' }] })
    expect(w.model.splits.canSplit()).toBe(true)
    w.space().activeTabId = 'b'
    expect(w.model.splits.canSplit()).toBe(true)
  })

  it('false for a full split view, a favorite, or with no active tab', () => {
    const w = world({ tabs: ['a', 'b', 'c', 'd', 'f'], favorites: ['f'], spaces: [{ today: [split('sp', ['a', 'b', 'c', 'd'])], activeTabId: 'a' }] })
    expect(w.model.splits.canSplit()).toBe(false)
    w.space().activeTabId = 'f'
    expect(w.model.splits.canSplit()).toBe(false)
    w.space().activeTabId = null
    expect(w.model.splits.canSplit()).toBe(false)
  })
})

describe('splits.splitWith', () => {
  it('joins the active tab and another into a split view, on the right by default', () => {
    const w = world({ tabs: ['a', 'b', 'c'], spaces: [{ today: [tab('a'), tab('b'), tab('c')], activeTabId: 'a' }] })
    expect(w.model.splits.splitWith('c')).toBe(true)
    expect(shape(w.space().today)).toEqual([expect.stringMatching(/^\{id\d+: a c\}$/), 'b'])
  })

  it("`edge: left` place l'autre onglet avant", () => {
    const w = world({ tabs: ['a', 'b'], spaces: [{ today: [tab('a'), tab('b')], activeTabId: 'a' }] })
    w.model.splits.splitWith('b', 'left')
    expect((w.space().today[0] as { tabIds: string[] }).tabIds).toEqual(['b', 'a'])
  })

  it('keeps the group of the created split view', () => {
    const w = world({ tabs: ['a', 'b'], spaces: [{ today: [tab('a', 'g'), tab('b')], activeTabId: 'a' }] })
    w.model.splits.splitWith('b')
    expect(sp(w)).toMatchObject({ kind: 'split', group: 'g' })
  })

  it('adds to an existing split view (sizes reset)', () => {
    const w = world({
      tabs: ['a', 'b', 'c'],
      spaces: [{ today: [{ kind: 'split', id: 'sp', tabIds: ['a', 'b'], sizes: [0.7, 0.3] }, tab('c')], activeTabId: 'a' }]
    })
    w.model.splits.splitWith('c')
    expect(sp(w).tabIds).toEqual(['a', 'b', 'c'])
    expect(sp(w)).not.toHaveProperty('sizes')
  })

  it('a tab of the pinned space joins a pinned tab: it gets its "home" address', () => {
    const w = world({ tabs: ['p', 'a'], spaces: [{ pinned: [tab('p')], today: [tab('a')], activeTabId: 'p' }] })
    w.model.splits.splitWith('a')
    expect(w.state.tabs.a.pinnedUrl).toBe('https://a.test/')
    expect(w.space().today).toEqual([])
  })

  it('a day tab that joins a day view loses its "home" address', () => {
    const w = world({ tabs: { a: {}, p: { pinnedUrl: 'x' } }, spaces: [{ pinned: [tab('p')], today: [tab('a')], activeTabId: 'a' }] })
    w.model.splits.splitWith('p')
    expect(w.state.tabs.p.pinnedUrl).toBeUndefined()
  })

  it('refusal: itself, a favorite, a tab already in the view, a detached tab, a full view', () => {
    const w = world({
      tabs: ['a', 'b', 'f', 'peek', 'c', 'd', 'e'],
      favorites: ['f'],
      spaces: [{ today: [split('sp', ['a', 'b']), tab('c'), tab('d'), tab('e')], activeTabId: 'a' }]
    })
    w.data.detached.add('peek')
    expect(w.model.splits.splitWith('a')).toBe(false)
    expect(w.model.splits.splitWith('f')).toBe(false)
    expect(w.model.splits.splitWith('b')).toBe(false)
    expect(w.model.splits.splitWith('peek')).toBe(false)
    expect(w.model.splits.splitWith('zz')).toBe(false)
    w.model.splits.splitWith('c')
    w.model.splits.splitWith('d')
    expect(w.model.splits.splitWith('e')).toBe(false) // 4 tabs at most
  })

  it('the joining tab requests a session check', () => {
    const w = world({ tabs: ['a', 'b', 'c'], spaces: [{ today: [tab('a'), tab('b'), tab('c')], activeTabId: 'a' }] })
    w.model.splits.splitWith('b')
    expect(w.effects.reconciled).toEqual([['b']])
  })
})

describe('splits: undo and adjust', () => {
  it("unsplit: the tabs stay in the view's place, with its group", () => {
    const w = world({ tabs: ['a', 'b'], spaces: [{ today: [split('sp', ['a', 'b'], 'g')] }] })
    expect(w.model.splits.unsplit('sp')).toBe(true)
    expect(shape(w.space().today)).toEqual(['a^g', 'b^g'])
    expect(w.model.splits.unsplit('sp')).toBe(false)
  })

  it('removeFromSplit: the tab leaves and is placed right after the view', () => {
    const w = world({ tabs: ['a', 'b', 'c'], spaces: [{ today: [split('sp', ['a', 'b', 'c'])] }] })
    expect(w.model.splits.removeFromSplit('a')).toBe(true)
    expect(shape(w.space().today)).toEqual(['{sp: b c}', 'a'])
    expect(w.model.splits.removeFromSplit('a')).toBe(false)
  })

  it('removeFromSplit in a group: the removed tab stays in the group', () => {
    const w = world({ tabs: ['a', 'b', 'c'], spaces: [{ today: [split('sp', ['a', 'b', 'c'], 'g')] }] })
    w.model.splits.removeFromSplit('a')
    expect(shape(w.space().today)).toEqual(['{sp: b c}^g', 'a^g'])
  })

  it('setLayout: layout set, sizes reset; the grid requires 3 tabs', () => {
    const w = world({ tabs: ['a', 'b', 'c'], spaces: [{ today: [{ kind: 'split', id: 'sp', tabIds: ['a', 'b'], sizes: [0.6, 0.4] }, tab('c')] }] })
    expect(w.model.splits.setLayout('sp', 'rows')).toBe(true)
    expect(sp(w)).toMatchObject({ layout: 'rows' })
    expect(sp(w)).not.toHaveProperty('sizes')
    expect(w.model.splits.setLayout('sp', 'grid')).toBe(false)
    expect(w.model.splits.setLayout('zz', 'rows')).toBe(false)
  })

  it('setSizes: normalized sizes (sum 1), invalid values reset to 0', () => {
    const w = world({ tabs: ['a', 'b'], spaces: [{ today: [split('sp', ['a', 'b'])] }] })
    expect(w.model.splits.setSizes('sp', [300, 100])).toBe(true)
    const sizes = sp(w).sizes!
    expect(sizes.reduce((x, y) => x + y, 0)).toBeCloseTo(1)
    expect(sizes[0]).toBeGreaterThan(sizes[1])
    expect(w.model.splits.setSizes('zz', [1, 1])).toBe(false)
    expect(w.model.splits.setSizes('sp', 'oups' as unknown as number[])).toBe(false)
  })
})

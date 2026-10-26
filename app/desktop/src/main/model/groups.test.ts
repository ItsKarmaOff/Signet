import { describe, expect, it } from 'vitest'
import { shape, tab, world } from './testkit'

const header = (name = 'Groupe') => ({ name, hue: 10, open: true })

describe('groups: header', () => {
  const setup = () => world({ tabs: ['a', 'b'], spaces: [{ today: [tab('a', 'g'), tab('b', 'g')], groups: { g: header() } }] })

  it('header finds the header in its space', () => {
    const w = setup()
    expect(w.model.groups.header('g')?.name).toBe('Groupe')
    expect(w.model.groups.header('zz')).toBeUndefined()
  })

  it('rename cleans the name and rejects empty', () => {
    const w = setup()
    expect(w.model.groups.rename('g', '  Lecture ')).toBe(true)
    expect(w.model.groups.header('g')?.name).toBe('Lecture')
    expect(w.model.groups.rename('g', ' ')).toBe(false)
    expect(w.model.groups.rename('zz', 'x')).toBe(false)
  })

  it('tint, emoji, fallback', () => {
    const w = setup()
    w.model.groups.setHue('g', 99)
    w.model.groups.setIcon('g', '🎵')
    w.model.groups.toggle('g')
    expect(w.model.groups.header('g')).toMatchObject({ hue: 99, icon: '🎵', open: false })
    w.model.groups.setIcon('g', undefined)
    w.model.groups.toggle('g', true)
    expect(w.model.groups.header('g')).not.toHaveProperty('icon')
    expect(w.model.groups.header('g')?.open).toBe(true)
  })
})

describe('groups.leave et ungroup', () => {
  it('leave: the tab is placed right after the group, ungrouped', () => {
    const w = world({ tabs: ['a', 'b', 'c', 'd'], spaces: [{ today: [tab('a', 'g'), tab('b', 'g'), tab('c', 'g'), tab('d')] }] })
    expect(w.model.groups.leave('a')).toBe(true)
    expect(shape(w.space().today)).toEqual(['b^g', 'c^g', 'a', 'd'])
  })

  it('leave: an ungrouped tab, or a favorite, does not move', () => {
    const w = world({ tabs: ['a', 'f'], favorites: ['f'], spaces: [{ today: [tab('a')] }] })
    expect(w.model.groups.leave('a')).toBe(false)
    expect(w.model.groups.leave('f')).toBe(false)
  })

  it('ungroup: the tabs stay in place, unlinked', () => {
    const w = world({ tabs: ['a', 'b'], spaces: [{ today: [tab('a', 'g'), tab('b', 'g')] }] })
    w.model.groups.ungroup('g')
    expect(shape(w.space().today)).toEqual(['a', 'b'])
  })
})

describe('groups.close', () => {
  const setup = () =>
    world({
      tabs: ['a', 'b', 'c', 'd'],
      spaces: [{ today: [tab('d'), tab('a', 'g'), tab('b', 'g'), tab('c')], groups: { g: header('Mon groupe') }, activeTabId: 'a' }]
    })

  it("destroys the group's tabs, stacks them in one batch (header on the first) and picks a neighbor", () => {
    const w = setup()
    const removed = w.model.groups.close(w.space(), 'g')!
    expect(removed.count).toBe(2)
    expect(shape(w.space().today)).toEqual(['d', 'c'])
    expect(Object.keys(w.state.tabs).sort()).toEqual(['c', 'd'])
    expect(w.effects.destroyed).toEqual(['a', 'b'])
    expect(w.data.closed).toHaveLength(2)
    expect(w.data.closed[0].group?.name).toBe('Mon groupe')
    expect(w.data.closed[0].url).toBe('https://a.test/')
    expect(w.space().activeTabId).toBe('c')
  })

  it('undo puts back the group, its header, the active tab and empties the stack', () => {
    const w = setup()
    const removed = w.model.groups.close(w.space(), 'g')!
    expect(removed.restore()).toBe(true)
    expect(shape(w.space().today)).toEqual(['d', 'a^g', 'b^g', 'c'])
    expect(w.space().groups?.g?.name).toBe('Mon groupe')
    expect(w.space().activeTabId).toBe('a')
    expect(w.data.closed).toEqual([])
    expect(Object.keys(w.state.tabs).sort()).toEqual(['a', 'b', 'c', 'd'])
  })

  it('unknown group: null, and no batch number consumed', () => {
    const w = setup()
    expect(w.model.groups.close(w.space(), 'zz')).toBeNull()
    expect(w.model.closed.nextBatch()).toBe(1)
  })
})

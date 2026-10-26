import { describe, expect, it } from 'vitest'
import { shape, split, tab, world } from './testkit'

describe('opening.open', () => {
  it('by default: at the top of the day tabs', () => {
    const w = world({ tabs: ['a', 'b'], spaces: [{ today: [tab('a'), tab('b')] }] })
    const id = w.model.opening.open('https://x.test/')
    expect(shape(w.space().today)).toEqual([id, 'a', 'b'])
    expect(w.state.tabs[id].url).toBe('https://x.test/')
    expect(w.space().activeTabId).toBeNull()
  })

  it('"at the bottom" setting: at the end', () => {
    const w = world({ tabs: ['a'], spaces: [{ today: [tab('a')] }], settings: { newTabPosition: 'bottom' } })
    const id = w.model.opening.open('https://x.test/')
    expect(shape(w.space().today)).toEqual(['a', id])
  })

  it("`activate` : l'onglet devient l'actif de l'espace", () => {
    const w = world({ spaces: [{}] })
    const id = w.model.opening.open('https://x.test/', { activate: true })
    expect(w.space().activeTabId).toBe(id)
    expect(w.effects.activated).toEqual([['s1', id]])
  })

  it('`afterTabId`: right after that day tab', () => {
    const w = world({ tabs: ['a', 'b', 'c'], spaces: [{ today: [tab('a'), tab('b'), tab('c')] }] })
    const id = w.model.opening.open('https://x.test/', { afterTabId: 'b' })
    expect(shape(w.space().today)).toEqual(['a', 'b', id, 'c'])
  })

  it('`afterTabId` of a pinned tab: falls back to the setting', () => {
    const w = world({ tabs: ['p', 'a'], spaces: [{ pinned: [tab('p')], today: [tab('a')] }] })
    const id = w.model.opening.open('https://x.test/', { afterTabId: 'p' })
    expect(shape(w.space().today)).toEqual([id, 'a'])
  })

  describe('from a day tab (`openerId`)', () => {
    it('the opener and the new tab form a group, the new one following', () => {
      const w = world({ tabs: ['a', 'b', 'c'], spaces: [{ today: [tab('a'), tab('b'), tab('c')] }] })
      const id = w.model.opening.open('https://x.test/', { openerId: 'b' })
      const group = (w.space().today[1] as { group?: string }).group!
      expect(group).toBeTruthy()
      expect(shape(w.space().today)).toEqual(['a', `b^${group}`, `${id}^${group}`, 'c'])
    })

    it('in an existing group: at the end of the group', () => {
      const w = world({ tabs: ['a', 'b', 'c'], spaces: [{ today: [tab('a', 'g'), tab('b', 'g'), tab('c')] }] })
      const id = w.model.opening.open('https://x.test/', { openerId: 'a' })
      expect(shape(w.space().today)).toEqual(['a^g', 'b^g', `${id}^g`, 'c'])
    })

    it('opener in a split view: the split view carries the group', () => {
      const w = world({ tabs: ['a', 'b'], spaces: [{ today: [split('sp', ['a', 'b'])] }] })
      const id = w.model.opening.open('https://x.test/', { openerId: 'a' })
      const sp = w.space().today[0] as { group?: string }
      expect(sp.group).toBeTruthy()
      expect(shape(w.space().today)).toEqual([`{sp: a b}^${sp.group}`, `${id}^${sp.group}`])
    })

    it('opener pinned or in another space: ordinary placement', () => {
      const w = world({ tabs: ['p', 'o', 'a'], spaces: [{ pinned: [tab('p')], today: [tab('a')] }, { today: [tab('o')] }] })
      const id1 = w.model.opening.open('https://x.test/1', { openerId: 'p' })
      const id2 = w.model.opening.open('https://x.test/2', { openerId: 'o' })
      expect(shape(w.space().today)).toEqual([id2, id1, 'a'])
    })
  })
})

describe('opening.openIn', () => {
  it('opens in the target space, makes it active and activates the tab', () => {
    const w = world({ tabs: ['a', 'b'], spaces: [{ today: [tab('a')] }, { today: [tab('b')] }] })
    const id = w.model.opening.openIn(w.space('s2'), 'https://x.test/')
    expect(shape(w.space('s2').today)).toEqual([id, 'b'])
    expect(w.state.activeSpaceId).toBe('s2')
    expect(w.space('s2').activeTabId).toBe(id)
    expect(shape(w.space('s1').today)).toEqual(['a'])
  })
})

describe('opening.adopt', () => {
  it('a detached tab takes a spot among the day tabs, becomes active and checks its session', () => {
    const w = world({ tabs: ['a', 'peek'], spaces: [{ today: [tab('a')] }] })
    w.data.detached.add('peek')
    w.model.opening.adopt('peek')
    expect(w.data.detached.has('peek')).toBe(false)
    expect(shape(w.space().today)).toEqual(['peek', 'a'])
    expect(w.space().activeTabId).toBe('peek')
    expect(w.effects.reconciled).toEqual([['peek']])
  })
})

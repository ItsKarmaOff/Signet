import { describe, expect, it } from 'vitest'
import { folder, shape, split, tab, world } from './testkit'

describe('removal.close', () => {
  it('a day tab: stacked for ⇧⌘T, destroyed, deleted, the neighbor becomes active', () => {
    const w = world({ tabs: ['a', 'b', 'c'], spaces: [{ today: [tab('a'), tab('b'), tab('c')], activeTabId: 'b' }] })
    expect(w.model.removal.close('b')).toBe(true)
    expect(shape(w.space().today)).toEqual(['a', 'c'])
    expect(w.state.tabs.b).toBeUndefined()
    expect(w.space().activeTabId).toBe('c')
    expect(w.data.closed).toHaveLength(1)
    expect(w.effects.destroyed).toEqual(['b'])
  })

  it('the view is destroyed before the tab leaves the state', () => {
    const w = world({ tabs: ['a'], spaces: [{ today: [tab('a')] }] })
    let existedAtDestroy = false
    const model = w.model
    w.effects.destroyed.push = (id: string) => {
      existedAtDestroy = id in w.state.tabs
      return 1
    }
    model.removal.close('a')
    expect(existedAtDestroy).toBe(true)
  })

  it('the last active tab closed: the space has no active tab anymore', () => {
    const w = world({ tabs: ['a'], spaces: [{ today: [tab('a')], activeTabId: 'a' }] })
    w.model.removal.close('a')
    expect(w.space().activeTabId).toBeNull()
  })

  it('onglet inconnu : faux', () => {
    expect(world().model.removal.close('x')).toBe(false)
  })

  it('a pinned tab is only unloaded and reset to its "home" address', () => {
    const w = world({
      tabs: { p: { pinnedUrl: 'https://home.test/', url: 'https://ailleurs.test/' }, q: { pinnedUrl: 'https://q.test/' }, a: {} },
      spaces: [{ pinned: [tab('p'), tab('q')], today: [tab('a')], activeTabId: 'p' }]
    })
    expect(w.model.removal.close('p')).toBe(true)
    expect(shape(w.space().pinned)).toEqual(['p', 'q'])
    expect(w.state.tabs.p.url).toBe('https://home.test/')
    expect(w.effects.destroyed).toEqual(['p'])
    expect(w.data.closed).toEqual([])
    expect(w.space().activeTabId).toBe('q') // the next one among the pinned items
  })

  it('closing the only active pinned tab does not pick a day tab (original behavior)', () => {
    const w = world({ tabs: { p: { pinnedUrl: 'https://home.test/' }, a: {} }, spaces: [{ pinned: [tab('p')], today: [tab('a')], activeTabId: 'p' }] })
    w.model.removal.close('p')
    expect(w.space().activeTabId).toBeNull()
  })

  it('a pinned tab of a split view leaves it first', () => {
    const w = world({
      tabs: { p: { pinnedUrl: 'https://home.test/' }, q: { pinnedUrl: 'https://q.test/' } },
      spaces: [{ pinned: [split('sp', ['p', 'q'])], activeTabId: 'p' }]
    })
    w.model.removal.close('p')
    expect(shape(w.space().pinned)).toEqual(['q', 'p'])
  })
})

describe('removal.remove et drop', () => {
  it('remove: the view goes, the data too, but nothing is stacked', () => {
    const w = world({ tabs: ['a', 'b'], spaces: [{ today: [tab('a'), tab('b')], activeTabId: 'a' }] })
    w.data.histories.set('a', { entries: [], index: 0 })
    w.model.removal.remove('a')
    expect(w.state.tabs.a).toBeUndefined()
    expect(w.data.histories.has('a')).toBe(false)
    expect(w.data.closed).toEqual([])
    expect(w.space().activeTabId).toBe('b')
  })

  it("remove on a tab of a split view: the other stays alone in the view's place", () => {
    const w = world({ tabs: ['a', 'b', 'c'], spaces: [{ today: [split('sp', ['a', 'b']), tab('c')], activeTabId: 'a' }] })
    w.model.removal.remove('a')
    expect(shape(w.space().today)).toEqual(['b', 'c'])
    expect(w.space().activeTabId).toBe('b')
  })

  it('drop: destroys each view then deletes tabs and histories', () => {
    const w = world({ tabs: ['a', 'b'] })
    w.data.histories.set('b', { entries: [], index: 0 })
    w.model.removal.drop(['a', 'b'])
    expect(w.effects.destroyed).toEqual(['a', 'b'])
    expect(Object.keys(w.state.tabs)).toEqual([])
    expect(w.data.histories.size).toBe(0)
  })

  it('a pinned folder: closing a tab inside it puts it on the stack with the folder', () => {
    const w = world({ tabs: ['a'], spaces: [{ pinned: [folder('f', [tab('a')])] }] })
    w.state.tabs.a.pinnedUrl = undefined
    w.model.removal.close('a')
    expect(w.data.closed[0]).toMatchObject({ folderId: 'f' })
  })
})

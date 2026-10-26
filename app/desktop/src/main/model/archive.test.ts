import { describe, expect, it } from 'vitest'
import { ARCHIVE_MAX } from './types'
import { shape, split, tab, world } from './testkit'

const HOUR = 3_600_000

describe('archive.archiveTab', () => {
  it('puts the tab at the head of the archive then deletes it', () => {
    const w = world({ tabs: { a: { title: 'A', favicon: 'ico' }, b: {} }, spaces: [{ today: [tab('a'), tab('b')], activeTabId: 'a' }] })
    const entry = w.model.archive.archiveTab('a')!
    expect(entry).toMatchObject({ url: 'https://a.test/', title: 'A', favicon: 'ico', archivedAt: w.clock.now })
    expect(w.state.archive[0]).toBe(entry)
    expect(shape(w.space().today)).toEqual(['b'])
    expect(w.state.tabs.a).toBeUndefined()
    expect(w.space().activeTabId).toBe('b')
  })

  it(`the archive is capped at ${ARCHIVE_MAX} entries`, () => {
    const w = world({ tabs: ['a'], spaces: [{ today: [tab('a')] }] })
    w.state.archive = Array.from({ length: ARCHIVE_MAX }, (_, i) => ({ url: `u${i}`, title: '', archivedAt: 0 }))
    w.model.archive.archiveTab('a')
    expect(w.state.archive).toHaveLength(ARCHIVE_MAX)
    expect(w.state.archive[0].url).toBe('https://a.test/')
  })

  it('unknown tab: null', () => {
    expect(world().model.archive.archiveTab('x')).toBeNull()
  })
})

describe('archive.clearToday', () => {
  const setup = () =>
    world({
      tabs: ['a', 'b', 'c', 'p'],
      spaces: [{ pinned: [tab('p')], today: [tab('a', 'g'), tab('b', 'g'), tab('c')], groups: { g: { name: 'G', hue: 1, open: true } }, activeTabId: 'b' }]
    })

  it('archives all today tabs, not the pinned ones', () => {
    const w = setup()
    const cleared = w.model.archive.clearToday()!
    expect(cleared.count).toBe(3)
    expect(w.space().today).toEqual([])
    expect(w.state.archive).toHaveLength(3)
    expect(Object.keys(w.state.tabs)).toEqual(['p'])
    expect(w.space().activeTabId).toBe('p')
    expect(w.effects.saved).toContain('a')
  })

  it('nothing to archive: null', () => {
    expect(world({ spaces: [{}] }).model.archive.clearToday()).toBeNull()
  })

  it('undo puts back the tabs, their groups, the active one, and empties the archive of those entries', () => {
    const w = setup()
    w.data.histories.set('a', { entries: [{ url: 'https://a.test/', title: 'a' }], index: 0 })
    const cleared = w.model.archive.clearToday()!
    expect(cleared.restore()).toBe(true)
    expect(shape(w.space().today)).toEqual(['a^g', 'b^g', 'c'])
    expect(w.space().groups?.g?.name).toBe('G')
    expect(w.space().activeTabId).toBe('b')
    expect(w.state.archive).toEqual([])
    expect(w.data.histories.has('a')).toBe(true)
    expect(w.state.tabs.a.loaded).toBe(false)
  })

  it('undo after choosing another active tab: the chosen active tab is kept', () => {
    const w = setup()
    const cleared = w.model.archive.clearToday()!
    w.model.opening.open('https://new.test/', { activate: true })
    const fresh = w.space().activeTabId
    cleared.restore()
    expect(w.space().activeTabId).toBe(fresh)
    expect(w.space().today).toHaveLength(4)
  })

  it('undo when the space is gone: false', () => {
    const w = world({ tabs: ['a'], spaces: [{ today: [tab('a')] }, {}] })
    const cleared = w.model.archive.clearToday()!
    w.state.spaces.shift()
    expect(cleared.restore()).toBe(false)
  })
})

describe('archive.sweep', () => {
  const old = (h: number) => ({ lastActive: 1_000_000 - h * HOUR })
  const never = () => false

  it('archives today tabs older than the delay, keeps recent ones', () => {
    const w = world({ tabs: { a: old(20), b: old(1), c: old(30) }, spaces: [{ today: [tab('a'), tab('b'), tab('c')] }] })
    expect(w.model.archive.sweep(never)).toBe(2)
    expect(shape(w.space().today)).toEqual(['b'])
    expect(w.state.archive.map((e) => e.url).sort()).toEqual(['https://a.test/', 'https://c.test/'])
  })

  it('never active tabs, nor pinned ones', () => {
    const w = world({ tabs: { a: old(40), p: old(40) }, spaces: [{ pinned: [tab('p')], today: [tab('a')], activeTabId: 'a' }] })
    expect(w.model.archive.sweep(never)).toBe(0)
    expect(w.space().today).toHaveLength(1)
  })

  it('setting at 0: never', () => {
    const w = world({ tabs: { a: old(400) }, spaces: [{ today: [tab('a')] }], settings: { archiveAfterHours: 0 } })
    expect(w.model.archive.sweep(never)).toBe(0)
  })

  it('a busy tab (sound, media…) is kept', () => {
    const w = world({ tabs: { a: old(40), b: old(40) }, spaces: [{ today: [tab('a'), tab('b')] }] })
    expect(w.model.archive.sweep((id) => id === 'a')).toBe(1)
    expect(shape(w.space().today)).toEqual(['a'])
  })

  it('a group is archived as a block, only if all its tabs are expired', () => {
    const w = world({
      tabs: { a: old(40), b: old(40), c: old(1), d: old(40) },
      spaces: [{ today: [tab('a', 'g'), tab('b', 'g'), tab('c', 'h'), tab('d', 'h')] }]
    })
    expect(w.model.archive.sweep(never)).toBe(2)
    expect(shape(w.space().today)).toEqual(['c^h', 'd^h'])
  })

  it('a group with an active or busy tab stays whole', () => {
    const w = world({ tabs: { a: old(40), b: old(40) }, spaces: [{ today: [tab('a', 'g'), tab('b', 'g')], activeTabId: 'b' }] })
    expect(w.model.archive.sweep(never)).toBe(0)
  })

  it('a split view with an active tab is kept', () => {
    const w = world({ tabs: { a: old(40), b: old(40) }, spaces: [{ today: [split('sp', ['a', 'b'])], activeTabId: 'a' }] })
    expect(w.model.archive.sweep(never)).toBe(0)
  })

  it('all spaces are swept', () => {
    const w = world({ tabs: { a: old(40), b: old(40) }, spaces: [{ today: [tab('a')] }, { today: [tab('b')] }] })
    expect(w.model.archive.sweep(never)).toBe(2)
  })
})

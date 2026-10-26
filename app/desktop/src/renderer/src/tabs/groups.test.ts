import { describe, expect, it } from 'vitest'
import type { Item } from '@shared/types'
import { aroundRun, groupOf, isBlock, runsOf, todayMark } from './groups'

const tab = (id: string, group?: string): Item => ({ kind: 'tab', tabId: id, group })
const folder = (id: string): Item => ({ kind: 'folder', id, name: id, open: true, items: [] }) as unknown as Item

describe('groupOf', () => {
  it('reads a tab’s group, nothing for a folder or undefined', () => {
    expect(groupOf(tab('a', 'g'))).toBe('g')
    expect(groupOf(tab('a'))).toBeUndefined()
    expect(groupOf(folder('f'))).toBeUndefined()
    expect(groupOf(undefined)).toBeUndefined()
  })
})

describe('runsOf', () => {
  it('liste vide', () => {
    expect(runsOf([])).toEqual([])
  })

  it('single items without a group', () => {
    expect(runsOf([tab('a'), tab('b')])).toEqual([
      { start: 0, end: 1 },
      { start: 1, end: 2 }
    ])
  })

  it('groups consecutive items of the same group', () => {
    expect(runsOf([tab('a'), tab('b', 'g'), tab('c', 'g'), tab('d')])).toEqual([
      { start: 0, end: 1 },
      { group: 'g', start: 1, end: 3 },
      { start: 3, end: 4 }
    ])
  })

  it('a single-member group is treated as a single item', () => {
    expect(runsOf([tab('a', 'g'), tab('b')])).toEqual([
      { start: 0, end: 1 },
      { start: 1, end: 2 }
    ])
  })

  it('two distinct neighboring groups stay separate; one group split in two gives two runs', () => {
    expect(runsOf([tab('a', 'g'), tab('b', 'g'), tab('c', 'h'), tab('d', 'h')])).toEqual([
      { group: 'g', start: 0, end: 2 },
      { group: 'h', start: 2, end: 4 }
    ])
    expect(runsOf([tab('a', 'g'), tab('b'), tab('c', 'g'), tab('d', 'g')])).toEqual([
      { start: 0, end: 1 },
      { start: 1, end: 2 },
      { group: 'g', start: 2, end: 4 }
    ])
  })

  it('a folder breaks a group', () => {
    expect(runsOf([tab('a', 'g'), folder('f'), tab('b', 'g')]).every((r) => r.group === undefined)).toBe(true)
  })
})

describe('todayMark', () => {
  const items = [tab('a'), tab('b', 'g'), tab('c', 'g'), tab('d')]
  const drop = (extra: Record<string, unknown>) => ({ zone: 'today', index: 0, ...extra }) as never

  it('null outside the “today” zone, in a folder, or when entering an item', () => {
    expect(todayMark(items, null)).toBeNull()
    expect(todayMark(items, drop({ zone: 'pinned' }))).toBeNull()
    expect(todayMark(items, drop({ folderId: 'f' }))).toBeNull()
    expect(todayMark(items, drop({ into: 'x' }))).toBeNull()
  })

  it('“before” the targeted item by default', () => {
    expect(todayMark(items, drop({ index: 1 }))).toEqual({ index: 1, side: 'before' })
    expect(todayMark(items, drop({ index: 0 }))).toEqual({ index: 0, side: 'before' })
  })

  it('“after” the last item at the end of the list', () => {
    expect(todayMark(items, drop({ index: 4 }))).toEqual({ index: 3, side: 'after' })
    expect(todayMark([], drop({ index: 0 }))).toEqual({ index: -1, side: 'after' })
  })

  it('end of list with last item in a group, no group targeted: after the card', () => {
    const list = [tab('a'), tab('b', 'g'), tab('c', 'g')]
    expect(todayMark(list, drop({ index: 3 }))).toEqual({ index: 2, side: 'after', card: true })
  })

  it('joining the end of a group: after its last member', () => {
    expect(todayMark(items, drop({ index: 3, groupId: 'g' }))).toEqual({ index: 2, side: 'after' })
  })

  it('groupId targeted but the group continues at the index: before', () => {
    expect(todayMark(items, drop({ index: 2, groupId: 'g' }))).toEqual({ index: 2, side: 'before' })
  })
})

describe('aroundRun', () => {
  // Dummy card 100 px tall at top = 100 (middle 150); 200 px wide at left = 0 (middle 100).
  const card = (live?: { m41: number; m42: number }) => {
    const el = {
      getBoundingClientRect: () => ({ top: 100, left: 0, width: 200, height: 100 }),
      hasAttribute: () => false,
      parentElement: null
    }
    return { el: el as unknown as Element, live }
  }
  const run = { group: 'g', start: 2, end: 5 }

  it('before if the pointer is in the first half, after otherwise', () => {
    const { el } = card()
    expect(aroundRun(el, run, 120, false)).toBe(2)
    expect(aroundRun(el, run, 180, false)).toBe(5)
  })

  it('suit l’axe horizontal', () => {
    const { el } = card()
    expect(aroundRun(el, run, 50, true)).toBe(2)
    expect(aroundRun(el, run, 150, true)).toBe(5)
  })

  it('right in the middle: after (pointer < mid is false)', () => {
    const { el } = card()
    expect(aroundRun(el, run, 150, false)).toBe(5)
  })

  it('4 px dead band: keeps the current target if it touches this group', () => {
    const { el } = card()
    expect(aroundRun(el, run, 148, false, 5)).toBe(5)
    expect(aroundRun(el, run, 152, false, 2)).toBe(2)
    // Outside the dead band or unrelated target: decide.
    expect(aroundRun(el, run, 140, false, 5)).toBe(2)
    expect(aroundRun(el, run, 149, false, 9)).toBe(2)
  })
})

describe('isBlock', () => {
  it('true for a dragged group or folder', () => {
    expect(isBlock({ kind: 'group', id: 'g' })).toBe(true)
    expect(isBlock({ kind: 'folder', id: 'f' })).toBe(true)
    expect(isBlock({ kind: 'tab', tabId: 't' })).toBe(false)
    expect(isBlock({ kind: 'split', id: 's' })).toBe(false)
    expect(isBlock(null)).toBe(false)
  })
})

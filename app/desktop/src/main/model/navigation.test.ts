import { describe, expect, it } from 'vitest'
import { cycleTarget, numberTarget } from './navigation'
import { folder, tab, world } from './testkit'

const setup = () =>
  world({
    tabs: ['f1', 'p1', 'in1', 'in2', 't1', 't2'],
    favorites: ['f1'],
    spaces: [{ pinned: [tab('p1'), folder('f', [tab('in1'), tab('in2')])], today: [tab('t1'), tab('t2')], activeTabId: 't1' }]
  })

describe('cycleTarget', () => {
  it('next and previous in the order favorites, pinned (folders flattened), day', () => {
    const w = setup()
    expect(cycleTarget(w.state, 1)).toBe('t2')
    expect(cycleTarget(w.state, -1)).toBe('in2')
  })

  it('wraps around at both ends', () => {
    const w = setup()
    w.space().activeTabId = 't2'
    expect(cycleTarget(w.state, 1)).toBe('f1')
    w.space().activeTabId = 'f1'
    expect(cycleTarget(w.state, -1)).toBe('t2')
  })

  it('with no known active tab: the first (next) or the last (previous)', () => {
    const w = setup()
    w.space().activeTabId = null
    expect(cycleTarget(w.state, 1)).toBe('f1')
    expect(cycleTarget(w.state, -1)).toBe('t2')
  })

  it('aucun onglet : rien', () => {
    expect(cycleTarget(world().state, 1)).toBeUndefined()
  })
})

describe('numberTarget', () => {
  it('nth visible tab (favorites, pinned, day)', () => {
    const w = setup()
    expect(numberTarget(w.state, 1, false)).toBe('f1')
    expect(numberTarget(w.state, 3, false)).toBe('in1')
    expect(numberTarget(w.state, 6, false)).toBe('t2')
  })

  it('⌘9: the last; out of range: nothing', () => {
    const w = setup()
    expect(numberTarget(w.state, 9, false)).toBe('t2')
    expect(numberTarget(w.state, 8, false)).toBeUndefined()
  })

  it('a collapsed folder hides its tabs', () => {
    const w = setup()
    ;(w.space().pinned[1] as { open: boolean }).open = false
    expect(numberTarget(w.state, 3, false)).toBe('t1')
  })

  it('tabs on top: folders count as collapsed', () => {
    const w = setup()
    expect(numberTarget(w.state, 3, true)).toBe('t1')
  })
})

import { describe, expect, it } from 'vitest'
import { folder, shape, tab, world } from './testkit'

describe('spaces: creation and settings', () => {
  it('create: adds an empty space with cleaned values', () => {
    const w = world()
    const space = w.model.spaces.create({ name: '  Travail ', icon: '💼', hue: 400 })
    expect(w.state.spaces.at(-1)).toBe(space)
    expect(space).toMatchObject({ name: 'Travail', icon: '💼', hue: 40, pinned: [], today: [], activeTabId: null })
  })

  it('create: an empty name gets the default name', () => {
    const w = world()
    expect(w.model.spaces.create({ name: '', icon: '', hue: 10 })).toMatchObject({ name: 'Espace', icon: '✨' })
  })

  it('update: changes name, icon, tint; a gradient then a solid color', () => {
    const w = world()
    expect(w.model.spaces.update('s1', { name: 'Neuf', icon: 'N', hue: 12, theme: { hues: [10, 200], grain: 0.5 } })).toBe(true)
    expect(w.space().theme).toBeDefined()
    w.model.spaces.update('s1', { name: 'Neuf', icon: 'N', hue: 12 })
    expect(w.space()).not.toHaveProperty('theme')
    expect(w.model.spaces.update('zz', { name: 'x', icon: 'x', hue: 1 })).toBe(false)
  })

  it('switchTo : change l’espace actif s’il existe', () => {
    const w = world({ spaces: [{}, {}] })
    expect(w.model.spaces.switchTo('s2')).toBe(true)
    expect(w.state.activeSpaceId).toBe('s2')
    expect(w.model.spaces.switchTo('zz')).toBe(false)
    expect(w.state.activeSpaceId).toBe('s2')
  })

  it("setProfile: sets the profile and rechecks the tabs' sessions", () => {
    const w = world({ tabs: ['p', 'a', 'in'], spaces: [{ pinned: [tab('p'), folder('f', [tab('in')])], today: [tab('a')] }] })
    expect(w.model.spaces.setProfile(w.space(), 'isolated')).toBe(true)
    expect(w.space().profile).toBe('isolated')
    expect(w.effects.reconciled).toEqual([['p', 'in', 'a']])
  })

  it('setProfile: space gone during the confirmation', () => {
    const w = world({ spaces: [{}, {}] })
    const space = w.space()
    w.state.spaces.shift()
    expect(w.model.spaces.setProfile(space, 'isolated')).toBe(false)
  })

  it('tabIds: pinned then day tabs, folders flattened', () => {
    const w = world({ tabs: ['p', 'a', 'in'], spaces: [{ pinned: [folder('f', [tab('in')]), tab('p')], today: [tab('a')] }] })
    expect(w.model.spaces.tabIds(w.space())).toEqual(['in', 'p', 'a'])
  })
})

describe('spaces: routing rules', () => {
  it('addRoute: normalized pattern, one rule per pattern', () => {
    const w = world({ spaces: [{}, {}] })
    expect(w.model.spaces.addRoute('https://GitHub.com/orgs', 's2')).toBe(true)
    const [route] = w.state.settings.routes
    expect(route).toMatchObject({ spaceId: 's2' })
    w.model.spaces.addRoute(route.pattern, 's1')
    expect(w.state.settings.routes).toHaveLength(1)
    expect(w.state.settings.routes[0].spaceId).toBe('s1')
  })

  it('addRoute: empty pattern or unknown space rejected', () => {
    const w = world()
    expect(w.model.spaces.addRoute('  ', 's1')).toBe(false)
    expect(w.model.spaces.addRoute('github.com', 'zz')).toBe(false)
    expect(w.state.settings.routes).toEqual([])
  })

  it('removeRoute', () => {
    const w = world({
      settings: {
        routes: [
          { id: 'r1', pattern: 'a.com', spaceId: 's1' },
          { id: 'r2', pattern: 'b.com', spaceId: 's1' }
        ]
      }
    })
    w.model.spaces.removeRoute('r1')
    expect(w.state.settings.routes.map((r) => r.id)).toEqual(['r2'])
  })
})

describe('spaces : ordre et navigation', () => {
  it('move: shifts by one rank, not beyond the edges', () => {
    const w = world({ spaces: [{}, {}, {}] })
    expect(w.model.spaces.move('s2', -1)).toBe(true)
    expect(w.state.spaces.map((s) => s.id)).toEqual(['s2', 's1', 's3'])
    expect(w.model.spaces.move('s2', -1)).toBe(false)
    expect(w.model.spaces.move('s3', 1)).toBe(false)
    expect(w.state.activeSpaceId).toBe('s1')
  })

  it('cycleTarget: wrapping around in both directions', () => {
    const w = world({ spaces: [{}, {}, {}], active: 's3' })
    expect(w.model.spaces.cycleTarget(1)).toBe('s1')
    expect(w.model.spaces.cycleTarget(-1)).toBe('s2')
  })

  it('stepTarget: without wrapping', () => {
    const w = world({ spaces: [{}, {}], active: 's2' })
    expect(w.model.spaces.stepTarget(1)).toBeUndefined()
    expect(w.model.spaces.stepTarget(-1)).toBe('s1')
  })

  it('numberTarget: starting from 1', () => {
    const w = world({ spaces: [{}, {}] })
    expect(w.model.spaces.numberTarget(2)).toBe('s2')
    expect(w.model.spaces.numberTarget(3)).toBeUndefined()
  })
})

describe('spaces.delete', () => {
  const setup = () =>
    world({
      tabs: ['a', 'b', 'x'],
      spaces: [{ today: [tab('a'), tab('b')], activeTabId: 'a' }, { today: [tab('x')] }],
      settings: {
        routes: [
          { id: 'r1', pattern: 'a.com', spaceId: 's1' },
          { id: 'r2', pattern: 'b.com', spaceId: 's2' }
        ]
      }
    })

  it('the last space cannot be deleted; neither can an unknown space', () => {
    const w = world()
    expect(w.model.spaces.delete('s1')).toBeNull()
    expect(setup().model.spaces.delete('zz')).toBeNull()
  })

  it('removes the space, its tabs (stacked as one batch) and its rules; the neighbor becomes active', () => {
    const w = setup()
    const removed = w.model.spaces.delete('s1')!
    expect(removed.tabCount).toBe(2)
    expect(w.state.spaces.map((s) => s.id)).toEqual(['s2'])
    expect(w.state.activeSpaceId).toBe('s2')
    expect(Object.keys(w.state.tabs)).toEqual(['x'])
    expect(w.state.settings.routes.map((r) => r.id)).toEqual(['r2'])
    expect(w.effects.destroyed).toEqual(['a', 'b'])
    expect(w.data.closed).toHaveLength(2)
    expect(w.data.closed[0].batch).toBe(w.data.closed[1].batch)
  })

  it('deleting a space that is not active keeps the active one', () => {
    const w = setup()
    w.model.spaces.delete('s2')
    expect(w.state.activeSpaceId).toBe('s1')
  })

  it('undo puts the space back at its rank, with its tabs, its rules and its active status', () => {
    const w = setup()
    const removed = w.model.spaces.delete('s1')!
    expect(removed.restore()).toBe(true)
    expect(w.state.spaces.map((s) => s.id)).toEqual(['s1', 's2'])
    expect(w.state.activeSpaceId).toBe('s1')
    expect(Object.keys(w.state.tabs).sort()).toEqual(['a', 'b', 'x'])
    expect(w.state.settings.routes.map((r) => r.id).sort()).toEqual(['r1', 'r2'])
    expect(w.data.closed).toEqual([])
    expect(shape(w.space('s1').today)).toEqual(['a', 'b'])
  })

  it('undoing twice: the second has no effect', () => {
    const w = setup()
    const removed = w.model.spaces.delete('s1')!
    removed.restore()
    expect(removed.restore()).toBe(false)
    expect(w.state.spaces).toHaveLength(2)
  })
})

import { describe, expect, it } from 'vitest'
import { folder, shape, tab, world } from './testkit'

describe('folders.create', () => {
  it('at the head of the pinned tabs, with the default name', () => {
    const w = world({ tabs: ['p'], spaces: [{ pinned: [tab('p')] }] })
    const created = w.model.folders.create()
    expect(created.name).toBe('Folder')
    expect(shape(w.space().pinned)).toEqual([`[${created.id}: ]`, 'p'])
  })

  it('in a parent folder, which opens', () => {
    const w = world({ spaces: [{ pinned: [folder('f', [], { open: false })] }] })
    const created = w.model.folders.create('f')
    expect(shape(w.space().pinned)).toEqual([`[f: [${created.id}: ]]`])
    expect((w.space().pinned[0] as { open: boolean }).open).toBe(true)
  })
})

describe('folders: settings', () => {
  const setup = () => world({ spaces: [{ pinned: [folder('f', [], { hue: 10, icon: '📁' })] }] })
  const f = (w: ReturnType<typeof setup>) => w.space().pinned[0] as { name: string; hue?: number; icon?: string; open: boolean }

  it('rename: name cleaned and capped at 80 characters', () => {
    const w = setup()
    expect(w.model.folders.rename('f', '  Travail  ')).toBe(true)
    expect(f(w).name).toBe('Travail')
    w.model.folders.rename('f', 'x'.repeat(200))
    expect(f(w).name).toHaveLength(80)
  })

  it('rename: empty name or unknown folder rejected', () => {
    const w = setup()
    expect(w.model.folders.rename('f', '   ')).toBe(false)
    expect(w.model.folders.rename('zz', 'ok')).toBe(false)
    expect(f(w).name).toBe('f')
  })

  it('tint: set, then removed with null', () => {
    const w = setup()
    w.model.folders.setHue('f', 200)
    expect(f(w).hue).toBe(200)
    w.model.folders.setHue('f', null)
    expect(f(w)).not.toHaveProperty('hue')
  })

  it('emoji: set, then removed', () => {
    const w = setup()
    w.model.folders.setIcon('f', '🔥')
    expect(f(w).icon).toBe('🔥')
    w.model.folders.setIcon('f', undefined)
    expect(f(w)).not.toHaveProperty('icon')
  })

  it('toggle: flips, or forces a state', () => {
    const w = setup()
    w.model.folders.toggle('f')
    expect(f(w).open).toBe(false)
    w.model.folders.toggle('f', true)
    expect(f(w).open).toBe(true)
    expect(w.model.folders.toggle('zz')).toBe(false)
  })
})

describe('folders.delete', () => {
  it('without the tabs: its items move up into its place', () => {
    const w = world({ tabs: ['a', 'b', 'p'], spaces: [{ pinned: [tab('p'), folder('f', [tab('a'), tab('b')])] }] })
    expect(w.model.folders.delete('f', false)).toEqual({})
    expect(shape(w.space().pinned)).toEqual(['p', 'a', 'b'])
    expect(w.effects.destroyed).toEqual([])
  })

  it('unknown folder: null', () => {
    expect(world().model.folders.delete('zz', true)).toBeNull()
  })

  it('with the tabs: destroyed, stacked for ⇧⌘T in one batch, and undoable', () => {
    const w = world({
      tabs: ['a', 'b', 'p'],
      spaces: [{ pinned: [tab('p'), folder('f', [tab('a'), tab('b')])], activeTabId: 'a' }]
    })
    const result = w.model.folders.delete('f', true)
    expect(result?.removed?.count).toBe(2)
    expect(shape(w.space().pinned)).toEqual(['p'])
    expect(Object.keys(w.state.tabs)).toEqual(['p'])
    expect(w.effects.destroyed).toEqual(['a', 'b'])
    expect(w.data.closed).toHaveLength(2)
    expect(w.data.closed[0].batch).toBe(w.data.closed[1].batch)
    expect(w.data.closed[0].url).toBe('https://a.test/') // the folder's first on top of the stack
    expect(w.space().activeTabId).toBe('p')

    expect(result!.removed!.restore()).toBe(true)
    expect(shape(w.space().pinned)).toEqual(['[f: a b]', 'p'])
    expect(Object.keys(w.state.tabs).sort()).toEqual(['a', 'b', 'p'])
    expect(w.data.closed).toEqual([])
    expect(w.space().activeTabId).toBe('a')
  })

  it("a folder's folder goes back into its parent at its rank", () => {
    const w = world({ tabs: ['a', 'b'], spaces: [{ pinned: [folder('outer', [tab('a'), folder('inner', [tab('b')])])] }] })
    const result = w.model.folders.delete('inner', true)
    expect(shape(w.space().pinned)).toEqual(['[outer: a]'])
    result!.removed!.restore()
    expect(shape(w.space().pinned)).toEqual(['[outer: a [inner: b]]'])
  })

  it('undo after the space was deleted: nothing', () => {
    const w = world({ tabs: ['a'], spaces: [{ pinned: [folder('f', [tab('a')])] }, {}] })
    const result = w.model.folders.delete('f', true)
    w.state.spaces.shift()
    expect(result!.removed!.restore()).toBe(false)
  })

  it('an empty folder deleted with its tabs: nothing to announce', () => {
    const w = world({ spaces: [{ pinned: [folder('f', [])] }] })
    expect(w.model.folders.delete('f', true)).toEqual({})
  })
})

describe('folders.addTo', () => {
  it('at the end of an existing folder, which opens', () => {
    const w = world({ tabs: ['a', 'x'], spaces: [{ pinned: [folder('f', [tab('x')], { open: false })], today: [tab('a')] }] })
    const result = w.model.folders.addTo('a', 'f')
    expect(result).toMatchObject({ created: false, moved: true })
    expect(shape(w.space().pinned)).toEqual(['[f: x a]'])
    expect((w.space().pinned[0] as { open: boolean }).open).toBe(true)
  })

  it('new folder: created at the head of the pinned tabs, with the tab inside', () => {
    const w = world({ tabs: ['a'], spaces: [{ today: [tab('a')] }] })
    const result = w.model.folders.addTo('a', null)!
    expect(result.created).toBe(true)
    expect(shape(w.space().pinned)).toEqual([`[${result.folder.id}: a]`])
    expect(w.space().today).toEqual([])
  })

  it('a favorite or an unknown tab: nothing', () => {
    const w = world({ tabs: ['f'], favorites: ['f'] })
    expect(w.model.folders.addTo('f', null)).toBeNull()
    expect(w.model.folders.addTo('zz', null)).toBeNull()
  })

  it('a split view enters the folder whole', () => {
    const w = world({ tabs: ['a', 'b'], spaces: [{ pinned: [folder('f', [])], today: [{ kind: 'split', id: 'sp', tabIds: ['a', 'b'] }] }] })
    w.model.folders.addTo('a', 'f')
    expect(shape(w.space().pinned)).toEqual(['[f: {sp: a b}]'])
  })
})

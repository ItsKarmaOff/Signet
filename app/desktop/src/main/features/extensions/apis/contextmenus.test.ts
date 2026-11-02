import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { describe, expect, it, vi } from 'vitest'
import { ContextMenus, contextsOf, itemMatches, labelFor, mergeProps, type MenuItem } from './contextmenus'

const item = (p: Partial<MenuItem> = {}): MenuItem => ({
  id: 'a',
  type: 'normal',
  title: 'A',
  checked: false,
  enabled: true,
  visible: true,
  contexts: ['page'],
  ...p
})
const setup = () => {
  const emitter = { emit: vi.fn() }
  return { menus: new ContextMenus(emitter), emitter }
}

describe('mergeProps (update)', () => {
  it('changes title, checked, enabled, visible, contexts and patterns and keeps the rest', () => {
    const next = mergeProps(item({ documentUrlPatterns: ['https://a/*'] }), {
      title: 'B',
      checked: true,
      enabled: false,
      visible: false,
      contexts: 'link',
      targetUrlPatterns: ['https://t/*']
    })
    expect(next).toMatchObject({
      id: 'a',
      title: 'B',
      checked: true,
      enabled: false,
      visible: false,
      contexts: ['link'],
      documentUrlPatterns: ['https://a/*'],
      targetUrlPatterns: ['https://t/*']
    })
  })
  it('rejects an empty title and an unknown type', () => {
    expect(() => mergeProps(item(), { title: '' })).toThrow(/title/)
    expect(() => mergeProps(item(), { type: 'bogus' })).toThrow()
    expect(mergeProps(item({ type: 'separator', title: '' }), {}).type).toBe('separator')
  })
})

describe('context filtering', () => {
  it('page applies only when nothing more specific was clicked', () => {
    expect([...contextsOf({ pageURL: 'https://a/' })]).toEqual(['all', 'page'])
    expect(contextsOf({ pageURL: 'https://a/', linkURL: 'https://b/' }).has('page')).toBe(false)
    expect(contextsOf({ pageURL: 'https://a/', selectionText: 'x', isEditable: true })).toEqual(new Set(['all', 'selection', 'editable']))
    expect(contextsOf({ mediaType: 'image' }).has('image')).toBe(true)
  })
  it('keeps items of a matching context, honours visible and the url patterns', () => {
    const link = contextsOf({ linkURL: 'https://t/x' })
    expect(itemMatches(item({ contexts: ['link'] }), link)).toBe(true)
    expect(itemMatches(item({ contexts: ['page'] }), link)).toBe(false)
    expect(itemMatches(item({ contexts: ['all'] }), link)).toBe(true)
    expect(itemMatches(item({ contexts: ['link'], visible: false }), link)).toBe(false)
    const target = { linkURL: 'https://t/x', pageURL: 'https://p/' }
    expect(itemMatches(item({ contexts: ['link'], targetUrlPatterns: ['https://t/*'] }), link, target)).toBe(true)
    expect(itemMatches(item({ contexts: ['link'], targetUrlPatterns: ['https://other/*'] }), link, target)).toBe(false)
    expect(itemMatches(item({ contexts: ['link'], documentUrlPatterns: ['https://p/*'] }), link, target)).toBe(true)
    expect(itemMatches(item({ contexts: ['link'], documentUrlPatterns: ['https://q/*'] }), link, target)).toBe(false)
  })
  it('action contexts are only in the button menu', () => {
    const button = new Set(['all', 'action'])
    expect(itemMatches(item({ contexts: ['action'] }), button)).toBe(true)
    expect(itemMatches(item({ contexts: ['action'] }), contextsOf({ pageURL: 'https://a/' }))).toBe(false)
  })
  it('%s stands for the selection', () => {
    expect(labelFor('Search %s', 'cats')).toBe('Search cats')
    expect(labelFor('Search %s')).toBe('Search %s')
  })
})

describe('ContextMenus store', () => {
  it('create generates ids, refuses duplicates and unknown parents', () => {
    const { menus } = setup()
    const id = menus.create('e', { title: 'x' })
    expect(typeof id).toBe('string')
    expect(() => menus.create('e', { id, title: 'y' })).toThrow(/duplicate/)
    expect(() => menus.create('e', { id: 'c', title: 'y', parentId: 'nope' })).toThrow(/Cannot find/)
    expect(() => menus.create('e', { id: 'd' })).toThrow(/title/)
    menus.create('other', { id, title: 'same id, other extension' })
  })

  it('update really changes the item shown, and fails for an unknown id', () => {
    const { menus } = setup()
    menus.create('e', { id: 'a', title: 'Old', contexts: ['link'] })
    expect(menus.tree('e', contextsOf({ linkURL: 'https://x/' }))[0].label).toBe('Old')
    menus.update('e', 'a', { title: 'New', contexts: ['selection'] })
    expect(menus.tree('e', contextsOf({ linkURL: 'https://x/' }))).toEqual([])
    expect(menus.tree('e', contextsOf({ selectionText: 's' }))[0].label).toBe('New')
    expect(() => menus.update('e', 'zzz', { title: 'x' })).toThrow(/Cannot find/)
    expect(() => menus.update('e', 'a', { parentId: 'a' })).toThrow()
  })

  it('builds a tree, removing a parent removes its children, and a hidden parent hides them', () => {
    const { menus } = setup()
    menus.create('e', { id: 'p', title: 'P' })
    menus.create('e', { id: 'c', title: 'C', parentId: 'p' })
    const tree = menus.tree('e', contextsOf({ pageURL: 'https://a/' }))
    expect(tree).toHaveLength(1)
    expect(tree[0].children.map((n) => n.item.id)).toEqual(['c'])
    menus.update('e', 'p', { visible: false })
    expect(menus.tree('e', contextsOf({ pageURL: 'https://a/' }))).toEqual([])
    menus.remove('e', 'p')
    expect(menus.has('e')).toBe(false)
  })

  it('click flips checkboxes, selects one radio of a group and wakes the worker on clicked', () => {
    const { menus, emitter } = setup()
    menus.create('e', { id: 'cb', title: 'C', type: 'checkbox' })
    expect(menus.click('e', 'cb')).toMatchObject({ checked: true, wasChecked: false })
    expect(menus.click('e', 'cb')).toMatchObject({ checked: false, wasChecked: true })
    menus.create('e', { id: 'r1', title: 'R1', type: 'radio', checked: true })
    menus.create('e', { id: 'r2', title: 'R2', type: 'radio' })
    menus.click('e', 'r2')
    expect(
      menus
        .tree('e', new Set(['page']))
        .map((n) => [n.item.id, n.item.checked])
        .filter(([id]) => String(id).startsWith('r'))
    ).toEqual([
      ['r1', false],
      ['r2', true]
    ])
    menus.clicked('e', { menuItemId: 'cb' }, { id: 3 })
    expect(emitter.emit).toHaveBeenCalledWith('e', 'contextMenus.onClicked', [{ menuItemId: 'cb' }, { id: 3 }], true)
  })
})

describe('persistence', () => {
  const file = () => join(mkdtempSync(join(tmpdir(), 'signet-menus-')), 'extension-context-menus.json')
  const page = new Set(['page'])

  it('saves items, restores them for the extension when it loads and flushes at quit', async () => {
    const path = file()
    const a = setup().menus
    a.persist(path)
    a.create('e', { id: 'x', title: 'X' })
    a.create('f', { id: 'y', title: 'Y' })
    a.update('e', 'x', { title: 'X2' })
    a.remove('f', 'y')
    a.flush()
    expect(Object.keys(JSON.parse(readFileSync(path, 'utf8')))).toEqual(['e'])
    const b = setup().menus
    b.persist(path)
    expect(b.tree('e', page)).toEqual([])
    b.restore('e')
    expect(b.tree('e', page).map((n) => n.label)).toEqual(['X2'])
  })

  it('a create with a restored id replaces it silently, a duplicate in the same run still fails', () => {
    const path = file()
    writeFileSync(path, JSON.stringify({ e: [{ id: 'x', type: 'normal', title: 'Old', checked: false, enabled: true, visible: true, contexts: ['page'] }] }))
    const { menus } = setup()
    menus.persist(path)
    menus.restore('e')
    expect(menus.create('e', { id: 'x', title: 'New' })).toBe('x')
    expect(menus.tree('e', page).map((n) => n.label)).toEqual(['New'])
    expect(() => menus.create('e', { id: 'x', title: 'Again' })).toThrow(/duplicate/)
  })

  it('an item created this run is not overwritten by the restore', () => {
    const path = file()
    writeFileSync(path, JSON.stringify({ e: [{ id: 'x', type: 'normal', title: 'Old', checked: false, enabled: true, visible: true, contexts: ['page'] }] }))
    const { menus } = setup()
    menus.persist(path)
    menus.create('e', { id: 'x', title: 'Fresh' })
    menus.restore('e')
    expect(menus.tree('e', page).map((n) => n.label)).toEqual(['Fresh'])
  })

  it('uninstalling drops the items from memory and disk', () => {
    const path = file()
    const { menus } = setup()
    menus.persist(path)
    menus.create('e', { id: 'x', title: 'X' })
    menus.flush()
    menus.unload('e')
    menus.flush()
    expect(JSON.parse(readFileSync(path, 'utf8'))).toEqual({})
    expect(menus.has('e')).toBe(false)
    const again = setup().menus
    again.persist(path)
    again.restore('e')
    expect(again.has('e')).toBe(false)
  })
})

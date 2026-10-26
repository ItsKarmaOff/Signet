import { describe, expect, it } from 'vitest'
import { countLinks, folderItems, folderPath, importBookmarks, insertBookmark, moveBookmarkBy, moveDestinations, removeBookmark } from './bookmarks'
import type { Bookmark } from '../types'

const link = (id: string): Bookmark => ({ kind: 'link', id, title: id, url: `https://${id}.test` })
const folder = (id: string, items: Bookmark[] = []): Bookmark => ({ kind: 'folder', id, name: id, items })
const ids = (nodes: readonly Bookmark[]) => nodes.map((n) => n.id)

const tree = (): Bookmark[] => [link('a'), folder('f', [link('b'), link('c'), folder('g', [link('d')])]), link('e')]

describe('bookmark tree helpers', () => {
  it('restores a removed folder with its content at its old place', () => {
    const t = tree()
    const node = t[1]
    const without = removeBookmark(t, 'f')
    expect(ids(insertBookmark(without, node, undefined, 1))).toEqual(['a', 'f', 'e'])
    expect(insertBookmark(t, node)).toBe(t)
  })

  it('finds folder contents and path', () => {
    expect(ids(folderItems(tree(), 'f') ?? [])).toEqual(['b', 'c', 'g'])
    expect(folderItems(tree(), 'zzz')).toBeNull()
    expect(folderItems(tree(), 'a')).toBeNull()
    expect(folderPath(tree(), 'g').map((f) => f.id)).toEqual(['f', 'g'])
    expect(folderPath(tree(), 'a')).toEqual([])
  })

  it('counts links', () => {
    expect(countLinks(tree()[1])).toBe(3)
    expect(countLinks(tree()[0])).toBe(1)
  })

  it('reorders by one rank and stops at the ends', () => {
    expect(ids(folderItems(moveBookmarkBy(tree(), 'b', 1), 'f') ?? [])).toEqual(['c', 'b', 'g'])
    expect(ids(folderItems(moveBookmarkBy(tree(), 'g', -1), 'f') ?? [])).toEqual(['b', 'g', 'c'])
    expect(ids(moveBookmarkBy(tree(), 'e', 1))).toEqual(['a', 'f', 'e'])
    expect(ids(moveBookmarkBy(tree(), 'a', -1))).toEqual(['a', 'f', 'e'])
    expect(ids(moveBookmarkBy(tree(), 'a', 1))).toEqual(['f', 'a', 'e'])
  })

  it('lists valid move destinations', () => {
    expect(moveDestinations(tree(), 'f').map((d) => d.folder.id)).toEqual([])
    expect(moveDestinations(tree(), 'b').map((d) => d.folder.id)).toEqual(['f', 'g'])
  })

  it('imports into a new folder and ignores an empty import', () => {
    const t = tree()
    const out = importBookmarks(t, [link('x')], 'Imported')
    expect(out).toHaveLength(4)
    expect(out[3]).toMatchObject({ kind: 'folder', name: 'Imported' })
    expect(importBookmarks(t, [], 'Imported')).toBe(t)
  })
})

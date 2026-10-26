import { describe, expect, it } from 'vitest'
import type { Bookmark } from '../types'
import {
  addBookmark,
  addFolder,
  bookmarkFolders,
  bookmarkLinks,
  findBookmark,
  isBookmarked,
  moveBookmark,
  parseBookmarks,
  removeBookmark,
  removeBookmarkByUrl,
  searchBookmarks,
  updateBookmark
} from './bookmarks'

function sample(): Bookmark[] {
  let t: Bookmark[] = []
  t = addBookmark(t, { title: 'A', url: 'https://a.com' })
  t = addFolder(t, 'Folder')
  const folder = t[1].id
  t = addBookmark(t, { title: 'B', url: 'https://b.com' }, folder)
  t = addBookmark(t, { title: 'C', url: 'https://c.com' }, folder)
  return t
}

describe('bookmarks', () => {
  it('adds links and folders, no duplicates within a folder', () => {
    const t = sample()
    expect(bookmarkLinks(t).map((l) => l.title)).toEqual(['A', 'B', 'C'])
    expect(addBookmark(t, { title: 'A2', url: 'https://a.com' })).toHaveLength(t.length)
    expect(isBookmarked(t, 'https://b.com')).toBe(true)
    expect(isBookmarked(t, 'https://z.com')).toBe(false)
  })

  it('never mutates the original tree', () => {
    const t = sample()
    const copy = JSON.stringify(t)
    removeBookmark(t, t[0].id)
    moveBookmark(t, t[0].id, { index: 0, folderId: t[1].id })
    updateBookmark(t, t[0].id, { title: 'X' })
    expect(JSON.stringify(t)).toBe(copy)
  })

  it('removes by id (the folder goes with its content) and by address', () => {
    const t = sample()
    expect(bookmarkLinks(removeBookmark(t, t[1].id))).toHaveLength(1)
    expect(bookmarkLinks(removeBookmarkByUrl(t, 'https://c.com')).map((l) => l.title)).toEqual(['A', 'B'])
  })

  it('updates a link and renames a folder', () => {
    const t = sample()
    const u = updateBookmark(updateBookmark(t, t[0].id, { title: 'Alpha' }), t[1].id, { name: 'Range' })
    expect(u[0]).toMatchObject({ title: 'Alpha' })
    expect(u[1]).toMatchObject({ name: 'Range' })
  })

  it('moves: into a folder, to the root, and reorders within the same folder', () => {
    const t = sample()
    const folder = t[1].id
    const intoFolder = moveBookmark(t, t[0].id, { folderId: folder, index: 0 })
    expect(intoFolder).toHaveLength(1)
    expect(bookmarkLinks(intoFolder).map((l) => l.title)).toEqual(['A', 'B', 'C'])
    const b = bookmarkLinks(t)[1]
    const out = moveBookmark(t, b.id, { index: 0 })
    expect(out[0].id).toBe(b.id)
    const c = bookmarkLinks(t)[2]
    const reordered = moveBookmark(t, c.id, { folderId: folder, index: 0 })
    expect(bookmarkLinks(reordered).map((l) => l.title)).toEqual(['A', 'C', 'B'])
    // To the end of the same folder (index = length).
    const toEnd = moveBookmark(t, bookmarkLinks(t)[1].id, { folderId: folder, index: 2 })
    expect(bookmarkLinks(toEnd).map((l) => l.title)).toEqual(['A', 'C', 'B'])
  })

  it('refuses to drop a folder into itself or into its descendant', () => {
    let t = sample()
    const outer = t[1].id
    t = addFolder(t, 'Inner', outer)
    const inner = (findBookmark(t, outer)!.node as Extract<Bookmark, { kind: 'folder' }>).items.find((n) => n.kind === 'folder')!.id
    expect(moveBookmark(t, outer, { folderId: outer, index: 0 })).toBe(t)
    expect(moveBookmark(t, outer, { folderId: inner, index: 0 })).toBe(t)
  })

  it('lists folders with their depth and searches', () => {
    let t = sample()
    t = addFolder(t, 'Inner', t[1].id)
    expect(bookmarkFolders(t).map((f) => f.depth)).toEqual([0, 1])
    expect(searchBookmarks(t, 'b.com').map((l) => l.title)).toEqual(['B'])
    expect(searchBookmarks(t, '')).toHaveLength(3)
  })

  it('parseBookmarks: tolerant, unique ids', () => {
    expect(parseBookmarks(42)).toEqual([])
    const out = parseBookmarks([
      { kind: 'link', id: 'x', title: 'T', url: 'https://a.com' },
      { kind: 'link', id: 'x', url: 'https://b.com' },
      { kind: 'link', id: 'y' },
      { kind: 'folder', id: 'f', name: 'F', items: [{ kind: 'link', id: 'z', title: 'Z', url: 'https://z.com' }, 5] },
      null,
      { kind: 'wat' }
    ])
    expect(out).toHaveLength(3)
    expect(new Set(bookmarkLinks(out).map((l) => l.id)).size).toBe(3)
    expect(bookmarkLinks(out)[1].title).toBe('https://b.com')
  })
})

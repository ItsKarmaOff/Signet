// Bookmarks: tree of links and folders (`Bookmark` type shared with desktop). Pure functions, never mutating.
import { newId } from '../ids'
import { fold } from '../urls'
import { parseHomepageInput } from './settings'
import type { Bookmark, BookmarkTarget } from '../types'

export type BookmarkLink = Extract<Bookmark, { kind: 'link' }>
export type BookmarkFolder = Extract<Bookmark, { kind: 'folder' }>

export const MAX_BOOKMARK_DEPTH = 6

/** Finds a bookmark (link or folder) by id, with the folder containing it (`undefined` = root) and its position. */
export function findBookmark(
  tree: readonly Bookmark[],
  id: string,
  parent?: BookmarkFolder
): { node: Bookmark; parent?: BookmarkFolder; index: number } | null {
  for (let i = 0; i < tree.length; i++) {
    const node = tree[i]
    if (node.id === id) return { node, parent, index: i }
    if (node.kind === 'folder') {
      const found = findBookmark(node.items, id, node)
      if (found) return found
    }
  }
  return null
}

/** All links of the tree, in order. */
export function bookmarkLinks(tree: readonly Bookmark[]): BookmarkLink[] {
  return tree.flatMap((n) => (n.kind === 'link' ? [n] : bookmarkLinks(n.items)))
}

export const findBookmarkByUrl = (tree: readonly Bookmark[], url: string): BookmarkLink | undefined => bookmarkLinks(tree).find((l) => l.url === url)

export const isBookmarked = (tree: readonly Bookmark[], url: string): boolean => !!findBookmarkByUrl(tree, url)

/** Flat list of folders (with their depth), for an "Add to…" picker. */
export function bookmarkFolders(tree: readonly Bookmark[], depth = 0): { folder: BookmarkFolder; depth: number }[] {
  return tree.flatMap((n) => (n.kind === 'folder' ? [{ folder: n, depth }, ...bookmarkFolders(n.items, depth + 1)] : []))
}

function mapFolder(tree: readonly Bookmark[], folderId: string | undefined, fn: (items: Bookmark[]) => Bookmark[]): Bookmark[] {
  if (folderId === undefined) return fn([...tree])
  return tree.map((n) =>
    n.kind === 'folder' ? (n.id === folderId ? { ...n, items: fn([...n.items]) } : { ...n, items: mapFolder(n.items, folderId, fn) }) : n
  )
}

const clampIndex = (i: number | undefined, len: number) => (i === undefined ? len : Math.min(Math.max(0, i), len))

/** Adds a link (at the end of folder `folderId`, or of the root). A link already in that folder is not duplicated. */
export function addBookmark(tree: readonly Bookmark[], link: { title: string; url: string; favicon?: string }, folderId?: string, index?: number): Bookmark[] {
  const node: BookmarkLink = { kind: 'link', id: newId(), title: link.title.trim() || link.url, url: link.url }
  if (link.favicon) node.favicon = link.favicon
  return mapFolder(tree, folderId, (items) => {
    if (items.some((n) => n.kind === 'link' && n.url === link.url)) return items
    items.splice(clampIndex(index, items.length), 0, node)
    return items
  })
}

export function addFolder(tree: readonly Bookmark[], name: string, folderId?: string): Bookmark[] {
  const node: BookmarkFolder = { kind: 'folder', id: newId(), name: name.trim() || '…', items: [] }
  return mapFolder(tree, folderId, (items) => [...items, node])
}

/** Removes a bookmark (a folder goes with its content). */
export function removeBookmark(tree: readonly Bookmark[], id: string): Bookmark[] {
  return tree.filter((n) => n.id !== id).map((n) => (n.kind === 'folder' ? { ...n, items: removeBookmark(n.items, id) } : n))
}

/** Removes the link with address `url` everywhere it appears. */
export function removeBookmarkByUrl(tree: readonly Bookmark[], url: string): Bookmark[] {
  return tree.filter((n) => !(n.kind === 'link' && n.url === url)).map((n) => (n.kind === 'folder' ? { ...n, items: removeBookmarkByUrl(n.items, url) } : n))
}

/** Edits a link (title, address) or renames a folder (`name`). */
export function updateBookmark(tree: readonly Bookmark[], id: string, patch: { title?: string; url?: string; name?: string }): Bookmark[] {
  return tree.map((n) => {
    if (n.id === id) {
      if (n.kind === 'link') return { ...n, title: patch.title?.trim() || n.title, url: patch.url?.trim() || n.url }
      return { ...n, name: patch.name?.trim() || n.name }
    }
    return n.kind === 'folder' ? { ...n, items: updateBookmark(n.items, id, patch) } : n
  })
}

const depthOf = (n: Bookmark): number => (n.kind === 'folder' ? 1 + Math.max(0, ...n.items.map(depthOf)) : 0)

/** Moves a bookmark to `target` (root or folder, at position `index`). Refused (tree unchanged) if it is a folder dropped into itself or if nesting exceeds `MAX_BOOKMARK_DEPTH`. */
export function moveBookmark(tree: readonly Bookmark[], id: string, target: BookmarkTarget): Bookmark[] {
  const found = findBookmark(tree, id)
  if (!found) return tree as Bookmark[]
  if (target.folderId !== undefined) {
    if (found.node.kind === 'folder' && (found.node.id === target.folderId || findBookmark(found.node.items, target.folderId))) return tree as Bookmark[]
    const dest = findBookmark(tree, target.folderId)
    if (!dest || dest.node.kind !== 'folder') return tree as Bookmark[]
    let level = 1
    for (let p = dest.parent; p; p = findBookmark(tree, p.id)?.parent) level++
    if (level + depthOf(found.node) > MAX_BOOKMARK_DEPTH) return tree as Bookmark[]
  }
  const without = removeBookmark(tree, id)
  // Removing the bookmark first shifts the positions in its original folder: adjust the index when staying in the same folder.
  const sameParent = found.parent?.id === target.folderId && (found.parent !== undefined || target.folderId === undefined)
  const index = sameParent && found.index < target.index ? target.index - 1 : target.index
  return mapFolder(without, target.folderId, (items) => {
    items.splice(clampIndex(index, items.length), 0, found.node)
    return items
  })
}

/** Links whose title or address contains all words of `query`. */
export function searchBookmarks(tree: readonly Bookmark[], query: string): BookmarkLink[] {
  const words = fold(query).split(/\s+/).filter(Boolean)
  const links = bookmarkLinks(tree)
  if (words.length === 0) return links
  return links.filter((l) => words.every((w) => fold(l.title).includes(w) || fold(l.url).includes(w)))
}

/** Reloads saved bookmarks: malformed nodes ignored, unique ids, depth limited. */
export function parseBookmarks(raw: unknown, depth = 0, seen = new Set<string>()): Bookmark[] {
  if (!Array.isArray(raw) || depth >= MAX_BOOKMARK_DEPTH) return []
  const out: Bookmark[] = []
  for (const item of raw as unknown[]) {
    const n = item as Record<string, unknown> | null
    if (!n || typeof n !== 'object') continue
    const id = typeof n.id === 'string' && n.id && !seen.has(n.id) ? n.id : newId()
    if (n.kind === 'link' && typeof n.url === 'string' && n.url) {
      seen.add(id)
      const link: BookmarkLink = { kind: 'link', id, title: typeof n.title === 'string' && n.title ? n.title : n.url, url: n.url }
      if (typeof n.favicon === 'string' && n.favicon) link.favicon = n.favicon
      out.push(link)
    } else if (n.kind === 'folder' && typeof n.name === 'string') {
      seen.add(id)
      out.push({ kind: 'folder', id, name: n.name || '…', items: parseBookmarks(n.items, depth + 1, seen) })
    }
  }
  return out
}

/** Reinserts a node as is (undo a deletion: the folder comes back with its content), at position `index` of folder `folderId`. */
export function insertBookmark(tree: readonly Bookmark[], node: Bookmark, folderId?: string, index?: number): Bookmark[] {
  if (findBookmark(tree, node.id)) return tree as Bookmark[]
  if (folderId !== undefined && findBookmark(tree, folderId)?.node.kind !== 'folder') folderId = undefined
  return mapFolder(tree, folderId, (items) => {
    items.splice(clampIndex(index, items.length), 0, node)
    return items
  })
}

/** Content of a folder (the root without `folderId`); `null` if the folder does not exist. */
export function folderItems(tree: readonly Bookmark[], folderId?: string): Bookmark[] | null {
  if (folderId === undefined) return tree as Bookmark[]
  const found = findBookmark(tree, folderId)
  return found?.node.kind === 'folder' ? found.node.items : null
}

/** Path of folders from the root to `folderId` (inclusive). Empty if not found. */
export function folderPath(tree: readonly Bookmark[], folderId: string): BookmarkFolder[] {
  const found = findBookmark(tree, folderId)
  if (!found || found.node.kind !== 'folder') return []
  return [...(found.parent ? folderPath(tree, found.parent.id) : []), found.node]
}

/** Number of links in a node (1 for a link). */
export const countLinks = (node: Bookmark): number => (node.kind === 'link' ? 1 : node.items.reduce((n, c) => n + countLinks(c), 0))

/** Shifts a bookmark by `delta` positions within its folder (up / down). At the ends, the tree is unchanged. */
export function moveBookmarkBy(tree: readonly Bookmark[], id: string, delta: number): Bookmark[] {
  const found = findBookmark(tree, id)
  if (!found) return tree as Bookmark[]
  const siblings = found.parent ? found.parent.items : tree
  const to = found.index + delta
  if (to < 0 || to >= siblings.length) return tree as Bookmark[]
  return moveBookmark(tree, id, { folderId: found.parent?.id, index: delta > 0 ? to + 1 : to })
}

/** Folders `id` can be moved into: all, except the bookmark itself and its subfolders, and those that would exceed the maximum depth. */
export function moveDestinations(tree: readonly Bookmark[], id: string): { folder: BookmarkFolder; depth: number }[] {
  const found = findBookmark(tree, id)
  if (!found) return []
  const own = found.node.kind === 'folder' ? found.node : null
  return bookmarkFolders(tree).filter(({ folder, depth }) => {
    if (own && (folder.id === own.id || findBookmark(own.items, folder.id))) return false
    return depth + 1 + depthOf(found.node) <= MAX_BOOKMARK_DEPTH
  })
}

/** Adds imported bookmarks in a new folder `folderName` at the end of the root (nothing if the list is empty). */
export function importBookmarks(tree: readonly Bookmark[], imported: readonly Bookmark[], folderName: string): Bookmark[] {
  if (imported.length === 0) return tree as Bookmark[]
  const folder: BookmarkFolder = { kind: 'folder', id: newId(), name: folderName.trim() || '…', items: [...imported] }
  return [...tree, folder]
}

/** Bookmark address input -> valid http(s) address (`example.com` becomes `https://example.com`), or `null`. */
export const parseBookmarkUrl = (input: string): string | null => parseHomepageInput(input)

/** Remembers the icon of bookmarks pointing to `url`. With no bookmark or no change, the tree is returned as is. */
export function setBookmarkFavicon(tree: readonly Bookmark[], url: string, favicon: string): Bookmark[] {
  if (!favicon) return tree as Bookmark[]
  let changed = false
  const walk = (nodes: readonly Bookmark[]): Bookmark[] =>
    nodes.map((n) => {
      if (n.kind === 'folder') {
        const items = walk(n.items)
        return items.some((it, i) => it !== n.items[i]) ? { ...n, items } : n
      }
      if (n.url !== url || n.favicon === favicon) return n
      changed = true
      return { ...n, favicon }
    })
  const next = walk(tree)
  return changed ? next : (tree as Bookmark[])
}

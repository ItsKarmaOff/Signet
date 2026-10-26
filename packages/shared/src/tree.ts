// Tree utilities for item lists (tabs, split views, folders).
// Pure functions: default (translated) names are passed by the caller.
import type { Item, Space, TabGroup, TabInfo } from './types'
import { MAX_PANES, sanitizeSplit } from './split'
import { cleanLiveFolder } from './feeds'
import { newId } from './ids'

export type Folder = Extract<Item, { kind: 'folder' }>
/** Item that directly carries tabs (not a folder). */
export type Leaf = Exclude<Item, { kind: 'folder' }>

/** Maximum display depth of folders (beyond it, the UI no longer indents). */
export const MAX_FOLDER_DEPTH = 6

/** Position of an item in the tree: its list, its index, its parent folder and the folder path. */
export interface Slot<T extends Item = Item> {
  list: Item[]
  index: number
  item: T
  parent: Folder | null
  /** Folders traversed, from the topmost to the closest. */
  path: Folder[]
}

export const tabItem = (tabId: string): Item => ({ kind: 'tab', tabId })

/** Ids of an item's tabs, descending into folders. */
export function itemTabIds(item: Item): string[] {
  if (item.kind === 'tab') return [item.tabId]
  if (item.kind === 'split') return item.tabIds
  return item.items.flatMap(itemTabIds)
}

/** Depth-first traversal in display order; `fn` returning `true` stops the traversal. */
export function walk(items: Item[], fn: (slot: Slot) => boolean | void, path: Folder[] = []): boolean {
  for (let index = 0; index < items.length; index++) {
    const item = items[index]
    const parent = path[path.length - 1] ?? null
    if (fn({ list: items, index, item, parent, path }) === true) return true
    if (item.kind === 'folder' && walk(item.items, fn, [...path, item])) return true
  }
  return false
}

export function findSlot(items: Item[], pred: (item: Item) => boolean): Slot | null {
  let found: Slot | null = null
  walk(items, (slot) => {
    if (!pred(slot.item)) return false
    found = slot
    return true
  })
  return found
}

/** Position of the item (single tab or split view) that contains the tab. */
export function findTabSlot(items: Item[], tabId: string): Slot<Leaf> | null {
  return findSlot(items, (i) => i.kind !== 'folder' && itemTabIds(i).includes(tabId)) as Slot<Leaf> | null
}

export function findFolderSlot(items: Item[], folderId: string): Slot<Folder> | null {
  return findSlot(items, (i) => i.kind === 'folder' && i.id === folderId) as Slot<Folder> | null
}

export function findSplitSlot(items: Item[], splitId: string): Slot<Leaf> | null {
  return findSlot(items, (i) => i.kind === 'split' && i.id === splitId) as Slot<Leaf> | null
}

/** True if `id` designates `folder` itself or one of its descendants. */
export function folderContains(folder: Folder, id: string): boolean {
  return folder.id === id || folder.items.some((i) => i.kind === 'folder' && folderContains(i, id))
}

export function removeAt(list: Item[], index: number): Item {
  return list.splice(index, 1)[0]
}

/** Inserts at `index` (clamped to the list size). */
export function insertAt(list: Item[], index: number, item: Item): void {
  list.splice(Math.min(list.length, Math.max(0, index)), 0, item)
}

/* ---------- Tab groups (day tabs) ---------- */

export const groupOf = (item: Item | undefined): string | undefined => (item && item.kind !== 'folder' ? item.group : undefined)

/** `[start, end[` bounds of the run of items of the group in the list. */
export function groupRange(list: Item[], id: string): { start: number; end: number } | null {
  const start = list.findIndex((i) => groupOf(i) === id)
  if (start < 0) return null
  let end = start
  while (end < list.length && groupOf(list[end]) === id) end++
  return { start, end }
}

/**
 * Keeps the group invariant: a single consecutive run of at least two items per group.
 * Members detached from their run and groups reduced to one item lose the label.
 */
export function normalizeGroups(list: Item[]): void {
  const seen = new Set<string>()
  for (let i = 0; i < list.length;) {
    const id = groupOf(list[i])
    if (!id) {
      i++
      continue
    }
    let end = i
    while (end < list.length && groupOf(list[end]) === id) end++
    if (seen.has(id) || end - i < 2) for (let k = i; k < end; k++) delete (list[k] as Leaf).group
    else seen.add(id)
    i = end
  }
}

/** Hues offered to new groups, in order: blue, green, purple, orange, pink, turquoise, yellow, red. */
export const GROUP_HUES = [215, 140, 265, 28, 330, 180, 48, 0]

/** First hue that no other group of the space uses (otherwise wrap around the list). */
export function pickGroupHue(groups: Record<string, TabGroup>): number {
  const used = new Set(Object.values(groups).map((g) => g.hue))
  return GROUP_HUES.find((h) => !used.has(h)) ?? GROUP_HUES[Object.keys(groups).length % GROUP_HUES.length]
}

/** Default title of a group: that of the tab that opened it (chosen name, page title, otherwise the domain). */
function defaultGroupName(item: Item, tabs: Record<string, TabInfo>): string {
  const tab = tabs[itemTabIds(item)[0]]
  if (!tab) return ''
  if (tab.customTitle || tab.title) return (tab.customTitle || tab.title).slice(0, 80)
  try {
    return new URL(tab.url).hostname.replace(/^www\./, '')
  } catch {
    return ''
  }
}

/** Group invariant of the space, then headers: one per live group (created as needed), none for a vanished group. */
export function syncGroups(space: Space, tabs: Record<string, TabInfo>): void {
  normalizeGroups(space.today)
  if (!space.groups || typeof space.groups !== 'object') space.groups = {}
  const groups = space.groups
  const live = new Map<string, Item>()
  for (const item of space.today) {
    const id = groupOf(item)
    if (id && !live.has(id)) live.set(id, item)
  }
  for (const [id, g] of Object.entries(groups)) {
    // Vanished, or unreadable (file edited by hand): recreated below if it still has tabs.
    if (!live.has(id) || !g || typeof g.name !== 'string' || !Number.isFinite(g.hue)) delete groups[id]
    else g.open = g.open !== false
  }
  for (const [id, first] of live) groups[id] ??= { name: defaultGroupName(first, tabs), hue: pickGroupHue(groups), open: true }
  if (Object.keys(groups).length === 0) delete space.groups
}

/** Insertion position that cuts no group: in the middle of a group, go just after it. */
export function outsideGroups(list: Item[], index: number): number {
  const at = Math.min(list.length, Math.max(0, index))
  const around = groupOf(list[at - 1])
  return around && around === groupOf(list[at]) ? groupRange(list, around)!.end : at
}

/** Single tabs and split views of a list, descending into folders (in display order). */
export function leavesOf(items: Item[]): Leaf[] {
  return items.flatMap((i) => (i.kind === 'folder' ? leavesOf(i.items) : [i]))
}

/** Removes any group label (lists outside the day tabs). */
export function stripGroups(items: Item[]): void {
  walk(items, ({ item }) => {
    if (item.kind !== 'folder') delete item.group
  })
}

/**
 * Inserts an item into the day tabs. It joins `groupId` if it touches that group at that spot, or the group
 * surrounding it if it lands in the middle of a run; otherwise it belongs to no group.
 */
export function insertToday(list: Item[], index: number, item: Item, groupId?: string): void {
  const at = Math.min(list.length, Math.max(0, index))
  if (item.kind !== 'folder') {
    const prev = groupOf(list[at - 1])
    const next = groupOf(list[at])
    if (groupId && (prev === groupId || next === groupId)) item.group = groupId
    else if (prev && prev === next) item.group = prev
    else delete item.group
  }
  list.splice(at, 0, item)
}

/** All folders, with their readable path "Parent / Child". */
export function folderPaths(items: Item[]): { folder: Folder; label: string }[] {
  const out: { folder: Folder; label: string }[] = []
  walk(items, ({ item, path }) => {
    if (item.kind === 'folder') out.push({ folder: item, label: [...path, item].map((f) => f.name).join(' / ') })
  })
  return out
}

export function newFolder(name: string): Folder {
  return { kind: 'folder', id: newId(), name, open: true, items: [] }
}

/**
 * Cleans a list read from disk: keeps known tabs, removes empty split views,
 * turns single-tab ones into a tab, normalizes folders (or flattens them if `folders` is false).
 * `folderFallbackName`: (translated) name of an unnamed folder.
 */
export function pruneItems(items: unknown, known: (tabId: string) => boolean, folders: boolean, folderFallbackName: string): Item[] {
  if (!Array.isArray(items)) return []
  return (items as Item[]).flatMap((item): Item[] => {
    if (!item || typeof item !== 'object') return []
    if (item.kind === 'tab') return known(item.tabId) ? [item] : []
    if (item.kind === 'split') {
      const tabIds = (Array.isArray(item.tabIds) ? item.tabIds : []).filter(known).slice(0, MAX_PANES)
      if (tabIds.length === 0) return []
      if (tabIds.length === 1) {
        return [item.group ? { kind: 'tab', tabId: tabIds[0], group: item.group } : tabItem(tabIds[0])]
      }
      // `layout` and `sizes` are kept, then brought back to a value consistent with the number of tabs.
      const split = { ...item, tabIds }
      if (tabIds.length !== item.tabIds.length) delete split.sizes
      sanitizeSplit(split)
      return [split]
    }
    if (item.kind === 'folder') {
      const inner = pruneItems(item.items, known, folders, folderFallbackName)
      if (!folders) return inner
      return [
        {
          kind: 'folder',
          id: typeof item.id === 'string' && item.id ? item.id : newId(),
          name: typeof item.name === 'string' && item.name ? item.name : folderFallbackName,
          ...(typeof item.icon === 'string' && item.icon ? { icon: item.icon.slice(0, 16) } : {}),
          open: item.open !== false,
          items: inner,
          ...(Number.isFinite(item.hue) ? { hue: Math.round(Number(item.hue)) % 360 } : {}),
          ...(item.live && cleanLiveFolder(item.live) ? { live: cleanLiveFolder(item.live) } : {})
        }
      ]
    }
    return []
  })
}

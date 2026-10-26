// Item lists (favorites, pinned, day tabs, folders): removal, moving, pinning.
import type { DragSource, DropTarget, Item } from '@shared/types'
import {
  findFolderSlot,
  findTabSlot,
  folderContains,
  groupRange,
  insertAt,
  insertToday,
  itemTabIds,
  leavesOf,
  outsideGroups,
  pickGroupHue,
  removeAt,
  tabItem,
  type Folder,
  type Leaf
} from '@signet/shared/tree'
import { sanitizeSplit } from '@signet/shared/split'
import type { Activation } from './activation'
import { activeSpace, findFolder, findSplit, locate, spaceOfGroup, type Location } from './locate'
import { clamp, type ModelData, type ModelEnv } from './types'

export class Lists {
  constructor(
    private readonly data: ModelData,
    private readonly env: ModelEnv,
    private readonly activation: Activation
  ) {}

  /** Removes the tab from its list. In a split view, only this tab goes (unless `whole`). */
  extract(tabId: string, whole: boolean): Location | null {
    const { state } = this.data
    const loc = locate(state, tabId)
    if (!loc) return null
    if (loc.zone === 'favorites') {
      state.favorites.splice(loc.index, 1)
      return loc
    }
    const list = loc.list
    if (loc.item.kind === 'split' && !whole) {
      const tabIds = loc.item.tabIds.filter((t) => t !== tabId)
      const { sizes: _sizes, ...rest } = loc.item
      // The tab left alone takes the split view's place in its group.
      list[loc.index] = tabIds.length === 1 ? { kind: 'tab', tabId: tabIds[0], ...(rest.group ? { group: rest.group } : {}) } : { ...rest, tabIds }
      const left = list[loc.index]
      if (left.kind === 'split') sanitizeSplit(left)
      return { ...loc, shrunk: true }
    }
    removeAt(list, loc.index)
    return loc
  }

  /** Moves a tab, a split view, a folder or a group. False if nothing moved. */
  moveItem(source: DragSource, target: DropTarget): boolean {
    if (source.kind === 'group') return target.zone === 'pinned' ? this.pinGroup(source.id, target) : this.moveGroup(source.id, target)
    if (source.kind === 'folder' && target.zone === 'today') return this.unpinFolder(source.id, target)
    const { state } = this.data
    const origin = source.kind === 'tab' ? locate(state, source.tabId) : source.kind === 'split' ? findSplit(state, source.id) : findFolder(state, source.id)
    if (!origin) return false
    const item: Item = source.kind === 'tab' ? tabItem(source.tabId) : origin.item
    const ids = itemTabIds(item)
    const dest = state.spaces.find((s) => s.id === target.spaceId) ?? activeSpace(state)
    if (target.zone === 'favorites' && item.kind !== 'tab') return false
    if (item.kind === 'folder' && target.zone !== 'pinned') return false
    // Target list: zone root, or folder (pinned only).
    let destList: Item[] | null
    if (target.zone === 'favorites') destList = null
    else if (target.zone === 'pinned' && target.folderId) {
      const folder = findFolderSlot(dest.pinned, target.folderId)?.item
      // No dropping a folder into itself or one of its descendants; a live folder fills itself.
      if (!folder || folder.live || (item.kind === 'folder' && folderContains(item, folder.id))) return false
      destList = folder.items
    } else destList = dest[target.zone]
    if (ids.length === 0 && item.kind !== 'folder') return false

    const loc = item.kind === 'folder' ? (removeAt(origin.list, origin.index), origin) : this.extract(ids[0], item.kind === 'split')
    if (!loc) return false
    let index = target.index
    const sameList = destList ? loc.list === destList : loc.zone === 'favorites'
    if (sameList && !loc.shrunk && loc.index < index) index -= 1

    if (!destList) {
      state.favorites.splice(clamp(index, 0, state.favorites.length), 0, ids[0])
    } else if (target.zone === 'today') {
      insertToday(destList, index, item, target.groupId)
    } else {
      if (item.kind !== 'folder') delete item.group
      insertAt(destList, index, item)
    }
    for (const id of ids) {
      const tab = state.tabs[id]
      if (target.zone === 'today') delete tab.pinnedUrl
      else tab.pinnedUrl ??= tab.url
    }
    this.activation.repick(loc, ids, true)
    this.env.reconcileSessions(ids)
    return true
  }

  /** Moves a whole group into a space's day tabs (never into the middle of another group). */
  private moveGroup(groupId: string, target: DropTarget): boolean {
    if (target.zone !== 'today') return false
    const { state } = this.data
    const from = state.spaces.find((sp) => groupRange(sp.today, groupId))
    const range = from && groupRange(from.today, groupId)
    if (!from || !range) return false
    const dest = state.spaces.find((sp) => sp.id === target.spaceId) ?? activeSpace(state)
    const items = from.today.splice(range.start, range.end - range.start)
    let index = target.index
    if (dest === from && range.start < index) index = Math.max(range.start, index - items.length)
    // In the middle of another group: land just after it.
    dest.today.splice(outsideGroups(dest.today, index), 0, ...items)
    const header = from.groups?.[groupId]
    if (header && dest !== from) {
      delete from.groups![groupId]
      ;(dest.groups ??= {})[groupId] = header
    }
    const ids = items.flatMap(itemTabIds)
    this.activation.repick({ zone: 'today', space: from, index: range.start, list: from.today, item: items[0] }, ids, true)
    this.env.reconcileSessions(ids)
    return true
  }

  /**
   * Pins a group: it becomes a folder of the pinned items (same name, emoji, tint, collapsed state), at the end of the pinned items
   * of its space, or at the spot targeted by a drag and drop (root or inside a folder).
   */
  pinGroup(groupId: string, target?: DropTarget): boolean {
    const { state } = this.data
    const from = spaceOfGroup(state, groupId)
    const range = from && groupRange(from.today, groupId)
    if (!from || !range) return false
    const dest = (target && state.spaces.find((sp) => sp.id === target.spaceId)) || from
    const parent = target?.folderId ? findFolderSlot(dest.pinned, target.folderId)?.item : undefined
    if (target?.folderId && !parent) return false
    const header = from.groups?.[groupId]
    const items = from.today.splice(range.start, range.end - range.start) as Leaf[]
    for (const item of items) delete item.group
    const ids = items.flatMap(itemTabIds)
    const first = state.tabs[ids[0]]
    const folder: Folder = {
      kind: 'folder',
      id: this.env.newId(),
      name: header?.name || first?.customTitle || first?.title || this.env.defaultFolderName(),
      open: header?.open ?? true,
      items,
      ...(header ? { hue: header.hue } : {}),
      ...(header?.icon ? { icon: header.icon } : {})
    }
    const list = parent ? parent.items : dest.pinned
    insertAt(list, target ? target.index : list.length, folder)
    for (const id of ids) {
      const tab = state.tabs[id]
      if (tab) tab.pinnedUrl ??= tab.url
    }
    this.activation.repick({ zone: 'today', space: from, index: range.start, list: from.today, item: items[0] }, ids, true)
    this.env.reconcileSessions(ids)
    return true
  }

  /**
   * Unpins a folder: its tabs (subfolders included, flattened) become a group of the day tabs, with
   * its name, emoji, tint and collapsed state; at the top of the day tabs, or at the targeted spot (never in the middle of another group).
   */
  unpinFolder(folderId: string, target?: DropTarget): boolean {
    const { state } = this.data
    const loc = findFolder(state, folderId)
    if (!loc?.space) return false
    const folder = loc.item
    const leaves = leavesOf(folder.items)
    if (leaves.length === 0) return false
    const dest = (target && state.spaces.find((sp) => sp.id === target.spaceId)) || loc.space
    removeAt(loc.list, loc.index)
    const id = this.env.newId()
    for (const leaf of leaves) leaf.group = id
    let index = target ? target.index : 0
    // The dragged folder leaves the pinned items, not the day tabs: nothing to shift in the target list.
    index = outsideGroups(dest.today, index)
    dest.today.splice(index, 0, ...leaves)
    const groups = (dest.groups ??= {})
    groups[id] = { name: folder.name, hue: folder.hue ?? pickGroupHue(groups), open: folder.open, ...(folder.icon ? { icon: folder.icon } : {}) }
    const ids = leaves.flatMap(itemTabIds)
    for (const tabId of ids) {
      const tab = state.tabs[tabId]
      if (tab) delete tab.pinnedUrl
    }
    this.activation.repick(loc, ids, true)
    this.env.reconcileSessions(ids)
    return true
  }

  /** Pins the tab (or its split view) at the end of its space's pinned items, or puts it back among the day tabs. */
  togglePin(tabId: string): boolean {
    const loc = locate(this.data.state, tabId)
    if (!loc) return false
    const source: DragSource = loc.item.kind === 'split' && loc.zone !== 'favorites' ? { kind: 'split', id: loc.item.id } : { kind: 'tab', tabId }
    const space = loc.space ?? activeSpace(this.data.state)
    const toPinned = loc.zone === 'today'
    return this.moveItem(source, {
      zone: toPinned ? 'pinned' : 'today',
      spaceId: space.id,
      index: toPinned ? space.pinned.length : 0
    })
  }

  toggleFavorite(tabId: string): boolean {
    const { state } = this.data
    const { favorites } = state
    if (favorites.includes(tabId)) {
      return this.moveItem({ kind: 'tab', tabId }, { zone: 'today', spaceId: activeSpace(state).id, index: 0 })
    }
    return this.moveItem({ kind: 'tab', tabId }, { zone: 'favorites', spaceId: activeSpace(state).id, index: favorites.length })
  }

  /** Moves a tab up (-1) or down (+1) one rank in its list (alternative to drag and drop). */
  moveTabStep(tabId: string, dir: -1 | 1): boolean {
    const { state } = this.data
    const loc = locate(state, tabId)
    if (!loc) return false
    const index = dir < 0 ? loc.index - 1 : loc.index + 2
    if (index < 0 || index > loc.list.length + 1 || (dir > 0 && loc.index + 1 >= loc.list.length)) return false
    const source: DragSource = loc.item.kind === 'split' && loc.zone !== 'favorites' ? { kind: 'split', id: loc.item.id } : { kind: 'tab', tabId }
    const folderId = loc.zone === 'pinned' && loc.space ? findTabSlot(loc.space.pinned, tabId)?.parent?.id : undefined
    return this.moveItem(source, {
      zone: loc.zone,
      spaceId: (loc.space ?? activeSpace(state)).id,
      folderId,
      groupId: loc.zone === 'today' && loc.item.kind !== 'folder' ? loc.item.group : undefined,
      index
    })
  }

  moveTabToSpace(tabId: string, spaceId: string): boolean {
    return this.moveItem({ kind: 'tab', tabId }, { zone: 'today', spaceId, index: 0 })
  }
}

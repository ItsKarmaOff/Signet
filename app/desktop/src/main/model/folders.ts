// Pinned folders: creation, settings, deletion (with or without their tabs), filing a tab.
import type { DragSource } from '@shared/types'
import { findFolderSlot, insertAt, itemTabIds, newFolder, removeAt, walk, type Folder } from '@signet/shared/tree'
import type { Activation } from './activation'
import type { ClosedTabs } from './closed'
import type { Lists } from './lists'
import { activeSpace, findFolder, locate } from './locate'
import type { Removal } from './removal'
import type { ModelData, ModelEnv } from './types'

/** Name of a folder or group typed by the user: no surrounding spaces, 80 characters at most. */
export const cleanName = (name: string): string => String(name).trim().slice(0, 80)

/** Result of a deletion that removed tabs: the means to announce and undo. */
export interface Removed {
  /** Number of tabs removed. */
  count: number
  /** Puts everything back; false if the space disappeared in the meantime. */
  restore: () => boolean
}

export class Folders {
  constructor(
    private readonly data: ModelData,
    private readonly env: ModelEnv,
    private readonly activation: Activation,
    private readonly closed: ClosedTabs,
    private readonly lists: Lists,
    private readonly removal: Removal
  ) {}

  /** Creates a folder at the top of the active space's pinned tabs (or of `parentId`). */
  create(parentId?: string): Folder {
    const space = activeSpace(this.data.state)
    const folder = newFolder(this.env.defaultFolderName())
    const parent = parentId ? findFolderSlot(space.pinned, parentId)?.item : undefined
    if (parent) parent.open = true
    insertAt(parent ? parent.items : space.pinned, 0, folder)
    return folder
  }

  rename(folderId: string, name: string): boolean {
    const folder = findFolder(this.data.state, folderId)?.item
    const clean = cleanName(name)
    if (!folder || !clean) return false
    folder.name = clean
    return true
  }

  /** Tint of a folder's card; `null` removes it. */
  setHue(folderId: string, hue: number | null): boolean {
    const folder = findFolder(this.data.state, folderId)?.item
    if (!folder) return false
    if (hue === null) delete folder.hue
    else folder.hue = hue
    return true
  }

  /** Emoji instead of the folder icon; absent = folder icon. */
  setIcon(folderId: string, icon: string | undefined): boolean {
    const folder = findFolder(this.data.state, folderId)?.item
    if (!folder) return false
    if (icon) folder.icon = icon
    else delete folder.icon
    return true
  }

  toggle(folderId: string, open?: boolean): boolean {
    const folder = findFolder(this.data.state, folderId)?.item
    if (!folder) return false
    folder.open = open ?? !folder.open
    return true
  }

  /**
   * Deletes the folder; its items move up into its place, or its tabs are destroyed (`withTabs`). Returns `null` if the
   * folder does not exist. `removed` exists only if tabs were destroyed: their undo.
   */
  delete(folderId: string, withTabs: boolean): { removed?: Removed } | null {
    const { state } = this.data
    const loc = findFolder(state, folderId)
    if (!loc) return null
    const ids = itemTabIds(loc.item)
    if (!withTabs) {
      loc.list.splice(loc.index, 1, ...loc.item.items)
      return {}
    }
    // The tabs join the ⇧⌘T stack in a single batch (the folder's first on top) before the folder disappears.
    const batch = this.closed.nextBatch()
    const snap = this.closed.snapshotTabs(ids)
    const saved = structuredClone(loc.item)
    const spaceId = loc.space?.id
    const wasActive = loc.space?.activeTabId ?? null
    let parentId: string | undefined
    if (loc.space && loc.list !== loc.space.pinned) {
      walk(loc.space.pinned, ({ item }) => item.kind === 'folder' && item.items === loc.list && ((parentId = item.id), true))
    }
    const at = loc.index
    for (const id of [...ids].reverse()) this.closed.remember(id, locate(state, id), batch)
    removeAt(loc.list, loc.index)
    this.removal.drop(ids)
    this.activation.repick(loc, ids, true)
    if (ids.length === 0) return {}
    const restore = (): boolean => {
      const sp = this.data.state.spaces.find((s) => s.id === spaceId)
      if (!sp) return false
      this.closed.restoreTabs(snap)
      this.closed.dropBatch(batch)
      // Parent folder gone: at the root of the pinned tabs.
      const parent = parentId ? findFolderSlot(sp.pinned, parentId)?.item.items : undefined
      insertAt(parent ?? sp.pinned, parent ? at : 0, saved)
      if (wasActive && wasActive in this.data.state.tabs) this.activation.setActive(sp, wasActive)
      return true
    }
    return { removed: { count: ids.length, restore } }
  }

  /**
   * Files the tab (or its split view) at the end of a folder of its space; null `folderId` = new folder.
   * Returns the folder, if it is new, and whether the move took place.
   */
  addTo(tabId: string, folderId: string | null): { folder: Folder; created: boolean; moved: boolean } | null {
    const loc = locate(this.data.state, tabId)
    if (!loc?.space || loc.zone === 'favorites') return null
    const source: DragSource = loc.item.kind === 'split' ? { kind: 'split', id: loc.item.id } : { kind: 'tab', tabId }
    const folder = folderId ? findFolderSlot(loc.space.pinned, folderId)?.item : newFolder(this.env.defaultFolderName())
    if (!folder) return null
    if (!folderId) insertAt(loc.space.pinned, 0, folder)
    const moved = this.lists.moveItem(source, { zone: 'pinned', spaceId: loc.space.id, folderId: folder.id, index: folder.items.length })
    folder.open = true
    return { folder, created: !folderId, moved }
  }
}

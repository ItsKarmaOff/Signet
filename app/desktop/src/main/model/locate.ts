// Pure reads of the persisted state: where a tab, a folder or a split view is, which space and which tab are active.
// No Electron import: this folder is testable without a window.
import type { Item, Space, TabInfo, Zone } from '@shared/types'
import type { State } from '../core/store'
import { findFolderSlot, findSplitSlot, findTabSlot, groupOf, itemTabIds, tabItem, type Folder, type Slot } from '@signet/shared/tree'

/** Where a tab is in the sidebar. */
export interface Location {
  zone: Zone
  /** Absent for favorites, which are global. */
  space: Space | null
  /** Rank in `list`. */
  index: number
  /** List that contains `item`: zone root or a folder's `items` (a copy for favorites). */
  list: Item[]
  item: Item
  /** True if the tab left a split view that still exists. */
  shrunk?: boolean
}

export const fromSlot = (zone: 'pinned' | 'today', space: Space, slot: Slot): Location => ({
  zone,
  space,
  index: slot.index,
  list: slot.list,
  item: slot.item
})

/** Active space (the first one if the remembered id has disappeared). */
export function activeSpace(state: Pick<State, 'spaces' | 'activeSpaceId'>): Space {
  const { spaces, activeSpaceId } = state
  return spaces.find((s) => s.id === activeSpaceId) ?? spaces[0]
}

export function activeTab(state: State): TabInfo | undefined {
  const id = activeSpace(state).activeTabId
  return id ? state.tabs[id] : undefined
}

export function locate(state: Pick<State, 'favorites' | 'spaces'>, tabId: string): Location | null {
  const fav = state.favorites.indexOf(tabId)
  if (fav >= 0) return { zone: 'favorites', space: null, index: fav, list: state.favorites.map(tabItem), item: tabItem(tabId) }
  for (const space of state.spaces) {
    for (const zone of ['pinned', 'today'] as const) {
      const slot = findTabSlot(space[zone], tabId)
      if (slot) return fromSlot(zone, space, slot)
    }
  }
  return null
}

export function findSplit(state: Pick<State, 'spaces'>, id: string): Location | null {
  for (const space of state.spaces) {
    for (const zone of ['pinned', 'today'] as const) {
      const slot = findSplitSlot(space[zone], id)
      if (slot) return fromSlot(zone, space, slot)
    }
  }
  return null
}

/** Folder (among any space's pinned items) and its position. */
export function findFolder(state: Pick<State, 'spaces'>, id: string): (Location & { item: Folder }) | null {
  for (const space of state.spaces) {
    const slot = findFolderSlot(space.pinned, id)
    if (slot) return fromSlot('pinned', space, slot) as Location & { item: Folder }
  }
  return null
}

/** Is the tab still reachable from the space (favorite, pinned or day tab)? */
export function reachable(state: Pick<State, 'favorites'>, space: Space, tabId: string): boolean {
  return state.favorites.includes(tabId) || [...space.pinned, ...space.today].some((i) => itemTabIds(i).includes(tabId))
}

/** Item (tab or split view) that carries the space's active tab, among the pinned items or the day tabs. */
function activeItem(state: Pick<State, 'tabs'>, space: Space): { active: string; item: Item | undefined } | null {
  const active = space.activeTabId
  if (!active || !state.tabs[active]) return null
  return { active, item: findTabSlot(space.pinned, active)?.item ?? findTabSlot(space.today, active)?.item }
}

/** Tabs displayed in the space's web card: those of the active split view, otherwise the active tab alone. */
export function displayedTabIds(state: Pick<State, 'tabs'>, space: Space): string[] {
  const found = activeItem(state, space)
  if (!found) return []
  return found.item ? itemTabIds(found.item) : [found.active]
}

/** Displayed split view (the one of the active tab), or null. */
export function displayedSplit(state: Pick<State, 'tabs'>, space: Space): Extract<Item, { kind: 'split' }> | null {
  const item = activeItem(state, space)?.item
  return item?.kind === 'split' ? item : null
}

/** Space that contains the tab group. */
export function spaceOfGroup(state: Pick<State, 'spaces'>, groupId: string): Space | undefined {
  return state.spaces.find((sp) => sp.today.some((i) => groupOf(i) === groupId))
}

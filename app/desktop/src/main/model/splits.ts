// Split views (several tabs side by side): joining, separating, layout and sizes.
import type { Item, SplitLayout } from '@shared/types'
import { insertToday, itemTabIds, tabItem } from '@signet/shared/tree'
import { insertIntoSplit, MAX_PANES, partsFromLengths, sanitizeSplit, type DropEdge } from '@signet/shared/split'
import type { Activation } from './activation'
import { activeSpace, findSplit, locate } from './locate'
import type { Lists } from './lists'
import type { ModelData, ModelEnv } from './types'

export class Splits {
  constructor(
    private readonly data: ModelData,
    private readonly env: ModelEnv,
    private readonly activation: Activation,
    private readonly lists: Lists
  ) {}

  /** True if the active tab can host one more pane. */
  canSplit(): boolean {
    const { state } = this.data
    const active = activeSpace(state).activeTabId
    const loc = active ? locate(state, active) : null
    if (!loc || loc.zone === 'favorites') return false
    return loc.item.kind === 'tab' || (loc.item.kind === 'split' && loc.item.tabIds.length < MAX_PANES)
  }

  /** Makes `otherId` join the active tab's split view (without activating it). False if impossible. */
  splitWith(otherId: string, edge?: DropEdge): boolean {
    const { state, detached } = this.data
    const activeId = activeSpace(state).activeTabId
    const other = state.tabs[otherId]
    if (!activeId || activeId === otherId || !other || detached.has(otherId) || state.favorites.includes(otherId)) return false
    if (!this.canSplit() || itemTabIds(locate(state, activeId)!.item).includes(otherId)) return false

    const from = this.lists.extract(otherId, false)
    const loc = locate(state, activeId)!
    const list = loc.list
    const item = list[loc.index]
    if (item.kind === 'tab') {
      const created: Extract<Item, { kind: 'split' }> = { kind: 'split', id: this.env.newId(), tabIds: [activeId] }
      insertIntoSplit(created, otherId, edge ?? 'right', activeId)
      if (item.group) created.group = item.group
      list[loc.index] = created
    } else if (item.kind === 'split') {
      if (edge) insertIntoSplit(item, otherId, edge, activeId)
      else {
        item.tabIds.push(otherId)
        delete item.sizes
        sanitizeSplit(item)
      }
    }
    if (loc.zone === 'pinned') other.pinnedUrl ??= other.url
    else delete other.pinnedUrl
    if (from) this.activation.repick(from, [otherId], true)
    this.env.reconcileSessions([otherId])
    return true
  }

  /** Undoes the split view: its tabs stay in its place, side by side in the list. */
  unsplit(splitId: string): boolean {
    const loc = findSplit(this.data.state, splitId)
    if (!loc || loc.item.kind !== 'split') return false
    const group = loc.item.group
    loc.list.splice(loc.index, 1, ...loc.item.tabIds.map((id): Item => (group ? { kind: 'tab', tabId: id, group } : tabItem(id))))
    return true
  }

  /** Takes the tab out of its split view: it is placed right after it. */
  removeFromSplit(tabId: string): boolean {
    const loc = locate(this.data.state, tabId)
    if (!loc || loc.item.kind !== 'split') return false
    this.lists.extract(tabId, false)
    if (loc.zone === 'today') insertToday(loc.list, loc.index + 1, tabItem(tabId), loc.item.group)
    else loc.list.splice(loc.index + 1, 0, tabItem(tabId))
    return true
  }

  setLayout(splitId: string, layout: SplitLayout): boolean {
    const loc = findSplit(this.data.state, splitId)
    if (!loc || loc.item.kind !== 'split') return false
    if (layout === 'grid' && loc.item.tabIds.length < 3) return false
    loc.item.layout = layout
    delete loc.item.sizes
    sanitizeSplit(loc.item)
    return true
  }

  /** Sizes proposed by the interface; minimums are applied when computing the rectangles. */
  setSizes(splitId: string, sizes: number[]): boolean {
    const loc = findSplit(this.data.state, splitId)
    if (!loc || loc.item.kind !== 'split' || !Array.isArray(sizes)) return false
    loc.item.sizes = partsFromLengths(sizes.map((x) => (Number.isFinite(x) && x > 0 ? x : 0)))
    sanitizeSplit(loc.item)
    return true
  }
}

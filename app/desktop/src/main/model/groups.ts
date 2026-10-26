// Today tab groups: headers, taking a tab out, closing a block.
import type { Space, TabGroup } from '@shared/types'
import { groupOf, groupRange, itemTabIds, outsideGroups, type Leaf } from '@signet/shared/tree'
import type { Activation } from './activation'
import type { ClosedTabs } from './closed'
import { cleanName, type Removed } from './folders'
import { locate, spaceOfGroup } from './locate'
import type { Removal } from './removal'
import type { ModelData } from './types'

export class Groups {
  constructor(
    private readonly data: ModelData,
    private readonly activation: Activation,
    private readonly closed: ClosedTabs,
    private readonly removal: Removal
  ) {}

  header(groupId: string): TabGroup | undefined {
    return spaceOfGroup(this.data.state, groupId)?.groups?.[groupId]
  }

  /** Takes the tab (or its split view) out of its group: it is placed right after the group. */
  leave(tabId: string): boolean {
    const loc = locate(this.data.state, tabId)
    const group = loc?.zone === 'today' ? groupOf(loc.item) : undefined
    if (!loc || !group) return false
    const range = groupRange(loc.list, group)
    if (!range) return false
    const [item] = loc.list.splice(loc.index, 1)
    delete (item as Leaf).group
    loc.list.splice(range.end - 1, 0, item)
    return true
  }

  /** Dissolves the group: its tabs stay in place, with no link between them. */
  ungroup(groupId: string): void {
    for (const space of this.data.state.spaces) {
      for (const item of space.today) if (groupOf(item) === groupId) delete (item as Leaf).group
    }
  }

  rename(groupId: string, name: string): boolean {
    const group = this.header(groupId)
    const clean = cleanName(name)
    if (!group || !clean) return false
    group.name = clean
    return true
  }

  setHue(groupId: string, hue: number): boolean {
    const group = this.header(groupId)
    if (!group) return false
    group.hue = hue
    return true
  }

  /** Emoji instead of the first tab's icon; absent = first tab's icon. */
  setIcon(groupId: string, icon: string | undefined): boolean {
    const group = this.header(groupId)
    if (!group) return false
    if (icon) group.icon = icon
    else delete group.icon
    return true
  }

  toggle(groupId: string, open?: boolean): boolean {
    const group = this.header(groupId)
    if (!group) return false
    group.open = open ?? !group.open
    return true
  }

  /**
   * Closes all the group's tabs; ⇧⌘T reopens them together, with their header. Returns the means to announce and undo,
   * or `null` if the group does not exist in this space.
   */
  close(space: Space, groupId: string): Removed | null {
    const range = groupRange(space.today, groupId)
    if (!range) return null
    const items = space.today.slice(range.start, range.end)
    const ids = items.flatMap(itemTabIds)
    const batch = this.closed.nextBatch()
    const header = space.groups?.[groupId]
    const snap = this.closed.snapshotTabs(ids)
    const wasActive = space.activeTabId
    // Stacked from last to first: the group's first tab (which carries the header) ends up at the top of the stack.
    for (const id of [...ids].reverse()) this.closed.remember(id, locate(this.data.state, id), batch, id === ids[0] ? header : undefined)
    space.today.splice(range.start, items.length)
    this.removal.drop(ids)
    this.activation.repick({ zone: 'today', space, index: range.start, list: space.today, item: items[0] }, ids, true)
    const spaceId = space.id
    const start = range.start
    const saved = structuredClone(items)
    const restore = (): boolean => {
      const sp = this.data.state.spaces.find((s) => s.id === spaceId)
      if (!sp) return false
      this.closed.restoreTabs(snap)
      this.closed.dropBatch(batch)
      sp.today.splice(outsideGroups(sp.today, start), 0, ...saved)
      if (header) (sp.groups ??= {})[groupId] = { ...header }
      if (wasActive && wasActive in this.data.state.tabs) this.activation.setActive(sp, wasActive)
      return true
    }
    return { count: ids.length, restore }
  }
}

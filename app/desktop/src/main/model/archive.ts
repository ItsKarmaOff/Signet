// Archiving today tabs: on demand (the whole space) or by expiry, with undo for the batch.
import type { Item, Space, TabGroup, TabInfo } from '@shared/types'
import { groupOf, itemTabIds } from '@signet/shared/tree'
import type { ArchivedTab, TabHistory } from '../core/store'
import type { Activation } from './activation'
import { activeSpace } from './locate'
import type { Removal } from './removal'
import { ARCHIVE_MAX, type ModelData, type ModelEnv } from './types'

/** What is needed to undo the last grouped archiving of today tabs. */
interface ArchiveUndo {
  spaceId: string
  /** List of today tabs before archiving (copy). */
  items: Item[]
  /** Group headers before archiving (copy). */
  groups: Record<string, TabGroup>
  tabs: TabInfo[]
  histories: [string, TabHistory][]
  /** Entries created in the archive. */
  entries: ArchivedTab[]
  active: string | null
  /** Active tab just after archiving. */
  activeAfter: string | null
}

export class Archive {
  constructor(
    private readonly data: ModelData,
    private readonly env: ModelEnv,
    private readonly activation: Activation,
    private readonly removal: Removal
  ) {}

  /** Puts the tab in the archive (500 entries at most) then deletes it. */
  archiveTab(tabId: string): ArchivedTab | null {
    const { state } = this.data
    const tab = state.tabs[tabId]
    if (!tab) return null
    const { archive } = state
    const entry: ArchivedTab = { id: this.env.newId(), url: tab.url, title: tab.title, favicon: tab.favicon, archivedAt: this.env.now() }
    archive.unshift(entry)
    archive.length = Math.min(archive.length, ARCHIVE_MAX)
    this.removal.remove(tabId)
    return entry
  }

  /**
   * Archives all today tabs of the active space. Returns the number of tabs and the means to undo (`null`: nothing to archive).
   * `restore` puts the tabs back in their places and removes them from the archive; false if the space is gone.
   */
  clearToday(): { count: number; restore: () => boolean } | null {
    const { state, histories } = this.data
    const space = activeSpace(state)
    const ids = space.today.flatMap(itemTabIds)
    if (ids.length === 0) return null
    for (const id of ids) this.env.saveHistory(id)
    const undo: ArchiveUndo = {
      spaceId: space.id,
      items: structuredClone(space.today),
      groups: structuredClone(space.groups ?? {}),
      tabs: ids.map((id) => ({ ...state.tabs[id] })),
      histories: ids.flatMap((id) => {
        const h = histories.get(id)
        return h ? [[id, h] as [string, TabHistory]] : []
      }),
      entries: [],
      active: space.activeTabId,
      activeAfter: null
    }
    for (const id of ids) {
      const entry = this.archiveTab(id)
      if (entry) undo.entries.push(entry)
    }
    undo.activeAfter = space.activeTabId
    return { count: ids.length, restore: () => this.restore(undo) }
  }

  private restore(undo: ArchiveUndo): boolean {
    const { state, histories } = this.data
    const space = state.spaces.find((s) => s.id === undo.spaceId)
    if (!space) return false
    for (const tab of undo.tabs) {
      if (tab.id in state.tabs) continue
      state.tabs[tab.id] = { ...tab, loaded: false, loading: false, audible: false }
    }
    for (const [tabId, history] of undo.histories) histories.set(tabId, history)
    for (const entry of undo.entries) {
      const at = state.archive.indexOf(entry)
      if (at >= 0) state.archive.splice(at, 1)
    }
    // Tabs opened since stay at the top; the old ones resume their order below them, groups and headers included.
    space.today.push(...undo.items)
    space.groups = { ...undo.groups, ...space.groups }
    // The previously active tab comes back, unless the user chose another in the meantime.
    if (undo.active && undo.active in state.tabs && space.activeTabId === undo.activeAfter) this.activation.setActive(space, undo.active)
    return true
  }

  /**
   * Archives today tabs left too long without activation (except active tabs). `busy`: tabs that sound,
   * the mini player, media or developer tools keep open. Returns the number of archived tabs.
   */
  sweep(busy: (tabId: string) => boolean): number {
    const { spaces, settings } = this.data.state
    if (settings.archiveAfterHours <= 0) return 0 // 0 = never
    const limit = this.env.now() - settings.archiveAfterHours * 3_600_000
    const actives = new Set(spaces.map((s) => s.activeTabId))
    const stale = (id: string) => (this.data.state.tabs[id]?.lastActive ?? Infinity) < limit
    let count = 0
    for (const space of spaces) count += this.sweepSpace(space, actives, stale, busy)
    return count
  }

  private sweepSpace(space: Space, actives: Set<string | null>, stale: (id: string) => boolean, busy: (id: string) => boolean): number {
    const seen = new Set<string>()
    let count = 0
    const archive = (id: string) => {
      if (this.archiveTab(id)) count++
    }
    for (const item of [...space.today]) {
      const group = groupOf(item)
      if (group) {
        // A group is archived as a block, when none of its tabs has been used within the delay.
        if (seen.has(group)) continue
        seen.add(group)
        const ids = space.today.filter((i) => groupOf(i) === group).flatMap(itemTabIds)
        if (ids.some((id) => actives.has(id) || busy(id)) || !ids.every(stale)) continue
        for (const id of ids) archive(id)
        continue
      }
      const ids = itemTabIds(item)
      if (ids.some((id) => actives.has(id))) continue
      for (const id of ids) {
        if (stale(id) && !busy(id)) archive(id)
      }
    }
    return count
  }
}

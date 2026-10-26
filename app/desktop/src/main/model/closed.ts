// Stack of closed tabs (⇧⌘T) and tab copies for undo operations.
import type { TabGroup, TabInfo } from '@shared/types'
import { findFolderSlot, insertAt, insertToday, outsideGroups, tabItem, walk } from '@signet/shared/tree'
import { newTab } from '@signet/shared/model'
import type { ClosedTab, TabHistory } from '../core/store'
import type { Activation } from './activation'
import { activeSpace, type Location } from './locate'
import type { Opening } from './opening'
import { clamp, type ModelData, type ModelEnv } from './types'

/** Number of tabs kept for ⇧⌘T. */
export const CLOSED_MAX = 20

/** Copy of tabs (and their history) before their destruction. */
export interface TabSnapshot {
  tabs: TabInfo[]
  histories: [string, TabHistory][]
}

export class ClosedTabs {
  /** Batch number: tabs closed at once (group, folder, space) share the same one. */
  private batchSeq = 0

  constructor(
    private readonly data: ModelData,
    private readonly env: ModelEnv,
    private readonly activation: Activation,
    private readonly opening: Opening
  ) {}

  nextBatch(): number {
    return ++this.batchSeq
  }

  /** Pushes a tab about to disappear (⇧⌘T): original place and navigation history included. `batch` links the tabs of a closed group. */
  remember(tabId: string, loc: Location | null, batch?: number, header?: TabGroup): void {
    const { state, histories, closed } = this.data
    const tab = state.tabs[tabId]
    if (!tab) return
    this.env.saveHistory(tabId)
    let folderId: string | undefined
    if (loc?.zone === 'pinned' && loc.space && loc.list !== loc.space.pinned) {
      walk(loc.space.pinned, ({ item }) => item.kind === 'folder' && item.items === loc.list && ((folderId = item.id), true))
    }
    closed.unshift({
      url: tab.url,
      title: tab.title,
      favicon: tab.favicon,
      pinnedUrl: tab.pinnedUrl,
      customTitle: tab.customTitle,
      customIcon: tab.customIcon,
      spaceId: loc?.space?.id ?? activeSpace(state).id,
      zone: loc?.zone ?? 'today',
      folderId,
      index: loc?.index ?? 0,
      history: histories.get(tabId),
      ...(batch ? { batch } : {}),
      ...(header ? { group: { ...header } } : {}),
      ...(loc?.zone === 'today' && loc.item.kind !== 'folder' && loc.item.group ? { groupId: loc.item.group } : {})
    })
    if (closed.length > CLOSED_MAX) closed.pop()
  }

  /**
   * Reopens the last closed tab at its original place (otherwise in the current space), with its history. A group closed
   * at once comes back whole. Returns the number of batch tabs left to reopen, one by one (`null`: empty stack).
   */
  reopen(): { more: number } | null {
    const { state, histories, closed: stack } = this.data
    const closed = stack.shift()
    if (!closed) return null
    const home = state.spaces.find((s) => s.id === closed.spaceId)
    const space = home ?? activeSpace(state)
    const revive = (c: ClosedTab) => {
      const tab = newTab(c.url, c.title)
      Object.assign(tab, { favicon: c.favicon, customTitle: c.customTitle, customIcon: c.customIcon })
      state.tabs[tab.id] = tab
      if (c.history) histories.set(tab.id, c.history)
      return tab
    }
    // Group closed at once: all its tabs come back together, grouped, in the group's place.
    const rest: ClosedTab[] = []
    while (closed.batch && stack[0]?.batch === closed.batch) rest.push(stack.shift()!)
    // Batch without header (folder deleted with its tabs): each tab takes its place, one after the other.
    let more = 0
    if (rest.length && !closed.group && !closed.groupId) {
      for (const c of rest) delete c.batch
      stack.unshift(...rest)
      more = rest.length
      rest.length = 0
    }
    if (rest.length) {
      const group = this.env.newId()
      const at = outsideGroups(space.today, home ? closed.index : this.opening.newTabIndex(space))
      const tabs = [closed, ...rest].map(revive)
      tabs.forEach((tab, k) => insertAt(space.today, at + k, { kind: 'tab', tabId: tab.id, group }))
      if (closed.group) (space.groups ??= {})[group] = { ...closed.group }
      state.activeSpaceId = space.id
      this.activation.setActive(space, tabs[0].id)
      return { more: 0 }
    }
    const tab = revive(closed)
    // Space deleted: a pinned tab has no place anymore, it comes back among the today tabs.
    const zone = closed.zone === 'pinned' && !home ? 'today' : closed.zone
    if (zone === 'today') {
      insertToday(space.today, home ? closed.index : this.opening.newTabIndex(space), tabItem(tab.id), closed.groupId)
    } else {
      tab.pinnedUrl = closed.pinnedUrl ?? closed.url
      if (zone === 'favorites') {
        state.favorites.splice(clamp(closed.index, 0, state.favorites.length), 0, tab.id)
      } else {
        const folder = closed.folderId ? findFolderSlot(space.pinned, closed.folderId)?.item : undefined
        // Folder gone: at the end of the pinned tabs.
        if (folder) insertAt(folder.items, closed.index, tabItem(tab.id))
        else insertAt(space.pinned, closed.folderId ? space.pinned.length : closed.index, tabItem(tab.id))
      }
    }
    // A favorite is global: we stay in the current space.
    if (zone !== 'favorites') state.activeSpaceId = space.id
    this.activation.setActive(zone === 'favorites' ? activeSpace(state) : space, tab.id)
    return { more }
  }

  /** Copy of tabs (and their history) before their destruction. */
  snapshotTabs(ids: string[]): TabSnapshot {
    const { state, histories } = this.data
    return {
      tabs: ids.flatMap((id) => (state.tabs[id] ? [{ ...state.tabs[id] }] : [])),
      histories: ids.flatMap((id) => {
        const h = histories.get(id)
        return h ? [[id, h] as [string, TabHistory]] : []
      })
    }
  }

  restoreTabs(snap: TabSnapshot): void {
    const { state, histories } = this.data
    for (const tab of snap.tabs) {
      if (tab.id in state.tabs) continue
      state.tabs[tab.id] = { ...tab, loaded: false, loading: false, audible: false }
    }
    for (const [tabId, history] of snap.histories) histories.set(tabId, history)
  }

  /** Removes from the ⇧⌘T stack the tabs closed at once (undo done: they must not come back twice). */
  dropBatch(batch: number): void {
    const { closed } = this.data
    for (let i = closed.length - 1; i >= 0; i--) if (closed[i].batch === batch) closed.splice(i, 1)
  }
}

// Opening tabs: where the new tab is placed among the day tabs.
import type { Space } from '@shared/types'
import { groupRange, insertToday, tabItem } from '@signet/shared/tree'
import { newTab } from '@signet/shared/model'
import type { Activation } from './activation'
import { activeSpace, locate } from './locate'
import type { ModelData, ModelEnv } from './types'

export interface OpenOptions {
  activate?: boolean
  /** Right after this day tab (otherwise at the top or bottom, per the setting). */
  afterTabId?: string
  /** Tab the link was opened from: if it is among the current space's day tabs, the new tab joins its group. */
  openerId?: string
}

export class Opening {
  constructor(
    private readonly data: ModelData,
    private readonly env: ModelEnv,
    private readonly activation: Activation
  ) {}

  /** Where a new tab goes among the day tabs ("New tabs" setting): at the top, or at the end. */
  newTabIndex(space: Space): number {
    return this.data.state.settings.newTabPosition === 'bottom' ? space.today.length : 0
  }

  /**
   * Creates a tab in the active space. If it comes from a day tab of that space (`openerId`), it joins its group
   * (created if needed), at the end; otherwise it is placed after `afterTabId`, or at the top or bottom per the setting.
   */
  open(url: string, opts: OpenOptions = {}): string {
    const { state } = this.data
    const tab = newTab(url)
    state.tabs[tab.id] = tab
    const space = activeSpace(state)
    const opener = opts.openerId ? locate(state, opts.openerId) : null
    if (opener?.zone === 'today' && opener.space === space && opener.item.kind !== 'folder') {
      const lead = opener.item
      const id = lead.group ?? this.env.newId()
      lead.group = id
      const range = groupRange(space.today, id)
      space.today.splice(range ? range.end : opener.index + 1, 0, { kind: 'tab', tabId: tab.id, group: id })
    } else {
      const after = opts.afterTabId ? locate(state, opts.afterTabId) : null
      const index = after?.zone === 'today' && after.space === space ? after.index + 1 : this.newTabIndex(space)
      insertToday(space.today, index, tabItem(tab.id))
    }
    if (opts.activate) this.activation.setActive(space, tab.id)
    return tab.id
  }

  /** Creates an active tab in another space (routing rule), which becomes the active space. */
  openIn(target: Space, url: string): string {
    const { state } = this.data
    const tab = newTab(url)
    state.tabs[tab.id] = tab
    insertToday(target.today, this.newTabIndex(target), tabItem(tab.id))
    state.activeSpaceId = target.id
    this.activation.setActive(target, tab.id)
    return tab.id
  }

  /** Gives a detached tab a place (among the active space's day tabs, like a new tab) and activates it. */
  adopt(tabId: string): void {
    const { state, detached } = this.data
    detached.delete(tabId)
    const space = activeSpace(state)
    insertToday(space.today, this.newTabIndex(space), tabItem(tabId))
    this.env.reconcileSessions([tabId])
    this.activation.setActive(space, tabId)
  }
}

// Removing tabs: closing, unloading pinned tabs, destroying views and data.
import type { Activation } from './activation'
import type { ClosedTabs } from './closed'
import type { Lists } from './lists'
import { locate } from './locate'
import type { Splits } from './splits'
import type { ModelData, ModelEnv } from './types'

export class Removal {
  constructor(
    private readonly data: ModelData,
    private readonly env: ModelEnv,
    private readonly activation: Activation,
    private readonly closed: ClosedTabs,
    private readonly lists: Lists,
    private readonly splits: Splits
  ) {}

  /** Destroys the views then deletes the tabs and their history. */
  drop(ids: string[]): void {
    for (const id of ids) {
      this.env.destroyView(id)
      delete this.data.state.tabs[id]
      this.data.histories.delete(id)
    }
  }

  /** Removes the tab from its list, destroys its view and deletes it, then picks a new active tab. */
  remove(tabId: string): void {
    const loc = this.lists.extract(tabId, false)
    this.drop([tabId])
    if (loc) this.activation.repick(loc, [tabId], true)
  }

  /**
   * Closes the tab. A pinned tab is only unloaded and reset to its "home" address; the others join the ⇧⌘T stack
   * then disappear. False if the tab is not found.
   */
  close(tabId: string): boolean {
    const { state } = this.data
    const tab = state.tabs[tabId]
    let loc = locate(state, tabId)
    if (!tab || !loc) return false
    if (tab.pinnedUrl) {
      // Closing a pinned tab = unloading it and resetting it to its "home" URL.
      // In a split view, take it out first: otherwise the layout would immediately recreate it.
      if (loc.item.kind === 'split') {
        this.splits.removeFromSplit(tabId)
        loc = locate(state, tabId) ?? loc
      }
      this.env.destroyView(tabId)
      tab.url = tab.pinnedUrl
      this.activation.repick(loc, [tabId], false)
      return true
    }
    this.closed.remember(tabId, loc)
    this.remove(tabId)
    return true
  }
}

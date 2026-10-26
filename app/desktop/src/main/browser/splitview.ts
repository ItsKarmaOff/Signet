import type { SplitLayout } from '@shared/types'
import type { Model } from '../model'
import type { Changed } from './broadcast'
import type { TabActions } from './tab-actions'

export interface SplitViewDeps {
  model: Model
  changed: Changed
  tabs: Pick<TabActions, 'open' | 'activate'>
}

/** Split view: several tabs side by side in the web card. */
export class SplitView {
  constructor(private readonly d: SplitViewDeps) {}

  /** Puts `tabId` in a split view with the active tab and gives it focus; otherwise simply activates it. */
  splitActiveWith(tabId: string): void {
    const { model, tabs } = this.d
    if (model.splits.splitWith(tabId)) model.activation.setActive(model.space, tabId)
    else tabs.activate(tabId)
    this.d.changed()
  }

  openInSplit(url: string): void {
    if (!this.d.model.splits.canSplit()) {
      this.d.tabs.open(url, { activate: true })
      return
    }
    const id = this.d.tabs.open(url)
    this.splitActiveWith(id)
  }

  unsplit(splitId: string): void {
    if (this.d.model.splits.unsplit(splitId)) this.d.changed()
  }

  removeFromSplit(tabId: string): void {
    if (this.d.model.splits.removeFromSplit(tabId)) this.d.changed()
  }

  focusPane(tabId: string): void {
    this.d.tabs.activate(tabId)
  }

  setLayout(splitId: string, layout: SplitLayout): void {
    if (this.d.model.splits.setLayout(splitId, layout)) this.d.changed()
  }

  /** Shares proposed by the UI; the minimums are applied when computing the rectangles. */
  setSizes(splitId: string, sizes: number[], live = false): void {
    // During the drag: nothing to write, the save goes out on release.
    if (this.d.model.splits.setSizes(splitId, sizes)) this.d.changed(live ? { persist: false } : undefined)
  }
}

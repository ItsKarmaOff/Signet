import type { BrowserWindow } from 'electron'
import { MAX_PANES } from '@signet/shared/split'
import { itemTabIds } from '@signet/shared/tree'
import type { Store } from '../core/store'
import type { Model } from '../model'
import type { Changed } from './broadcast'
import type { Floating } from './floating'
import type { Layout } from './layout'
import type { Overlay } from './overlay'
import type { Peek } from './peek'
import type { TabActions } from './tab-actions'

/** Delay after which the last tab change is no longer attributed to the press that starts a drag. */
const SWITCH_WINDOW_MS = 1500
/** Wait, after a drag with no drop on the page, before reactivating the pressed tab (a late drop cancels this callback). */
const DRAG_END_MS = 150

export interface TabDragDeps {
  store: Store
  window: { readonly win: BrowserWindow | null }
  model: Model
  changed: Changed
  apply: () => void
  peek: Pick<Peek, 'tabId' | 'close'>
  floating: Pick<Floating, 'hide'>
  overlay: Pick<Overlay, 'contents' | 'kind' | 'setState' | 'send' | 'hide'>
  layout: Pick<Layout, 'panes' | 'dropArea'>
  tabs: Pick<TabActions, 'takeLastSwitch' | 'activate'>
}

/** Dragging a sidebar tab onto the page: drop zones to create a split view. */
export class TabDrag {
  /** Dragged tab whose activation (on press) is suspended for the duration of the drag. */
  private dragging: { tabId: string; timer?: NodeJS.Timeout } | null = null

  constructor(private readonly d: TabDragDeps) {}

  /** Opens the overlay in drop-zones mode while a sidebar tab is dragged. */
  begin(tabId: string): void {
    const { store, window, model, overlay, layout } = this.d
    if (!window.win || !overlay.contents || !store.state.tabs[tabId]) return
    if (this.d.peek.tabId) this.d.peek.close()
    this.d.floating.hide()
    // The press activated the dragged tab: we put the previous page back so it can be shared with it.
    const last = this.d.tabs.takeLastSwitch()
    if (last && last.tabId === tabId && Date.now() - last.at < SWITCH_WINDOW_MS && model.space.activeTabId === tabId && model.locate(last.before)) {
      this.dragging = { tabId }
      model.activation.setActive(model.space, last.before)
      this.d.changed()
    }
    overlay.setState({ kind: 'dropzones' })
    // Focus unchanged: taking focus during a drag in progress is pointless.
    this.d.apply()
    const { panes } = layout
    const focused = panes.find((p) => p.focused)
    const splitCard = focused && panes.length < MAX_PANES && !store.state.favorites.includes(focused.tabId) ? focused.card : null
    overlay.send({ mode: 'dropzones', area: layout.dropArea(), splitCard })
  }

  end(): void {
    if (this.d.overlay.kind === 'dropzones') this.d.overlay.hide()
    const drag = this.dragging
    if (!drag) return
    // Drag with no drop on the page: the pressed tab becomes active again (a possible drop cancels this callback).
    clearTimeout(drag.timer)
    drag.timer = setTimeout(() => {
      if (this.dragging !== drag) return
      this.dragging = null
      if (this.d.store.state.tabs[drag.tabId]) this.d.tabs.activate(drag.tabId)
    }, DRAG_END_MS)
  }

  dropOnContent(tabId: string, zone: 'left' | 'right' | 'top' | 'bottom' | 'center'): void {
    const { model, overlay, tabs } = this.d
    if (overlay.kind === 'dropzones') overlay.hide()
    if (this.dragging) clearTimeout(this.dragging.timer)
    this.dragging = null
    const loc = model.locate(tabId)
    if (!loc) return
    const active = model.space.activeTabId
    const activeLoc = active ? model.locate(active) : null
    const edge = zone === 'center' ? undefined : zone
    const joinable =
      edge &&
      activeLoc &&
      activeLoc.zone !== 'favorites' &&
      loc.zone !== 'favorites' &&
      active !== tabId &&
      model.splits.canSplit() &&
      !itemTabIds(activeLoc.item).includes(tabId)
    if (joinable && model.splits.splitWith(tabId, edge)) {
      model.activation.setActive(model.space, tabId)
      this.d.changed()
    } else tabs.activate(tabId)
  }
}

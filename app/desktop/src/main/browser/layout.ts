import type { BrowserWindow, WebContents, WebContentsView } from 'electron'
import { IS_MAC } from '../app/platform'
import type { Divider, Pane, Rect } from '@shared/types'
import { computeSplit } from '@signet/shared/split'
import type { Store } from '../core/store'
import { activeSpace, displayedSplit, displayedTabIds } from '../model/locate'
import { isHorizontal, isSidebarOn, isSidebarRight } from '../model/chrome'
import type { Bookmarks } from '../features/bookmarks'
import { BOOKMARK_BAR } from '../features/bookmarks'
import type { Extensions } from '../features/extensions/extensions'
import type { DevTools } from '../devtools/devtools'
import type { Develop } from '../devtools/develop'
import type { SiteColor } from './sitecolor'
import type { Permissions } from '../web/permissions'
import type { Pip } from '../windows/pip'
import { COMPACT_MARGIN, type Compact } from './compact'
import type { FindBar } from './find'
import type { Media } from './media'
import type { TabPreviews } from './preview'
import type { TabSleep } from './sleep'
import type { Views } from './views'
import type { Floating } from './floating'
import type { Changed } from './broadcast'
import type { HtmlFullscreen } from './fullscreen'

const PAD = 8
/** Height of the panel bar, at the top of each card. */
export const PANE_BAR = 40
/** Height of the tab bar (`horizontal` layout) and gap between it and the cards. */
export const TAB_BAR = 40
const TAB_BAR_GAP = 4
/** Size of the archive notice view (360 px notice + margin for the shadow). */
const TOAST_W = 384
const TOAST_H = 72

/** What the layout gives to the floating layers (overlay, Peek) so they place their views. */
export interface LayoutContext {
  /** Size of the window's content area. */
  W: number
  H: number
  panes: Pane[]
  /** Displayed tabs. */
  ids: string[]
  /** Views to stack, from bottom to top. */
  desired: WebContentsView[]
}

/** The overlay as seen by the layout: its state, and the placing of its view. */
export interface OverlayLayer {
  readonly open: boolean
  readonly kind: string | undefined
  readonly leaving: boolean
  readonly popoverOpen: boolean
  readonly contents: WebContents | null
  place(ctx: LayoutContext): void
}

/** The Peek as seen by the layout. */
export interface PeekLayer {
  /** True during entry, exit or the switch to a tab. */
  readonly busy: boolean
  /** Places the Peek's views; returns its web view if it is open. */
  place(ctx: LayoutContext): WebContentsView | undefined
}

export interface LayoutDeps {
  store: Store
  window: { readonly win: BrowserWindow | null }
  changed: Changed
  views: Pick<Views, 'ensureView' | 'viewOf' | 'onRelease'>
  sleep: Pick<TabSleep, 'trackHidden'>
  floating: Pick<Floating, 'shown' | 'leaving' | 'sidebar' | 'warm'>
  compact: Pick<Compact, 'create' | 'hide' | 'view' | 'floating' | 'leaving' | 'bounds' | 'stripBounds'>
  find: Pick<FindBar, 'follow' | 'isOpen' | 'view' | 'bounds'>
  pip: Pick<Pip, 'owns'>
  devtools: Pick<DevTools, 'carve' | 'afterLayout'>
  develop: Pick<Develop, 'place'>
  bookmarks: Pick<Bookmarks, 'visible'>
  extensions: Pick<Extensions, 'select'>
  media: Pick<Media, 'onLayout'>
  permissions: Pick<Permissions, 'onLayout'>
  siteColor: Pick<SiteColor, 'onShown'>
  previews: Pick<TabPreviews, 'placed' | 'viewIfAny' | 'onShown'>
  fullscreen: Pick<HtmlFullscreen, 'tabId' | 'onLayout'>
}

/**
 * Main window layout: rectangles of the cards and separators, view stack (pages, floating bars,
 * notice, preview, overlay), focus, traffic lights.
 */
export class Layout {
  /** Views placed in the window, from bottom to top. */
  private stack: WebContentsView[] = []
  panes: Pane[] = []
  dividers: Divider[] = []
  /** Content area (web card) at the last layout. */
  contentArea: Rect = { x: 0, y: 0, width: 0, height: 0 }
  /** Tabs displayed in the window at the last layout computation. */
  shownTabs: string[] = []
  /** Transient width during a gesture (elastic effect at the limits): never written to the persisted state. */
  liveSidebarWidth: number | null = null
  private focusPending = false
  private findFocus = false
  /** Last `webContents` reported to extensions (null: unknown, to report again). */
  private selectedContents: WebContents | undefined | null = null
  private buttonsVisible = true
  private lightsY = 16
  /** Last rectangle and radius set by the layout on each view (avoids identical native calls). */
  private applied = new WeakMap<WebContentsView, { rect: Rect; radius: number }>()
  private overlay!: OverlayLayer
  private peek!: PeekLayer
  private toast!: { readonly view: WebContentsView | null }

  constructor(private readonly d: LayoutDeps) {
    d.views.onRelease((view) => this.release(view))
  }

  /** Layers placed over the pages; built after the layout, which they need. */
  attach(layers: { overlay: OverlayLayer; peek: PeekLayer; toast: { readonly view: WebContentsView | null } }): void {
    this.overlay = layers.overlay
    this.peek = layers.peek
    this.toast = layers.toast
  }

  // ───────────────────────── Display modes ─────────────────────────

  get horizontal(): boolean {
    return isHorizontal(this.d.store.state)
  }

  get sidebarRight(): boolean {
    return isSidebarRight(this.d.store.state)
  }

  get sidebarOn(): boolean {
    return isSidebarOn(this.d.store.state)
  }

  /** Traffic lights centered in the tab bar (on top) or in the sidebar's 44 px header. */
  trafficLights(): { x: number; y: number } {
    if (this.horizontal) return { x: 16, y: Math.round((TAB_BAR - 14) / 2) }
    // Bar on the right: centered in the top-left card's bar.
    if (this.sidebarRight && !this.d.store.state.settings.compactMode) return { x: 16, y: Math.round(PAD + (PANE_BAR - 14) / 2) }
    return { x: 16, y: 16 }
  }

  /** A main window was just created. */
  onOpen(): void {
    this.buttonsVisible = true
    this.selectedContents = null
  }

  /** The main window is closed. */
  onClosed(): void {
    this.stack = []
    this.panes = []
    this.dividers = []
  }

  /** Origin (window coordinates) of a UI view: sidebar or floating toolbar, otherwise the window. */
  uiOrigin(sender: WebContents): { x: number; y: number } {
    const { sidebar } = this.d.floating
    const toolbar = this.d.compact.view
    const view = sidebar?.webContents === sender ? sidebar.view : toolbar?.webContents === sender ? toolbar : null
    const b = view?.getBounds()
    return b ? { x: b.x, y: b.y } : { x: 0, y: 0 }
  }

  /** Requests focus for the next layout computation (`find`: on the find bar). */
  requestFocus(find = false): void {
    if (find) this.findFocus = true
    this.focusPending = true
  }

  // ───────────────────────── Placing views ─────────────────────────

  /** Places a view (or moves it) without a needless native call. */
  put(view: WebContentsView, rect: Rect, radius?: number): void {
    const last = this.applied.get(view)
    if (radius !== undefined && last?.radius !== radius) view.setBorderRadius(radius)
    if (!last || last.rect.x !== rect.x || last.rect.y !== rect.y || last.rect.width !== rect.width || last.rect.height !== rect.height) view.setBounds(rect)
    this.applied.set(view, { rect: { x: rect.x, y: rect.y, width: rect.width, height: rect.height }, radius: radius ?? last?.radius ?? -1 })
  }

  /** Places or removes only the tab preview view (at the top of the stack), without redoing the layout or sending the state. */
  placePreview(): void {
    const win = this.d.window.win
    if (!win || win.isDestroyed()) return
    // Overlay, Peek or exit in progress: the view order is settled in the full layout.
    if (this.overlay.open || this.overlay.leaving || this.peek.busy) return this.d.changed({ persist: false, layout: true })
    const { previews } = this.d
    const placed = previews.placed
    const known = this.stack.find((v) => v === previews.viewIfAny)
    if (!placed) {
      if (!known) return
      win.contentView.removeChildView(known)
      this.applied.delete(known)
      this.stack = this.stack.filter((v) => v !== known)
      return
    }
    this.put(placed.view, placed.bounds)
    if (known) return
    win.contentView.addChildView(placed.view)
    this.stack = [...this.stack, placed.view]
  }

  /** Removes a view from the main window (it goes to the mini player). */
  detachFromMain(view: WebContentsView): void {
    const win = this.d.window.win
    if (win && !win.isDestroyed() && this.stack.includes(view)) win.contentView.removeChildView(view)
    this.stack = this.stack.filter((v) => v !== view)
    this.applied.delete(view)
  }

  /** A web view is destroyed: it leaves the stack. */
  private release(view: WebContentsView): void {
    const win = this.d.window.win
    this.applied.delete(view)
    if (view.webContents === this.selectedContents) this.selectedContents = null
    if (win && this.stack.includes(view)) {
      win.contentView.removeChildView(view)
      this.stack = this.stack.filter((v) => v !== view)
    }
  }

  /** Aligns the children of `contentView` with `desired`; re-adding a view brings it to the front. */
  private setStack(win: BrowserWindow, desired: WebContentsView[]): void {
    for (const view of this.stack) {
      if (desired.includes(view)) continue
      win.contentView.removeChildView(view)
      this.applied.delete(view)
    }
    const kept = this.stack.filter((v) => desired.includes(v))
    const prefixKept = kept.every((v, i) => desired[i] === v)
    for (const view of prefixKept ? desired.slice(kept.length) : desired) {
      win.contentView.addChildView(view)
      this.applied.delete(view)
    }
    this.stack = desired
  }

  private applyFocus(win: BrowserWindow, ids: string[], peekView?: WebContentsView): void {
    if (!this.focusPending) return
    this.focusPending = false
    const { find, pip, views, store } = this.d
    const active = activeSpace(store.state).activeTabId
    const findFocus = this.findFocus && find.isOpen
    this.findFocus = false
    const target =
      this.overlay.kind === 'peek' && peekView
        ? peekView.webContents
        : this.overlay.open && this.overlay.contents
          ? this.overlay.contents
          : findFocus && find.view
            ? find.view.webContents
            : active && ids.includes(active) && !pip.owns(active)
              ? views.viewOf(active)?.webContents
              : win.webContents
    target?.focus()
  }

  /**
   * Area of the split view targets while dragging a tab: the web card, without the top bars strip when
   * the bookmarks bar is shown (the tab must be droppable there to make it a bookmark).
   */
  dropArea(): Rect {
    const area = { ...this.contentArea }
    if (!this.d.bookmarks.visible || this.d.store.state.settings.compactMode) return area
    const band = Math.min(area.height, PANE_BAR + BOOKMARK_BAR)
    return { ...area, y: area.y + band, height: area.height - band }
  }

  // ───────────────────────── Layout ─────────────────────────

  /** Recomputes the layout; on Mac, also adjusts the traffic lights. */
  update(win: BrowserWindow): void {
    this.panes = this.compute(win)
    const compactMode = this.d.store.state.settings.compactMode
    const { compact } = this.d
    const buttons = this.horizontal || this.sidebarRight ? !compactMode || compact.floating : this.sidebarOn || (compactMode && compact.floating)
    // Traffic lights: Mac only (elsewhere, the system frame keeps its buttons).
    if (IS_MAC && this.buttonsVisible !== buttons) {
      this.buttonsVisible = buttons
      win.setWindowButtonVisibility(buttons)
    }
    const lights = this.trafficLights()
    if (IS_MAC && lights.y !== this.lightsY) {
      this.lightsY = lights.y
      win.setWindowButtonPosition(lights)
    }
  }

  private compute(win: BrowserWindow): Pane[] {
    const { store, floating, compact, find, views, pip, devtools, develop, bookmarks, previews } = this.d
    const { state } = store
    const space = activeSpace(state)
    const { width: W, height: H } = win.getContentBounds()
    const sidebarWidth = this.liveSidebarWidth ?? state.sidebarWidth
    const sidebarVisible = this.sidebarOn
    const compactMode = state.settings.compactMode
    if (compactMode) compact.create()
    floating.warm()
    const right = this.sidebarRight
    const side = sidebarVisible ? sidebarWidth : PAD
    const x0 = right ? PAD : side
    const top = this.horizontal ? TAB_BAR + TAB_BAR_GAP : PAD
    const area = compactMode
      ? { x: COMPACT_MARGIN, y: COMPACT_MARGIN, width: W - COMPACT_MARGIN * 2, height: H - COMPACT_MARGIN * 2 }
      : { x: x0, y: top, width: W - PAD - side, height: H - top - PAD }

    this.contentArea = area
    const ids = displayedTabIds(state, space)
    find.follow(space.activeTabId)
    const split = displayedSplit(state, space)
    const geometry = computeSplit(area, split ?? { tabIds: ids })
    this.dividers = split ? geometry.dividers.map((d) => ({ ...d, splitId: split.id })) : []
    const active = space.activeTabId
    const desired: WebContentsView[] = []
    const panes = ids.map((tabId, i): Pane => {
      const owned = pip.owns(tabId)
      // Built-in developer tools: they take their share of the card, on the right or at the bottom.
      const { page: card, tools } = owned ? { page: geometry.rects[i], tools: undefined } : devtools.carve(tabId, geometry.rects[i])
      // Compact mode: no panel bar, the view fills the card. Bookmarks bar under the address bar if shown.
      const bar = compactMode ? 0 : PANE_BAR + (bookmarks.visible ? BOOKMARK_BAR : 0)
      const area = { x: card.x, y: card.y + bar, width: card.width, height: Math.max(0, card.height - bar) }
      const view = views.ensureView(tabId)
      // Responsive mode: the page takes the device's size, centered under its bar.
      const responsive = owned ? null : develop.place(tabId, area)
      const rect = responsive?.frame ?? area
      // While the page is in the mini player, the main window does not display it.
      if (!owned) {
        this.put(view, rect, 12)
        desired.push(view)
      } else this.applied.delete(view)
      if (tools) {
        this.put(tools.view, tools.rect, 12)
        desired.push(tools.view)
      }
      const pane: Pane = { tabId, rect, card, focused: tabId === active }
      if (tools) pane.devtools = { dock: tools.dock, card: tools.card, rect: tools.rect, divider: tools.divider }
      if (responsive) pane.responsive = { ...responsive.responsive, bar: responsive.bar, area: responsive.area, scale: responsive.scale }
      return pane
    })

    const findPane = panes.find((p) => p.focused)
    if (find.isOpen && findPane && find.view) {
      this.put(find.view, find.bounds(findPane.rect))
      desired.push(find.view)
    }
    if ((floating.shown || floating.leaving) && !sidebarVisible && floating.sidebar) {
      this.put(floating.sidebar.view, { x: right ? W - 6 - sidebarWidth : 6, y: 6, width: sidebarWidth, height: H - 12 }, 14)
      desired.push(floating.sidebar.view)
    }
    // A popover of the tab row keeps the floating bar under it (its anchor).
    const keepStrip = this.horizontal && (this.overlay.popoverOpen || this.overlay.kind === 'dropzones')
    if (this.overlay.open && !keepStrip) compact.hide()
    const toolbarView = compact.view
    if (compactMode && (compact.floating || compact.leaving) && toolbarView) {
      this.put(toolbarView, this.horizontal ? compact.stripBounds(W) : compact.bounds(area), 14)
      desired.push(toolbarView)
    }
    // Archive notice: small transparent view centered at the bottom of the web card (under the overlay, above the pages).
    const toast = this.toast.view
    if (toast) {
      const tw = Math.min(TOAST_W, Math.max(0, area.width))
      this.put(toast, {
        x: Math.round(area.x + (area.width - tw) / 2),
        y: Math.max(0, area.y + area.height - TOAST_H),
        width: tw,
        height: TOAST_H
      })
      desired.push(toast)
    }
    // Tab preview on hover: above the pages and the floating bars, under the overlay.
    const preview = previews.placed
    if (preview && !this.overlay.open) {
      this.put(preview.view, preview.bounds)
      desired.push(preview.view)
    }
    const ctx: LayoutContext = { W, H, panes, ids, desired }
    this.overlay.place(ctx)
    const peekView = this.peek.place(ctx)
    const selected = active ? views.viewOf(active)?.webContents : undefined
    if (selected !== this.selectedContents) {
      this.selectedContents = selected
      this.d.extensions.select(selected)
    }
    this.d.media.onLayout(ids)
    this.shownTabs = ids
    this.d.fullscreen.onLayout(ids)
    this.d.sleep.trackHidden(ids)
    this.d.permissions.onLayout()
    this.d.siteColor.onShown(ids)
    previews.onShown(ids)
    // Page fullscreen: only that tab's view, over the whole window; every other layer is out of the stack.
    const fsId = this.d.fullscreen.tabId
    const fsView = fsId && ids.includes(fsId) && !pip.owns(fsId) ? views.viewOf(fsId) : undefined
    if (fsView) {
      this.put(fsView, { x: 0, y: 0, width: W, height: H }, 0)
      desired.length = 0
      desired.push(fsView)
    }
    this.setStack(win, desired)
    this.applyFocus(win, ids, peekView)
    devtools.afterLayout(ids)
    return panes
  }
}

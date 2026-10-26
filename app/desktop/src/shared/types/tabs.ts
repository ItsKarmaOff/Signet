import type { DevResponsive, Rect, Zone } from '@signet/shared/types'
/** Media playing in a tab other than the displayed one. */
export interface MediaInfo {
  tabId: string
  title: string
  artist?: string
  /** Cover art URL (page metadata), otherwise absent. */
  artwork?: string
  site: string
  playing: boolean
  muted: boolean
  /** True if the page declares previous / next tracks. */
  canSkip: boolean
}

/** What an icon is being chosen for (an emoji). */
export type IconTarget = { kind: 'tab'; tabId: string } | { kind: 'folder'; folderId: string } | { kind: 'group'; groupId: string }

export interface Pane {
  tabId: string
  /** Area of the web view. */
  rect: Rect
  /** Whole card: pane toolbar (top) + web view. */
  card: Rect
  focused: boolean
  /** Devtools docked next to the page: their card (header + view), their view, and the divider. */
  devtools?: { dock: 'right' | 'bottom'; card: Rect; rect: Rect; divider: Rect }
  /** Responsive mode: device bar, area around the page, display scale (the web view is `rect`). */
  responsive?: DevResponsive & { bar: Rect; area: Rect; scale: number }
}

/** State of the find-in-page bar. Pushed on `find:state` to the `#find` view. */
export interface FindState {
  /** Reset (new id) on each opening, so the UI starts from a clean field. */
  session: number
  query: string
  /** Rank of the active match (1-based), 0 if none. */
  active: number
  matches: number
  hue: number
  /** True during the exit animation: the pill fades out before the view is removed. */
  closing?: boolean
}

/** Content of a tab's hover preview. Pushed on `preview:show` to the `#preview` view. */
export interface TabPreviewPayload {
  tabId: string
  title: string
  url: string
  favicon?: string
  /** Thumbnail (JPEG as a data URL), if the tab was displayed recently. */
  thumb?: string
  audible: boolean
  /** Unloaded (sleeping) tab. */
  asleep: boolean
  /** True when the card appears (enter animation); false when it moves from one tab to another. */
  fresh: boolean
  /** Side the card comes from: right or left of the sidebar, or under the tab row. */
  from: 'right' | 'left' | 'below'
}

/** What the capture tool takes: an area (web view coordinates), the visible part or the whole page. */
export type CaptureTarget = { kind: 'region'; rect: Rect } | { kind: 'visible' } | { kind: 'full' }

/** Target of a drag and drop in the sidebar. */
export interface DropTarget {
  zone: Zone
  /** Target space (ignored for favorites). */
  spaceId: string
  /** Target folder (`pinned` zone only); absent = zone root. */
  folderId?: string
  /** Target group (`today` zone only): the dropped item joins it if it touches this group. */
  groupId?: string
  /** Insertion position in the target list (root or the folder's `items`). */
  index: number
}

/** What is being dragged: a tab, a whole split view, a whole folder or a whole group (by its header). */
export type DragSource = { kind: 'tab'; tabId: string } | { kind: 'split'; id: string } | { kind: 'folder'; id: string } | { kind: 'group'; id: string }

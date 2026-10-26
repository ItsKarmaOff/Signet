import type { DevOverrides, DevToolsDock } from './devtools'
import type { LiveFolder } from './spaces'

/**
 * A sidebar item: a single tab, a split view, or a folder.
 * Folders only exist among the pinned tabs and can be nested.
 *
 * Tab group (day tabs only): a run of at least two consecutive items carrying the same
 * `group`; its title, color and collapsed state are in `Space.groups`. Tabs opened from a member
 * are added at the end of the group (created as needed around the tab that opens).
 */
export type Item =
  | { kind: 'tab'; tabId: string; group?: string }
  | {
      kind: 'split'
      id: string
      group?: string
      /** 2 to 4 tabs. */
      tabIds: string[]
      /** Disposition ; absente = `columns`. */
      layout?: SplitLayout
      /** Parts (sum 1). `columns` / `rows`: one part per tab, in `tabIds` order. `grid`: [top row, bottom row]. Missing = equal parts. */
      sizes?: number[]
    }
  | {
      kind: 'folder'
      id: string
      name: string
      /** Emoji chosen in place of the folder icon. */
      icon?: string
      open: boolean
      items: Item[]
      /** Hue of the folder card; missing = neutral hue. */
      hue?: number
      /** Live folder: its tabs come from a source (feed, GitHub) and update on their own. */
      live?: LiveFolder
    }

/** Where an item lives in the sidebar. */
export type Zone = 'favorites' | 'pinned' | 'today'

export interface TabInfo {
  id: string
  url: string
  title: string
  favicon?: string
  /** "Home" URL of pinned tabs and favorites. Absent for day tabs. */
  pinnedUrl?: string
  loading: boolean
  canGoBack: boolean
  canGoForward: boolean
  /** True if the tab has a live web view (otherwise it is unloaded). */
  loaded: boolean
  audible: boolean
  muted: boolean
  /** Timestamp (ms) of the last activation, used for automatic archiving. */
  lastActive: number
  /** Title chosen by the user; takes precedence over `title` when displayed. */
  customTitle?: string
  /** Emoji chosen by the user; takes precedence over the favicon when displayed. */
  customIcon?: string
  /** Color of the top of the page (`#rrggbb`), to blend the toolbar into the site. Missing = unknown. */
  themeColor?: string
  /** Reader mode: `available` if the page looks like an article, `on` if it is shown. Missing = unavailable. */
  reader?: 'available' | 'on'
  /** Language declared by the page (`<html lang>`) when it is not among the preferred languages: translation is offered. */
  foreignLang?: string
  /** Page translation: in progress, or shown. Missing = original page. */
  translation?: 'working' | 'on'
  /** Key button in the address bar: typed password not yet saved, or accounts already saved for the site. */
  passwordKey?: 'pending' | 'saved'
  /** Item of a live folder not yet opened ("new" dot). */
  liveUnread?: boolean
  /** Developer mode, local page: errors and warnings written to the console since loading. Missing = none. */
  consoleErrors?: number
  consoleWarnings?: number
  /** Development settings active on the tab (Development menu). Missing = none. */
  devOverrides?: DevOverrides
  /** Developer tools open, and their location. Missing = closed. */
  devtools?: DevToolsDock
}

export type SplitLayout = 'columns' | 'rows' | 'grid'

/** Header of a day tab group. */
export interface TabGroup {
  name: string
  /** Emoji chosen in place of the first tab's icon. */
  icon?: string
  /** Hue of the card (0-359). */
  hue: number
  /** Collapsed: only the header remains (with the active tab if it is a member). */
  open: boolean
}

export interface Rect {
  x: number
  y: number
  width: number
  height: number
}

/** Resizable divider between two panes of a split view (in the gap between the views). */
export interface Divider {
  splitId: string
  /** Index of the boundary in `sizes` (see SPEC). */
  index: number
  orientation: 'vertical' | 'horizontal'
  rect: Rect
}

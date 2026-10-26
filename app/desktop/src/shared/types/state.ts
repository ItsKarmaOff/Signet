import type { Divider, DownloadInfo, Settings, Space, TabInfo } from '@signet/shared/types'
import type { MediaInfo, Pane } from './tabs'

/** Everything the main window needs to render itself. Pushed on the `state` channel. */
export interface Snapshot {
  tabs: Record<string, TabInfo>
  favorites: string[]
  spaces: Space[]
  activeSpaceId: string
  sidebarVisible: boolean
  sidebarWidth: number
  /** Areas occupied by the displayed web views, in window coordinates. */
  panes: Pane[]
  settings: Settings
  /** Newest to oldest, 30 max. */
  downloads: DownloadInfo[]
  /** True when the hidden sidebar is shown floating (`#sidebar` view). */
  sidebarFloating: boolean
  /** True when, in compact mode, the toolbar is shown floating (`#toolbar` view). */
  toolbarFloating: boolean
  /** Dividers of the displayed split view. */
  dividers: Divider[]
  /** Most recent media of a tab that is not displayed, or null. */
  media: MediaInfo | null
  /** Sites (`siteOf`) that have an active Boost, for the toolbar indicator. */
  boostedSites: string[]
  /** True while the overlay (palette, popover, site panel, Peek) covers the main window. */
  overlayOpen: boolean
}

/** Partial state update: only the changed fields of tabs already sent. */
export interface StatePatch {
  tabs: Record<string, Partial<TabInfo>>
}

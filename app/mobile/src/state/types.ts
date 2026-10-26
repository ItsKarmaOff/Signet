import type { Bookmark, HistoryEntry, SpaceDraft, TabInfo } from '@signet/shared/types'
import type { DownloadEntry } from '@signet/shared/mobile/downloads'
import type { StoredLink } from '@signet/shared/mobile/link'
import type { MobileSettings } from '@signet/shared/mobile/settings'
import type { UiState } from '@signet/shared/mobile/persist'
import type { ClosedBatch, MoveTarget, OpenTabOptions, TabLocation, Workspace } from '@signet/shared/mobile/workspace'

export interface WorkspaceSlice {
  workspace: Workspace
  /** Last public tab closed, for "Undo" (never a private tab). */
  lastClosed: { tab: TabInfo; at: TabLocation } | null
  /** Last of today's tabs closed at once ("Close today's tabs"), for "Undo". */
  lastClosedBatch: ClosedBatch | null
  /** Opens a tab and returns its id. */
  openTab: (opts: OpenTabOptions) => string
  closeTab: (id: string) => void
  /** Reopens the last closed tab. */
  undoCloseTab: () => void
  activateTab: (id: string) => void
  updateTab: (id: string, patch: Partial<TabInfo>) => void
  moveTab: (id: string, to: MoveTarget) => void
  pinTab: (id: string, pinned?: boolean) => void
  favoriteTab: (id: string, favorite?: boolean) => void
  /** Closes a space's today tabs and keeps the batch for `undoCloseTodayTabs`. */
  closeTodayTabs: (spaceId?: string) => void
  undoCloseTodayTabs: () => void
  /** Duplicates a tab (the copy follows the original, without becoming the displayed tab) and returns its id. */
  duplicateTab: (id: string) => string
  /** Sends a tab to another space, in the same zone. */
  moveTabToSpace: (id: string, spaceId: string) => void
  setPrivateMode: (on: boolean) => void
  closePrivateTabs: () => void
  setActiveSpace: (id: string) => void
  /** Creates a space, activates it and returns its id. */
  addSpace: (draft: SpaceDraft) => string
  editSpace: (id: string, draft: SpaceDraft) => void
  removeSpace: (id: string) => void
  moveSpace: (id: string, to: number) => void
  /**
   * Call when a tab's page has navigated (WebView): updates the tab and, outside private browsing,
   * adds the page to history. This is THE entry point for visits: do not call `addVisit` by hand.
   */
  reportNavigation: (id: string, page: { url: string; title?: string; favicon?: string }, patch?: Partial<TabInfo>) => void
}

export interface HistorySlice {
  history: HistoryEntry[]
  addVisit: (visit: { url: string; title?: string; favicon?: string }) => void
  removeHistoryEntry: (url: string) => void
  /** Clears all history, or the visits made since `since` (ms). */
  clearHistory: (since?: number) => void
  /** Puts a deleted entry back in place ("Undo"). */
  restoreHistoryEntry: (entry: HistoryEntry) => void
  /** Remembers the icon of a page already in history. */
  setHistoryFavicon: (url: string, favicon: string) => void
}

export interface BookmarksSlice {
  bookmarks: Bookmark[]
  addBookmark: (link: { title: string; url: string; favicon?: string }, folderId?: string) => void
  addBookmarkFolder: (name: string, parentId?: string) => void
  removeBookmark: (id: string) => void
  removeBookmarkByUrl: (url: string) => void
  updateBookmark: (id: string, patch: { title?: string; url?: string; name?: string }) => void
  moveBookmark: (id: string, target: { folderId?: string; index: number }) => void
  /** Adds the page to bookmarks, or removes it if present; returns true if it is there now. */
  toggleBookmark: (link: { title: string; url: string; favicon?: string }) => boolean
  /** Reinserts a deleted node (link or folder with its contents) at its former place ("Undo"). */
  insertBookmark: (node: Bookmark, folderId?: string, index?: number) => void
  /** Moves a bookmark up (-1) or down (+1) in its folder. */
  moveBookmarkBy: (id: string, delta: number) => void
  /** Adds imported bookmarks in a new folder at the end of the root. */
  importBookmarks: (nodes: Bookmark[], folderName: string) => void
  /** Remembers the icon of bookmarks pointing to this address. */
  setBookmarkFavicon: (url: string, favicon: string) => void
}

export interface SettingsSlice {
  settings: MobileSettings
  /** Updates settings (sanitized values: an invalid value is ignored). */
  setSettings: (patch: Partial<MobileSettings>) => void
  resetSettings: () => void
}

export interface UiSlice {
  ui: UiState
  /** False until storage reading is done: show nothing but a background before then. */
  hydrated: boolean
  setOnboardingDone: (done?: boolean) => void
}

export interface DownloadsSlice {
  /** Downloads, newest to oldest. Saved separately (own AsyncStorage key), see `slices/downloads.ts`. */
  downloads: DownloadEntry[]
  addDownload: (entry: DownloadEntry) => void
  updateDownload: (id: string, patch: Partial<Omit<DownloadEntry, 'id'>>) => void
  removeDownload: (id: string) => void
  /** Empties the list (in-progress downloads stay). */
  clearDownloads: () => void
}

export interface LinksSlice {
  /** Paired computers (each one's token is in the Keychain, never here). Saved separately, see `slices/links.ts`. */
  links: StoredLink[]
  /** True once the list has been read from storage. */
  linksLoaded: boolean
  addLink: (link: StoredLink) => void
  patchLink: (linkId: string, patch: Partial<Omit<StoredLink, 'linkId'>>) => void
  removeLink: (linkId: string) => void
}

export type AppState = WorkspaceSlice & HistorySlice & BookmarksSlice & SettingsSlice & UiSlice & DownloadsSlice & LinksSlice

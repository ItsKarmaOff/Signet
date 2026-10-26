import { create } from 'zustand'
import { createBookmarksSlice } from './slices/bookmarks'
import { createDownloadsSlice } from './slices/downloads'
import { createHistorySlice } from './slices/history'
import { createLinksSlice } from './slices/links'
import { createSettingsSlice } from './slices/settings'
import { createUiSlice } from './slices/ui'
import { createWorkspaceSlice } from './slices/workspace'
import type { AppState } from './types'

/** The app store: a single source of truth, split into slices (see `slices/`). */
export const useStore = create<AppState>()((...a) => ({
  ...createWorkspaceSlice(...a),
  ...createHistorySlice(...a),
  ...createBookmarksSlice(...a),
  ...createSettingsSlice(...a),
  ...createUiSlice(...a),
  ...createDownloadsSlice(...a),
  ...createLinksSlice(...a)
}))

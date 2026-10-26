// Pure selectors on the state: `useStore(selectCurrentTab)`. Those returning a new array go through a hook in `hooks.ts`.
import { isBookmarked } from '@signet/shared/mobile/bookmarks'
import { activeSpace, currentTab, tabsOf, visibleTabIds } from '@signet/shared/mobile/workspace'
import type { AppState } from './types'

export const selectActiveSpace = (s: AppState) => activeSpace(s.workspace)
export const selectCurrentTab = (s: AppState) => currentTab(s.workspace)
export const selectSpaces = (s: AppState) => s.workspace.spaces
export const selectPrivateMode = (s: AppState) => s.workspace.privateMode
export const selectVisibleTabs = (s: AppState) => tabsOf(s.workspace, visibleTabIds(s.workspace))
export const selectIsBookmarked = (url: string) => (s: AppState) => isBookmarked(s.bookmarks, url)

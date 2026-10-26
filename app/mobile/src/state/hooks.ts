import { useShallow } from 'zustand/react/shallow'
import { selectActiveSpace, selectCurrentTab, selectIsBookmarked, selectPrivateMode, selectSpaces, selectVisibleTabs } from './selectors'
import { useStore } from './store'

/** Active space (even in private browsing: it is the one we return to). */
export const useActiveSpace = () => useStore(selectActiveSpace)
export const useSpaces = () => useStore(selectSpaces)
/** Tab shown in the browser. */
export const useCurrentTab = () => useStore(selectCurrentTab)
/** Switcher tabs: those of the active space (favorites, pinned, today) or, in private, the private tabs. */
export const useVisibleTabs = () => useStore(useShallow(selectVisibleTabs))
export const usePrivateMode = () => useStore(selectPrivateMode)
export const useSettings = () => useStore((s) => s.settings)
export const useIsBookmarked = (url: string) => useStore(selectIsBookmarked(url))

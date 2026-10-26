// Display modes derived from settings: sidebar or tab row, sidebar side, compact mode.
import type { State } from '../core/store'

type Display = Pick<State, 'settings' | 'sidebarVisible'>

/** "Tabs on top" layout: no sidebar, a tab row at the top of the window. */
export const isHorizontal = (state: Pick<State, 'settings'>): boolean => state.settings.tabLayout === 'horizontal'

/** Sidebar on the right (vertical layout only): the traffic lights move onto the card at the top left. */
export const isSidebarRight = (state: Pick<State, 'settings'>): boolean => state.settings.sidebarSide === 'right' && !isHorizontal(state)

/** Sidebar actually shown: never in compact mode nor with tabs on top. */
export const isSidebarOn = (state: Display): boolean => state.sidebarVisible && !state.settings.compactMode && !isHorizontal(state)

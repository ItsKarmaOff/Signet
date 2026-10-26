// Keyboard navigation between tabs, in the order the bar shows them.
import type { Item } from '@shared/types'
import { itemTabIds } from '@signet/shared/tree'
import type { State } from '../core/store'
import { activeSpace } from './locate'

/** Next or previous tab: favorites, pinned, then day tabs. */
export function cycleTarget(state: State, step: number): string | undefined {
  const space = activeSpace(state)
  const ids = [...state.favorites, ...[...space.pinned, ...space.today].flatMap(itemTabIds)]
  if (ids.length === 0) return undefined
  const i = ids.indexOf(space.activeTabId ?? '')
  const n = ids.length
  return ids[i < 0 ? (step < 0 ? n - 1 : 0) : (i + step + n) % n]
}

/**
 * ⌘1 to ⌘8: nth tab as the bar shows it (favorites, pinned, day); ⌘9: the last. A collapsed folder hides
 * its tabs (and in the "tabs on top" layout, all folders are collapsed).
 */
export function numberTarget(state: State, n: number, horizontal: boolean): string | undefined {
  const open = (items: Item[]): string[] => items.flatMap((i) => (i.kind === 'folder' ? (i.open && !horizontal ? open(i.items) : []) : itemTabIds(i)))
  const space = activeSpace(state)
  const ids = [...state.favorites, ...open(space.pinned), ...open(space.today)]
  return n >= 9 ? ids[ids.length - 1] : ids[n - 1]
}

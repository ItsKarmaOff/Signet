import type { DragSource, DropTarget, Item } from '@shared/types'
import type { Indicator } from './TabRow'

/** Today tab groups: runs of consecutive items carrying the same `group` (header in `Space.groups`). */

export const groupOf = (item: Item | undefined): string | undefined => (item && item.kind !== 'folder' ? item.group : undefined)

export interface Run {
  /** Absent: single item, no group. */
  group?: string
  start: number
  end: number
}

/** Splits the list into runs: a whole group, or a single item. */
export function runsOf(items: Item[]): Run[] {
  const runs: Run[] = []
  for (let i = 0; i < items.length;) {
    const group = groupOf(items[i])
    let end = i + 1
    if (group) while (end < items.length && groupOf(items[end]) === group) end++
    runs.push(group && end - i > 1 ? { group, start: i, end } : { start: i, end: i + 1 })
    i = end
  }
  return runs
}

/**
 * Where to draw the insertion marker in today tabs: "after" the last member when dropping at the end
 * of a group to join it (or at the end of the list), otherwise "before" the targeted item.
 */
export function todayMark(items: Item[], drop: (DropTarget & { into?: string }) | null): { index: number; side: Indicator; card?: boolean } | null {
  if (!drop || drop.zone !== 'today' || drop.folderId || drop.into) return null
  // End of list with no group targeted and last item in a group: land after the card, without joining it.
  if (!drop.groupId && items.length > 0 && drop.index >= items.length && groupOf(items[items.length - 1]))
    return { index: items.length - 1, side: 'after', card: true }
  const joinsEnd = !!drop.groupId && groupOf(items[drop.index - 1]) === drop.groupId && groupOf(items[drop.index]) !== drop.groupId
  if (joinsEnd || drop.index >= items.length) return { index: drop.index - 1, side: 'after' }
  return { index: drop.index, side: 'before' }
}

/**
 * Dragging a whole group over another group: never insert in the middle,
 * only before or after it depending on the hovered half of its card.
 */
export function aroundRun(card: Element, run: Run, pointer: number, horizontal: boolean, current: number | null = null): number {
  const r = card.getBoundingClientRect()
  // Final position: remove the in-progress FLIP offsets from the ancestors (rows slide under the pointer).
  let dx = 0
  let dy = 0
  for (let n: Element | null = card; n; n = n.parentElement) {
    if (!n.hasAttribute('data-flip-live')) continue
    const m = new DOMMatrixReadOnly(getComputedStyle(n).transform)
    dx += m.m41
    dy += m.m42
  }
  const mid = horizontal ? r.left - dx + r.width / 2 : r.top - dy + r.height / 2
  // 4 px dead band around the middle: keep the current target if it touches this group.
  if ((current === run.start || current === run.end) && Math.abs(pointer - mid) <= 4) return current
  return pointer < mid ? run.start : run.end
}

/** Dragging a block (whole group or folder that will become a group): it never lands in the middle of a group. */
export const isBlock = (src: DragSource | null): boolean => src?.kind === 'group' || src?.kind === 'folder'

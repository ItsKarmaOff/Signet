// Advanced split view: computing rectangles and dividers (pure functions, no Electron)
// and inserting a tab into a split view.
import type { Divider, Rect, SplitLayout } from './types'

export const GAP = 8
/** Minimum size of a pane (width or height depending on the axis), in pixels. */
export const MIN_PANE = 200
export const MAX_PANES = 4

export type DropEdge = 'left' | 'right' | 'top' | 'bottom'

/** Part of a split view needed for the computation (subset of `Item`). */
export interface SplitShape {
  tabIds: string[]
  layout?: SplitLayout
  sizes?: number[]
}

export interface SplitGeometry {
  /** One rectangle per tab, in `tabIds` order. */
  rects: Rect[]
  dividers: Omit<Divider, 'splitId'>[]
  /** Layout actually applied (`grid` becomes `columns` below 3 panes). */
  layout: SplitLayout
}

/** Effective layout: the grid only exists from 3 panes. */
export function effectiveLayout(layout: SplitLayout | undefined, n: number): SplitLayout {
  if (layout === 'rows') return 'rows'
  if (layout === 'grid' && n >= 3) return 'grid'
  return 'columns'
}

/** Number of panes in the top row of a grid. */
export const topCount = (n: number): number => Math.ceil(n / 2)

/** Expected number of parts per layout. */
export const partCount = (layout: SplitLayout, n: number): number => (layout === 'grid' ? 2 : n)

/** Valid parts (positive, sum 1) or equal parts. */
export function normalizeParts(sizes: number[] | undefined, count: number): number[] {
  const equal = Array.from({ length: count }, () => 1 / count)
  if (!sizes || sizes.length !== count || sizes.some((s) => !Number.isFinite(s) || s <= 0)) return equal
  const sum = sizes.reduce((a, b) => a + b, 0)
  return sum > 0 ? sizes.map((s) => s / sum) : equal
}

/**
 * Lengths (sum = `total`) matching the parts, each at least `min`.
 * Parts that are too small are raised to the minimum, the others share the remainder proportionally.
 * If `total` does not allow the minimum everywhere, all lengths are equal.
 */
export function clampLengths(parts: number[], total: number, min = MIN_PANE): number[] {
  const n = parts.length
  if (n === 0) return []
  if (total < n * min) return Array.from({ length: n }, () => total / n)
  const fixed = new Array<boolean>(n).fill(false)
  const out = parts.map((p) => p * total)
  for (let pass = 0; pass < n; pass++) {
    let changed = false
    for (let i = 0; i < n; i++) {
      if (!fixed[i] && out[i] < min) {
        out[i] = min
        fixed[i] = true
        changed = true
      }
    }
    const freeSum = parts.reduce((s, p, i) => (fixed[i] ? s : s + p), 0)
    const rest = total - fixed.reduce((s, f) => s + (f ? min : 0), 0)
    for (let i = 0; i < n; i++) if (!fixed[i]) out[i] = freeSum > 0 ? (parts[i] / freeSum) * rest : rest / fixed.filter((f) => !f).length
    if (!changed) break
  }
  return out
}

/** Splits `total` (excluding gaps) into rounded pieces whose sum is exact. */
function segments(start: number, lengths: number[]): { pos: number; len: number }[] {
  const out: { pos: number; len: number }[] = []
  let acc = 0
  let pos = start
  lengths.forEach((l, i) => {
    acc += l
    const end = Math.round(start + acc + GAP * i)
    out.push({ pos: Math.round(pos), len: end - Math.round(pos) })
    pos = end + GAP
  })
  return out
}

/** Rectangles and dividers of the split view within `area`. */
export function computeSplit(area: Rect, split: SplitShape): SplitGeometry {
  const n = split.tabIds.length
  const layout = effectiveLayout(split.layout, n)
  if (n <= 1) return { rects: n === 1 ? [{ ...area }] : [], dividers: [], layout: 'columns' }

  const parts = normalizeParts(split.sizes, partCount(layout, n))
  const dividers: SplitGeometry['dividers'] = []

  if (layout === 'columns') {
    const segs = segments(area.x, clampLengths(parts, area.width - GAP * (n - 1)))
    const rects = segs.map((s) => ({ x: s.pos, y: area.y, width: s.len, height: area.height }))
    for (let i = 0; i < n - 1; i++) {
      dividers.push({ index: i, orientation: 'vertical', rect: { x: rects[i].x + rects[i].width, y: area.y, width: GAP, height: area.height } })
    }
    return { rects, dividers, layout }
  }

  if (layout === 'rows') {
    const segs = segments(area.y, clampLengths(parts, area.height - GAP * (n - 1)))
    const rects = segs.map((s) => ({ x: area.x, y: s.pos, width: area.width, height: s.len }))
    for (let i = 0; i < n - 1; i++) {
      dividers.push({ index: i, orientation: 'horizontal', rect: { x: area.x, y: rects[i].y + rects[i].height, width: area.width, height: GAP } })
    }
    return { rects, dividers, layout }
  }

  // Grid: two rows, equal columns in each row.
  const [top, bottom] = segments(area.y, clampLengths(parts, area.height - GAP))
  const nTop = topCount(n)
  const rects: Rect[] = []
  const row = (count: number, y: number, height: number) => {
    for (const s of segments(
      area.x,
      Array.from({ length: count }, () => (area.width - GAP * (count - 1)) / count)
    )) {
      rects.push({ x: s.pos, y, width: s.len, height })
    }
  }
  row(nTop, top.pos, top.len)
  row(n - nTop, bottom.pos, bottom.len)
  dividers.push({ index: 0, orientation: 'horizontal', rect: { x: area.x, y: top.pos + top.len, width: area.width, height: GAP } })
  return { rects, dividers, layout }
}

/** Parts from measured lengths (any sum). */
export const partsFromLengths = (lengths: number[]): number[] => {
  const sum = lengths.reduce((a, b) => a + b, 0)
  return sum > 0 ? lengths.map((l) => l / sum) : lengths.map(() => 1 / lengths.length)
}

/** Adds `tabId` to the split view, before (left/top) or after (right/bottom) `focusedId`. False if it is full. */
export function insertIntoSplit(split: SplitShape, tabId: string, edge: DropEdge, focusedId: string | null): boolean {
  if (split.tabIds.length >= MAX_PANES || split.tabIds.includes(tabId)) return false
  const at = focusedId ? split.tabIds.indexOf(focusedId) : -1
  const base = at < 0 ? split.tabIds.length - 1 : at
  split.tabIds.splice(edge === 'left' || edge === 'top' ? base : base + 1, 0, tabId)

  const n = split.tabIds.length
  const was = effectiveLayout(split.layout, n - 1)
  const horizontal = edge === 'left' || edge === 'right'
  let next: SplitLayout
  if (was === 'grid') next = 'grid'
  else if (horizontal) next = was === 'rows' && n >= 3 ? 'grid' : 'columns'
  else next = was === 'columns' && n >= 3 ? 'grid' : 'rows'
  split.layout = next
  delete split.sizes
  return true
}

/** Puts the split view back in a consistent state after a removal or a read from disk. */
export function sanitizeSplit(split: SplitShape): void {
  const layout = effectiveLayout(split.layout, split.tabIds.length)
  if (layout === 'columns') delete split.layout
  else split.layout = layout
  const count = partCount(layout, split.tabIds.length)
  if (split.sizes && (split.sizes.length !== count || split.sizes.some((x) => !Number.isFinite(x) || x <= 0))) delete split.sizes
}

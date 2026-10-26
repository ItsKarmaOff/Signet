import { describe, expect, it } from 'vitest'
import {
  clampLengths,
  computeSplit,
  effectiveLayout,
  GAP,
  insertIntoSplit,
  MAX_PANES,
  normalizeParts,
  partsFromLengths,
  sanitizeSplit,
  type SplitShape
} from './split'

const area = { x: 10, y: 20, width: 1000, height: 600 }

describe('effectiveLayout', () => {
  it('the grid needs 3 panes', () => {
    expect(effectiveLayout('grid', 2)).toBe('columns')
    expect(effectiveLayout('grid', 3)).toBe('grid')
    expect(effectiveLayout('rows', 2)).toBe('rows')
    expect(effectiveLayout(undefined, 4)).toBe('columns')
  })
})

describe('normalizeParts / clampLengths / partsFromLengths', () => {
  it('valid parts normalized to 1, otherwise equal', () => {
    expect(normalizeParts([1, 3], 2)).toEqual([0.25, 0.75])
    expect(normalizeParts([1, 3], 3)).toEqual([1 / 3, 1 / 3, 1 / 3])
    expect(normalizeParts([1, -1], 2)).toEqual([0.5, 0.5])
    expect(normalizeParts([1, NaN], 2)).toEqual([0.5, 0.5])
    expect(normalizeParts(undefined, 2)).toEqual([0.5, 0.5])
  })

  it('clampLengths respects the minimum and the sum', () => {
    const out = clampLengths([0.05, 0.95], 1000, 200)
    expect(out[0]).toBe(200)
    expect(out[0] + out[1]).toBeCloseTo(1000)
    expect(clampLengths([0.9, 0.1], 300, 200)).toEqual([150, 150]) // too narrow: equal parts
    expect(clampLengths([], 100)).toEqual([])
  })

  it('partsFromLengths', () => {
    expect(partsFromLengths([100, 300])).toEqual([0.25, 0.75])
    expect(partsFromLengths([0, 0])).toEqual([0.5, 0.5])
  })
})

describe('computeSplit', () => {
  it('0 and 1 tab', () => {
    expect(computeSplit(area, { tabIds: [] })).toEqual({ rects: [], dividers: [], layout: 'columns' })
    const one = computeSplit(area, { tabIds: ['a'] })
    expect(one.rects).toEqual([area])
    expect(one.rects[0]).not.toBe(area)
  })

  it('columns: covers the whole width with a GAP gap', () => {
    const g = computeSplit(area, { tabIds: ['a', 'b'] })
    expect(g.layout).toBe('columns')
    expect(g.rects).toHaveLength(2)
    expect(g.rects[0].x).toBe(10)
    expect(g.rects[0].width + GAP + g.rects[1].width).toBe(1000)
    expect(g.rects[1].x).toBe(g.rects[0].x + g.rects[0].width + GAP)
    expect(g.dividers).toEqual([{ index: 0, orientation: 'vertical', rect: { x: g.rects[0].x + g.rects[0].width, y: 20, width: GAP, height: 600 } }])
  })

  it('columns: respects the parts', () => {
    const g = computeSplit(area, { tabIds: ['a', 'b'], sizes: [0.25, 0.75] })
    expect(g.rects[0].width).toBe(Math.round(0.25 * (1000 - GAP)))
  })

  it('lignes', () => {
    const g = computeSplit(area, { tabIds: ['a', 'b', 'c'], layout: 'rows' })
    expect(g.layout).toBe('rows')
    expect(g.rects.every((r) => r.x === 10 && r.width === 1000)).toBe(true)
    expect(g.rects.reduce((s, r) => s + r.height, 0) + 2 * GAP).toBe(600)
    expect(g.dividers.map((d) => d.orientation)).toEqual(['horizontal', 'horizontal'])
  })

  it('3-pane grid: 2 on top, 1 on the bottom, one horizontal divider', () => {
    const g = computeSplit(area, { tabIds: ['a', 'b', 'c'], layout: 'grid' })
    expect(g.layout).toBe('grid')
    expect(g.rects).toHaveLength(3)
    expect(g.rects[0].y).toBe(g.rects[1].y)
    expect(g.rects[2].width).toBe(1000)
    expect(g.rects[2].y).toBeGreaterThan(g.rects[0].y)
    expect(g.dividers).toHaveLength(1)
  })

  it('2-pane grid becomes columns', () => {
    expect(computeSplit(area, { tabIds: ['a', 'b'], layout: 'grid' }).layout).toBe('columns')
  })
})

describe('insertIntoSplit', () => {
  const mk = (tabIds: string[], layout?: SplitShape['layout']): SplitShape => ({ tabIds, layout, sizes: [1, 1] })

  it('refuses a duplicate or a full view', () => {
    expect(insertIntoSplit(mk(['a', 'b']), 'a', 'right', 'a')).toBe(false)
    expect(insertIntoSplit(mk(['a', 'b', 'c', 'd']), 'e', 'right', 'a')).toBe(false)
    expect(MAX_PANES).toBe(4)
  })

  it('inserts before/after the focused tab', () => {
    const s = mk(['a', 'b'])
    insertIntoSplit(s, 'x', 'left', 'b')
    expect(s.tabIds).toEqual(['a', 'x', 'b'])
    const t = mk(['a', 'b'])
    insertIntoSplit(t, 'x', 'right', 'a')
    expect(t.tabIds).toEqual(['a', 'x', 'b'])
  })

  it('without focus, appends at the end (right) or before the last (left)', () => {
    const s = mk(['a', 'b'])
    insertIntoSplit(s, 'x', 'right', null)
    expect(s.tabIds).toEqual(['a', 'b', 'x'])
    const t = mk(['a', 'b'])
    insertIntoSplit(t, 'x', 'left', 'inconnu')
    expect(t.tabIds).toEqual(['a', 'x', 'b'])
  })

  it('updates the layout and clears the parts', () => {
    const s = mk(['a', 'b'])
    insertIntoSplit(s, 'x', 'bottom', 'a')
    expect(s.layout).toBe('grid') // columns + vertical add with 3 panes
    expect(s.sizes).toBeUndefined()
    const t = mk(['a', 'b'], 'rows')
    insertIntoSplit(t, 'x', 'right', 'a')
    expect(t.layout).toBe('grid')
    const u = mk(['a', 'b'])
    insertIntoSplit(u, 'x', 'top', 'a')
    expect(u.layout).toBe('grid')
    const w = mk(['a', 'b'], 'rows')
    insertIntoSplit(w, 'x', 'bottom', 'a')
    expect(w.layout).toBe('rows')
    const v = mk(['a', 'b', 'c'])
    insertIntoSplit(v, 'x', 'bottom', 'a')
    expect(v.layout).toBe('grid')
  })
})

describe('sanitizeSplit', () => {
  it('columns -> no layout, inconsistent parts removed', () => {
    const s: SplitShape = { tabIds: ['a', 'b'], layout: 'columns', sizes: [1] }
    sanitizeSplit(s)
    expect(s.layout).toBeUndefined()
    expect(s.sizes).toBeUndefined()
  })

  it('2-pane grid -> columns; valid grid kept', () => {
    const s: SplitShape = { tabIds: ['a', 'b'], layout: 'grid' }
    sanitizeSplit(s)
    expect(s.layout).toBeUndefined()
    const g: SplitShape = { tabIds: ['a', 'b', 'c'], layout: 'grid', sizes: [0.4, 0.6] }
    sanitizeSplit(g)
    expect(g).toEqual({ tabIds: ['a', 'b', 'c'], layout: 'grid', sizes: [0.4, 0.6] })
  })
})

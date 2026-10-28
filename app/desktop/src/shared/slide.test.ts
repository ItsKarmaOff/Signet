import { describe, expect, it } from 'vitest'
import { slideAt, slideRect, slideState, type SidebarSlide, type SlidePlan } from './slide'

// Area laid out from x 8 (width 1000), stretched to the sidebar's edge at x 208 (width 800).
const slide: SidebarSlide = { progress: 0.8, sidebar: -40, from: { x: 8, width: 1000 }, to: { x: 208, width: 800 } }

describe('slideRect', () => {
  it('stretches a full-width card into the remaining space', () => {
    expect(slideRect({ x: 8, y: 8, width: 1000, height: 600 }, slide)).toEqual({ x: 208, y: 8, width: 800, height: 600 })
  })

  it('keeps split panes proportional', () => {
    expect(slideRect({ x: 508, y: 8, width: 500, height: 600 }, slide)).toEqual({ x: 608, y: 8, width: 400, height: 600 })
  })

  it('only moves a fixed-width rect, following its right edge', () => {
    expect(slideRect({ x: 708, y: 60, width: 300, height: 40 }, slide, true)).toEqual({ x: 708, y: 60, width: 300, height: 40 })
  })

  it('leaves a rect alone when nothing moves', () => {
    const still: SidebarSlide = { progress: 1, sidebar: 0, from: { x: 8, width: 1000 }, to: { x: 8, width: 1000 } }
    expect(slideRect({ x: 100, y: 0, width: 50, height: 10 }, still)).toEqual({ x: 100, y: 0, width: 50, height: 10 })
  })
})

describe('slideAt', () => {
  const plan: SlidePlan = { from: { x: 8, width: 1000 }, width: 248, gap: 8, right: false, motion: { start: 0, from: 0, velocity: 0, target: 1 } }

  it('leaves the whole area to the pages when the bar is hidden', () => {
    expect(slideAt(plan, 0)).toMatchObject({ sidebar: -248, to: { x: 8, width: 1000 } })
  })

  it('gives the bar its width when shown', () => {
    expect(slideAt(plan, 1)).toMatchObject({ sidebar: 0, to: { x: 248, width: 760 } })
  })

  it('mirrors on the right', () => {
    expect(slideAt({ ...plan, right: true }, 0.5)).toMatchObject({ sidebar: 124, to: { x: 8, width: 880 } })
  })
})

describe('slideState', () => {
  const motion = { start: 1000, from: 1, velocity: 0, target: 0 as const }

  it('starts where it is, still', () => {
    expect(slideState(motion, 1000)).toEqual({ progress: 1, velocity: 0, done: false })
  })

  it('moves toward the target without overshooting', () => {
    const mid = slideState(motion, 1100)
    expect(mid.progress).toBeGreaterThan(0)
    expect(mid.progress).toBeLessThan(1)
    expect(mid.velocity).toBeLessThan(0)
  })

  it('settles on the target', () => {
    expect(slideState(motion, 3000)).toEqual({ progress: 0, velocity: 0, done: true })
  })

  it('reads the same position at the same time (main and UI agree)', () => {
    expect(slideState(motion, 1150)).toEqual(slideState({ ...motion }, 1150))
  })
})

import { describe, expect, it, vi } from 'vitest'

vi.mock('electron', () => ({ BrowserWindow: class {}, screen: {}, systemPreferences: {} }))

import { resizedBounds } from './pip'

const wa = { x: 0, y: 0, width: 1920, height: 1080 }
const RATIO = 16 / 9
/** 480×270 window in the middle of the screen. */
const s = { x: 800, y: 400, width: 480, height: 270 }

describe('resizedBounds', () => {
  it('east edge: grows to the right, keeps the proportions and the left anchor', () => {
    const r = resizedBounds(s, 'e', 160, 0, RATIO, wa, false)
    expect(r.width).toBe(640)
    expect(r.height).toBe(360)
    expect(r.x).toBe(800)
    // free axis centered
    expect(r.y).toBe(Math.round(400 + (270 - 360) / 2))
  })

  it('west edge: the anchor is the right edge', () => {
    const r = resizedBounds(s, 'w', -160, 0, RATIO, wa, false)
    expect(r.width).toBe(640)
    expect(r.x + r.width).toBe(1280)
  })

  it('south edge: the width follows the wanted height', () => {
    const r = resizedBounds(s, 's', 0, 90, RATIO, wa, false)
    expect(r.height).toBe(360)
    expect(r.width).toBe(640)
    expect(r.y).toBe(400)
  })

  it('southeast corner: anchor at top left', () => {
    const r = resizedBounds(s, 'se', 160, 90, RATIO, wa, false)
    expect(r).toEqual({ x: 800, y: 400, width: 640, height: 360 })
  })

  it('northwest corner: anchor at bottom right', () => {
    const r = resizedBounds(s, 'nw', -160, -90, RATIO, wa, false)
    expect(r.x + r.width).toBe(1280)
    expect(r.y + r.height).toBe(670)
  })

  it('largeur minimale 240 (dur)', () => {
    expect(resizedBounds(s, 'e', -1000, 0, RATIO, wa, false).width).toBe(240)
  })

  it('maximum: 90% of the work area height (hard)', () => {
    const left = { ...s, x: 100 }
    const r = resizedBounds(left, 'e', 5000, 0, RATIO, wa, false)
    expect(r.width).toBe(Math.floor(1080 * 0.9 * RATIO))
  })

  it('never crosses the screen edge', () => {
    const near = { x: 1600, y: 400, width: 200, height: 112 }
    const r = resizedBounds(near, 'e', 5000, 0, RATIO, wa, true)
    expect(r.x + r.width).toBeLessThanOrEqual(1920)
    expect(r.width).toBe(320)
  })

  it('rubber band: beyond the minimum, the window shrinks more slowly than the pointer', () => {
    const r = resizedBounds(s, 'e', -400, 0, RATIO, wa, true)
    expect(r.width).toBeLessThan(240)
    expect(r.width).toBeGreaterThan(80)
    expect(r.width).toBeGreaterThan(480 - 400) // resistance
  })

  it('integer result, proportions respected to within rounding', () => {
    const r = resizedBounds(s, 'se', 33, 17, RATIO, wa, false)
    for (const n of Object.values(r)) expect(Number.isInteger(n)).toBe(true)
    expect(Math.abs(r.width / r.height - RATIO)).toBeLessThan(0.02)
  })
})

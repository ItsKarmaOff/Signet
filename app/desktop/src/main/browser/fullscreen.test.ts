import { describe, expect, it } from 'vitest'
import { canFill, fitRect } from './fullscreen'

describe('canFill', () => {
  it('accepts a displayed tab', () => {
    expect(canFill('a', ['a', 'b'], false)).toBe(true)
  })
  it('refuses a hidden tab, a mini player tab and no tab', () => {
    expect(canFill('c', ['a', 'b'], false)).toBe(false)
    expect(canFill('a', ['a'], true)).toBe(false)
    expect(canFill(null, ['a'], false)).toBe(false)
  })
})

describe('fitRect', () => {
  const normal = { x: 8, y: 48, width: 500, height: 300 }
  it('keeps the normal rectangle', () => {
    expect(fitRect(false, { width: 900, height: 700 }, normal)).toBe(normal)
  })
  it('fills the content area when fullscreen', () => {
    expect(fitRect(true, { width: 900, height: 700 }, normal)).toEqual({ x: 0, y: 0, width: 900, height: 700 })
  })
})

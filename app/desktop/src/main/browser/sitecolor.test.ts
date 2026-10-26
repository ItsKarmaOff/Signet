import { describe, expect, it, vi } from 'vitest'

vi.mock('electron', () => ({ BrowserWindow: class {} }))

import { dominantColor } from './sitecolor'

/** BGRA row from [r, g, b, a] pixels. */
const row = (...px: [number, number, number, number][]) => Buffer.from(px.flatMap(([r, g, b, a]) => [b, g, r, a]))

describe('dominantColor', () => {
  it('couleur unie', () => {
    expect(dominantColor(row([10, 20, 30, 255], [10, 20, 30, 255]), 2)).toBe('#0a141e')
  })
  it('the most frequent color wins', () => {
    expect(dominantColor(row([255, 0, 0, 255], [0, 0, 255, 255], [0, 0, 255, 255]), 3)).toBe('#0000ff')
  })
  it('ignores transparent pixels', () => {
    expect(dominantColor(row([255, 0, 0, 0], [0, 255, 0, 255]), 2)).toBe('#00ff00')
    expect(dominantColor(row([255, 0, 0, 0]), 1)).toBeNull()
  })
  it('averages close shades in the same bucket', () => {
    expect(dominantColor(row([0x10, 0x10, 0x10, 255], [0x1e, 0x1e, 0x1e, 255]), 2)).toBe('#171717')
  })
  it('empty buffer or shorter than the width', () => {
    expect(dominantColor(Buffer.alloc(0), 10)).toBeNull()
    expect(dominantColor(row([1, 2, 3, 255]), 10)).toBe('#010203')
  })
})

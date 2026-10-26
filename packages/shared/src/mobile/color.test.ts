import { describe, expect, it } from 'vitest'
import {
  accentColor,
  hexToRgb,
  hslToRgb,
  luminance,
  mixHex,
  previewColors,
  rgbToHex,
  spaceGradient,
  spacePalette,
  spaceThemeVars,
  tintBoost,
  withAlpha
} from './color'

describe('color', () => {
  it('hsl -> hex connus', () => {
    expect(rgbToHex(hslToRgb(0, 1, 0.5))).toBe('#ff0000')
    expect(rgbToHex(hslToRgb(120, 1, 0.5))).toBe('#00ff00')
    expect(rgbToHex(hslToRgb(0, 0, 1))).toBe('#ffffff')
  })

  it('hex <-> rgb and alpha', () => {
    expect(rgbToHex(hexToRgb('#1a2b3c'))).toBe('#1a2b3c')
    expect(withAlpha('#ff0000', 0.5)).toBe('rgba(255, 0, 0, 0.5)')
    expect(hexToRgb('nope')).toEqual([0, 0, 0])
  })

  it('the accent is equalized: similar lightness for all hues (unlike HSL)', () => {
    for (const scheme of ['light', 'dark'] as const) {
      const lums = [0, 45, 90, 150, 215, 250, 285, 330].map((h) => luminance(accentColor(h, scheme)))
      expect(Math.max(...lums) - Math.min(...lums)).toBeLessThan(0.12)
    }
    const hsl = [60, 250].map((h) => luminance(rgbToHex(hslToRgb(h, 0.7, 0.55))))
    expect(Math.abs(hsl[0] - hsl[1])).toBeGreaterThan(0.3)
  })

  it('the dark theme has a lighter accent than the light theme', () => {
    expect(luminance(accentColor(250, 'dark'))).toBeGreaterThan(luminance(accentColor(250, 'light')))
  })

  it('tintBoost: maximal around 295°, zero beyond 90°', () => {
    expect(tintBoost(295)).toBeCloseTo(1.7)
    expect(tintBoost(115)).toBe(1)
  })

  it('mixHex: bounds and midpoint', () => {
    expect(mixHex('#000000', '#ffffff', 0)).toBe('#000000')
    expect(mixHex('#000000', '#ffffff', 1)).toBe('#ffffff')
    expect(luminance(mixHex('#000000', '#ffffff', 0.5))).toBeGreaterThan(0.1)
  })

  it('the palette is tinted more with a greater strength, and stays light / dark', () => {
    const light = spacePalette(250, 'light')
    const dark = spacePalette(250, 'dark')
    expect(luminance(light.background)).toBeGreaterThan(luminance(dark.background))
    const d = (p: string) => Math.abs(luminance(p) - luminance(spacePalette(250, 'light', 'subtle').background))
    expect(d(spacePalette(250, 'light', 'strong').background)).toBeGreaterThan(0)
    for (const c of [light.accent, light.background, light.surface, dark.border]) expect(c).toMatch(/^#[0-9a-f]{6}$/)
  })

  it('spaceThemeVars: solid repeats the hue, gradient keeps the 2nd and 3rd (the 3rd falls back to the 2nd)', () => {
    expect(spaceThemeVars({ hue: 40 })).toEqual({ h1: 40, h2: 40, h3: 40, gk: 1, grain: 0 })
    expect(spaceThemeVars({ hue: 10, theme: { hues: [10, 200], grain: 0.4 } })).toEqual({ h1: 10, h2: 200, h3: 200, gk: 2.6, grain: 0.4 })
  })

  it('spaceGradient: always at least 2 colors, 3 if the theme has 3', () => {
    expect(spaceGradient({ hue: 40 }, 'light').colors).toHaveLength(2)
    expect(spaceGradient({ hue: 10, theme: { hues: [10, 200], grain: 0 } }, 'dark').colors).toHaveLength(2)
    expect(spaceGradient({ hue: 10, theme: { hues: [10, 100, 200], grain: 0 } }, 'dark').colors).toHaveLength(3)
  })

  it('previewColors: like desktop themeBackground (hsl(h 70% 62%))', () => {
    expect(previewColors(undefined, 0)).toEqual([rgbToHex(hslToRgb(0, 0.7, 0.62))])
    expect(previewColors({ hues: [0, 120], grain: 0 }, 0)).toHaveLength(2)
  })
})

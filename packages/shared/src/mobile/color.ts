// Space colors: the desktop computation (styles/tokens.css + lib/util.ts) ported to pure functions.
// Desktop lets the browser evaluate `oklch(from hsl(var(--h) 70% 55%) 0.55 0.16 h)`; here we compute it by hand
// so the mobile app (React Native has no CSS) gets the same hues. All colors come out as `#rrggbb`.
import type { Space, SpaceTheme, TintStrength } from '../types'

export type Scheme = 'light' | 'dark'
type Rgb = [number, number, number]
type Lab = [number, number, number]

/** Gradient strength relative to the solid hue (same constant as desktop). */
export const GRADIENT_STRENGTH = 2.6

const clamp01 = (n: number) => Math.min(1, Math.max(0, n))
const toLinear = (c: number) => (c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4)
const toGamma = (c: number) => (c <= 0.0031308 ? c * 12.92 : 1.055 * c ** (1 / 2.4) - 0.055)

/** Hue wrapped into [0, 360[. */
export const normalizeHue = (h: number): number => ((h % 360) + 360) % 360

/** HSL (h in degrees, s and l between 0 and 1) to sRGB (0-1). */
export function hslToRgb(h: number, s: number, l: number): Rgb {
  const a = s * Math.min(l, 1 - l)
  const f = (n: number) => {
    const k = (n + normalizeHue(h) / 30) % 12
    return l - a * Math.max(-1, Math.min(k - 3, 9 - k, 1))
  }
  return [f(0), f(8), f(4)]
}

function rgbToOklab([r, g, b]: Rgb): Lab {
  const [lr, lg, lb] = [toLinear(r), toLinear(g), toLinear(b)]
  const l = Math.cbrt(0.4122214708 * lr + 0.5363325363 * lg + 0.0514459929 * lb)
  const m = Math.cbrt(0.2119034982 * lr + 0.6806995451 * lg + 0.1073969566 * lb)
  const s = Math.cbrt(0.0883024619 * lr + 0.2817188376 * lg + 0.6299787005 * lb)
  return [
    0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s,
    1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s,
    0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s
  ]
}

function oklabToLinear([L, a, b]: Lab): Rgb {
  const l = (L + 0.3963377774 * a + 0.2158037573 * b) ** 3
  const m = (L - 0.1055613458 * a - 0.0638541728 * b) ** 3
  const s = (L - 0.0894841775 * a - 1.291485548 * b) ** 3
  return [
    4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
    -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
    -0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s
  ]
}

const inGamut = (rgb: Rgb) => rgb.every((c) => c >= -0.0005 && c <= 1.0005)

/** OKLCH (L 0-1, C, h in degrees) to sRGB (0-1); out of gamut, chroma is reduced until it fits in sRGB. */
export function oklchToRgb(L: number, C: number, h: number): Rgb {
  const rad = (normalizeHue(h) * Math.PI) / 180
  const at = (c: number) => oklabToLinear([L, c * Math.cos(rad), c * Math.sin(rad)])
  let lin = at(C)
  if (!inGamut(lin)) {
    let lo = 0
    let hi = C
    for (let i = 0; i < 24; i++) {
      const mid = (lo + hi) / 2
      if (inGamut(at(mid))) lo = mid
      else hi = mid
    }
    lin = at(lo)
  }
  return [clamp01(toGamma(clamp01(lin[0]))), clamp01(toGamma(clamp01(lin[1]))), clamp01(toGamma(clamp01(lin[2])))]
}

/** OKLCH hue (degrees) of an sRGB color: this is the `h` of `oklch(from <color> … h)` in CSS. */
export function oklchHue(rgb: Rgb): number {
  const [, a, b] = rgbToOklab(rgb)
  return normalizeHue((Math.atan2(b, a) * 180) / Math.PI)
}

export function rgbToHex([r, g, b]: Rgb): string {
  const byte = (c: number) => Math.round(clamp01(c) * 255)
  return '#' + [r, g, b].map((c) => byte(c).toString(16).padStart(2, '0')).join('')
}

export function hexToRgb(hex: string): Rgb {
  const m = /^#?([0-9a-f]{6})$/i.exec(hex.trim())
  if (!m) return [0, 0, 0]
  const n = parseInt(m[1], 16)
  return [((n >> 16) & 255) / 255, ((n >> 8) & 255) / 255, (n & 255) / 255]
}

/** `#rrggbb` + opacity -> `rgba(r, g, b, a)` (React Native does not accept `#rrggbbaa` everywhere). */
export function withAlpha(hex: string, alpha: number): string {
  const [r, g, b] = hexToRgb(hex).map((c) => Math.round(c * 255))
  return `rgba(${r}, ${g}, ${b}, ${Math.round(clamp01(alpha) * 1000) / 1000})`
}

/** Mix of `a` toward `b` (`t` from 0 to 1) in OKLab space, like `color-mix(in oklch)` for small amounts. */
export function mixHex(a: string, b: string, t: number): string {
  const la = rgbToOklab(hexToRgb(a))
  const lb = rgbToOklab(hexToRgb(b))
  const k = clamp01(t)
  const lab: Lab = [la[0] + (lb[0] - la[0]) * k, la[1] + (lb[1] - la[1]) * k, la[2] + (lb[2] - la[2]) * k]
  return rgbToHex(oklchFromLab(lab))
}

function oklchFromLab(lab: Lab): Rgb {
  const C = Math.hypot(lab[1], lab[2])
  return oklchToRgb(lab[0], C, (Math.atan2(lab[2], lab[1]) * 180) / Math.PI)
}

/** WCAG relative luminance (0-1), to pick readable text on a color. */
export function luminance(hex: string): number {
  const [r, g, b] = hexToRgb(hex).map(toLinear)
  return 0.2126 * r + 0.7152 * g + 0.0722 * b
}

/** OKLCH hue of `hsl(hue 70% 55%)`: desktop's starting color, from which the accent and background tint derive. */
const baseHue = (hue: number, s = 0.7, l = 0.55) => oklchHue(hslToRgb(hue, s, l))

/** Hue boost around 295° (blue, purple), where cool gray backgrounds absorb the tint (desktop's `--tint-boost`). */
export function tintBoost(hue: number): number {
  const rad = ((hue - 295) * Math.PI) / 180
  return 1 + 0.7 * Math.max(0, Math.cos(rad))
}

/** Accent of a space: fixed lightness (white on top stays readable for every hue). */
export function accentColor(hue: number, scheme: Scheme): string {
  return scheme === 'dark' ? rgbToHex(oklchToRgb(0.72, 0.14, baseHue(hue, 0.75, 0.68))) : rgbToHex(oklchToRgb(0.55, 0.16, baseHue(hue)))
}

/** Soft tint of the space, used to color the backgrounds (`--tint-color`). */
export function tintColor(hue: number, scheme: Scheme): string {
  const boost = tintBoost(hue)
  return scheme === 'dark' ? rgbToHex(oklchToRgb(0.7, 0.12 * boost, baseHue(hue))) : rgbToHex(oklchToRgb(0.62, 0.13 * boost, baseHue(hue)))
}

const TINT_K: Record<TintStrength, number> = { subtle: 0.5, normal: 1, strong: 2 }

export interface SpacePalette {
  accent: string
  /** Text placed on the accent. */
  onAccent: string
  tint: string
  /** Screen background, slightly tinted. */
  background: string
  /** Cards and list rows. */
  surface: string
  /** Background of fields and chips. */
  surface2: string
  border: string
  /** Background of a selection (active row). */
  selection: string
}

const BASE = {
  light: { bg: '#ececef', surface: '#ffffff', surface2: '#f0f0f2', border: '#dcdce0', dose: 0.07 },
  dark: { bg: '#18181b', surface: '#232327', surface2: '#2c2c31', border: '#3a3a40', dose: 0.08 }
} as const

/** Screen neutrals tinted 2 to 8% by the space hue (the computation of desktop's `--bg`, `--surface`…). */
export function spacePalette(hue: number, scheme: Scheme, strength: TintStrength = 'normal'): SpacePalette {
  const k = TINT_K[strength]
  const base = BASE[scheme]
  const tint = tintColor(hue, scheme)
  const accent = accentColor(hue, scheme)
  return {
    accent,
    onAccent: scheme === 'dark' ? '#121216' : '#ffffff',
    tint,
    background: mixHex(base.bg, tint, base.dose * k),
    surface: mixHex(base.surface, tint, 0.02 * k),
    surface2: mixHex(base.surface2, tint, 0.04 * k),
    border: mixHex(base.border, tint, 0.1 * k),
    selection: withAlpha(tint, scheme === 'dark' ? 0.18 : 0.14)
  }
}

/** Hues of a space as desktop sets them: `h1`, `h2`, `h3`, gradient strength `gk` and grain. A solid space repeats its hue. */
export function spaceThemeVars(space: Pick<Space, 'hue' | 'theme'>): { h1: number; h2: number; h3: number; gk: number; grain: number } {
  const hues = space.theme?.hues
  const gradient = !!hues && hues.length >= 2
  const [h1, h2, h3] = gradient ? [hues[0], hues[1], hues[2] ?? hues[1]] : [space.hue, space.hue, space.hue]
  return { h1, h2, h3, gk: gradient ? GRADIENT_STRENGTH : 1, grain: gradient ? (space.theme?.grain ?? 0) : 0 }
}

/** Preview colors of a space (swatch, form): one for a solid space, 2 or 3 for a gradient (desktop's `themeBackground`). */
export function previewColors(theme: SpaceTheme | undefined, hue: number): string[] {
  const hues = theme?.hues ?? []
  const list = hues.length >= 2 ? hues : [hues[0] ?? hue]
  return list.map((h) => rgbToHex(hslToRgb(h, 0.7, 0.62)))
}

export interface GradientSpec {
  colors: string[]
  /** Start and end points for `expo-linear-gradient` (desktop's 135°: top left to bottom right). */
  start: { x: number; y: number }
  end: { x: number; y: number }
}

/** Background gradient of a space, already tinted for the light or dark theme. Always at least 2 colors (a solid space gives a single repeated color). */
export function spaceGradient(space: Pick<Space, 'hue' | 'theme'>, scheme: Scheme): GradientSpec {
  const v = spaceThemeVars(space)
  const hues = v.gk > 1 ? [v.h1, v.h2, v.h3].slice(0, space.theme?.hues.length === 2 ? 2 : 3) : [v.h1]
  const colors = hues.map((h) => tintColor(h, scheme))
  if (colors.length === 1) colors.push(colors[0])
  return { colors, start: { x: 0, y: 0 }, end: { x: 1, y: 1 } }
}

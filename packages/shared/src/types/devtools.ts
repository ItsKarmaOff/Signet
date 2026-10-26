import type { Rect } from './tabs'

/** Local name (Settings › Development): `mysite`, `mysite.test` or `mysite.localhost`, never a real domain. */
export interface DevAlias {
  name: string
  /** Local address (origin, and optional path), without trailing `/`. */
  target: string
}

/** Simulated connection (Chromium presets). */
export type DevNetwork = 'fast4g' | 'slow4g' | '3g' | 'offline'

/** Simulated user agent. */
export type DevUserAgent = 'safari-mac' | 'safari-iphone' | 'safari-ipad' | 'chrome-android' | 'firefox-mac'

/** Device simulated by responsive mode (see app/desktop/src/shared/devices.ts); `custom` = hand-picked size. */
export type DevDevice =
  'iphone-se' | 'iphone-16' | 'iphone-16-pro-max' | 'pixel-9' | 'galaxy-s24' | 'ipad-mini' | 'ipad-pro-11' | 'laptop' | 'desktop' | 'custom'

/** Responsive mode of a tab: device and size of the simulated screen (CSS px, in the displayed orientation). */
export interface DevResponsive {
  device: DevDevice
  width: number
  height: number
  /** `custom` only: device whose size was left (density, touch and agent inherited). */
  base?: DevDevice
  /** `custom` only: simulates a mobile device (touch, viewport, agent); missing = same as `base`. */
  mobile?: boolean
}

/** Development settings of a tab (Development menu), while it is open. Missing field = normal behavior. */
export interface DevOverrides {
  /** Cache disabled. */
  noCache?: boolean
  /** JavaScript disabled. */
  noJs?: boolean
  /** Theme announced to the page (`prefers-color-scheme`). */
  scheme?: 'light' | 'dark'
  /** `prefers-reduced-motion: reduce`. */
  reducedMotion?: boolean
  network?: DevNetwork
  userAgent?: DevUserAgent
  /** Responsive mode (the page takes the device size, centered in the panel). */
  responsive?: DevResponsive
}

/** Location of a tab's developer tools: card on the right or bottom of the page, or a separate window. */
export type DevToolsDock = 'right' | 'bottom' | 'window'

/** Last location and sizes of the tools, restored on next open. */
export interface DevToolsPrefs {
  dock: DevToolsDock
  /** Width of the tools on the right (px). */
  width: number
  /** Height of the tools at the bottom (px). */
  height: number
  /** Position and size of the detached tools window. */
  bounds?: Rect
}

import type { DevDevice, DevResponsive, DevUserAgent } from './types'

/** Responsive mode device: screen size in CSS px (portrait), density, mobile (touch, viewport), user agent. */
export interface DeviceSpec {
  id: DevDevice
  /** The device's own name; absent = translated label (`develop.device.<id>`). */
  name?: string
  kind: 'phone' | 'tablet' | 'computer' | 'custom'
  width: number
  height: number
  dpr: number
  mobile: boolean
  userAgent?: DevUserAgent
}

export const DEVICES: readonly DeviceSpec[] = [
  { id: 'iphone-se', name: 'iPhone SE', kind: 'phone', width: 375, height: 667, dpr: 2, mobile: true, userAgent: 'safari-iphone' },
  { id: 'iphone-16', name: 'iPhone 16', kind: 'phone', width: 393, height: 852, dpr: 3, mobile: true, userAgent: 'safari-iphone' },
  { id: 'iphone-16-pro-max', name: 'iPhone 16 Pro Max', kind: 'phone', width: 440, height: 956, dpr: 3, mobile: true, userAgent: 'safari-iphone' },
  { id: 'pixel-9', name: 'Pixel 9', kind: 'phone', width: 412, height: 923, dpr: 2.625, mobile: true, userAgent: 'chrome-android' },
  { id: 'galaxy-s24', name: 'Galaxy S24', kind: 'phone', width: 360, height: 780, dpr: 3, mobile: true, userAgent: 'chrome-android' },
  { id: 'ipad-mini', name: 'iPad mini', kind: 'tablet', width: 744, height: 1133, dpr: 2, mobile: true, userAgent: 'safari-ipad' },
  { id: 'ipad-pro-11', name: 'iPad Pro 11"', kind: 'tablet', width: 834, height: 1210, dpr: 2, mobile: true, userAgent: 'safari-ipad' },
  { id: 'laptop', kind: 'computer', width: 1280, height: 800, dpr: 2, mobile: false },
  { id: 'desktop', kind: 'computer', width: 1920, height: 1080, dpr: 1, mobile: false },
  { id: 'custom', kind: 'custom', width: 800, height: 600, dpr: 2, mobile: false }
]

export const DEFAULT_DEVICE: DevDevice = 'iphone-16'
/** Bounds of the size chosen by hand (CSS px). */
export const MIN_DEVICE = 200
export const MAX_DEVICE = 3840

export function deviceSpec(id: DevDevice | undefined): DeviceSpec {
  return DEVICES.find((d) => d.id === id) ?? DEVICES.find((d) => d.id === DEFAULT_DEVICE)!
}

/** Emulated profile (density, mobile, agent): the device's, or inherited from `base` for a free size. */
export function responsiveProfile(r: DevResponsive): { dpr: number; mobile: boolean; userAgent?: DevUserAgent } {
  const spec = deviceSpec(r.device)
  if (r.device !== 'custom') return { dpr: spec.dpr, mobile: spec.mobile, userAgent: spec.userAgent }
  const base = r.base && r.base !== 'custom' ? deviceSpec(r.base) : spec
  const mobile = r.mobile ?? base.mobile
  if (!mobile) return { dpr: base.dpr, mobile: false }
  // Mobile wanted without a mobile base: default device profile.
  const m = base.mobile ? base : deviceSpec(DEFAULT_DEVICE)
  return { dpr: m.dpr, mobile: true, userAgent: m.userAgent }
}

/** Switches to free size while keeping the current device's profile. */
export function asCustom(r: DevResponsive, size: { width?: number; height?: number } = {}): DevResponsive {
  const out: DevResponsive = { device: 'custom', width: size.width ?? r.width, height: size.height ?? r.height }
  if (r.device === 'custom') {
    if (r.base) out.base = r.base
    if (r.mobile !== undefined) out.mobile = r.mobile
  } else {
    const spec = deviceSpec(r.device)
    out.base = spec.id
    out.mobile = spec.mobile
  }
  return out
}

import { describe, expect, it } from 'vitest'
import { DEFAULT_DEVICE, DEVICES, MAX_DEVICE, MIN_DEVICE, asCustom, deviceSpec, responsiveProfile } from './devices'

describe('DEVICES', () => {
  it('has unique ids and includes the default device and "custom"', () => {
    const ids = DEVICES.map((d) => d.id)
    expect(new Set(ids).size).toBe(ids.length)
    expect(ids).toContain(DEFAULT_DEVICE)
    expect(ids).toContain('custom')
  })

  it('stays within the size bounds', () => {
    for (const d of DEVICES) {
      expect(d.width).toBeGreaterThanOrEqual(MIN_DEVICE)
      expect(d.width).toBeLessThanOrEqual(MAX_DEVICE)
      expect(d.height).toBeGreaterThanOrEqual(MIN_DEVICE)
      expect(d.height).toBeLessThanOrEqual(MAX_DEVICE)
    }
  })

  it('gives a user agent to mobile devices only', () => {
    for (const d of DEVICES) expect(!!d.userAgent, d.id).toBe(d.mobile)
  })
})

describe('deviceSpec', () => {
  it('finds a device, otherwise takes the default one', () => {
    expect(deviceSpec('pixel-9').name).toBe('Pixel 9')
    expect(deviceSpec(undefined).id).toBe(DEFAULT_DEVICE)
    expect(deviceSpec('inconnu' as never).id).toBe(DEFAULT_DEVICE)
  })
})

describe('responsiveProfile', () => {
  it('takes over the device profile', () => {
    expect(responsiveProfile({ device: 'iphone-se', width: 375, height: 667 })).toEqual({ dpr: 2, mobile: true, userAgent: 'safari-iphone' })
    expect(responsiveProfile({ device: 'desktop', width: 1, height: 1 })).toEqual({ dpr: 1, mobile: false, userAgent: undefined })
  })

  it('in free size, inherits from the base', () => {
    expect(responsiveProfile({ device: 'custom', width: 500, height: 500, base: 'pixel-9' })).toEqual({
      dpr: 2.625,
      mobile: true,
      userAgent: 'chrome-android'
    })
    expect(responsiveProfile({ device: 'custom', width: 500, height: 500, base: 'desktop' })).toEqual({ dpr: 1, mobile: false })
  })

  it('in free size without a base: "custom" profile (non-mobile)', () => {
    expect(responsiveProfile({ device: 'custom', width: 500, height: 500 })).toEqual({ dpr: 2, mobile: false })
  })

  it('mobile wanted without a mobile base: default device profile', () => {
    expect(responsiveProfile({ device: 'custom', width: 500, height: 500, base: 'laptop', mobile: true })).toEqual({
      dpr: 3,
      mobile: true,
      userAgent: 'safari-iphone'
    })
  })

  it('mobile forced to false on a mobile base', () => {
    expect(responsiveProfile({ device: 'custom', width: 500, height: 500, base: 'iphone-16', mobile: false })).toEqual({ dpr: 3, mobile: false })
  })
})

describe('asCustom', () => {
  it('keeps the profile of the device being left', () => {
    expect(asCustom({ device: 'pixel-9', width: 412, height: 923 }, { width: 500 })).toEqual({
      device: 'custom',
      width: 500,
      height: 923,
      base: 'pixel-9',
      mobile: true
    })
  })

  it('keeps base and mobile already in free size', () => {
    expect(asCustom({ device: 'custom', width: 1, height: 2, base: 'laptop', mobile: false }, { height: 9 })).toEqual({
      device: 'custom',
      width: 1,
      height: 9,
      base: 'laptop',
      mobile: false
    })
    expect(asCustom({ device: 'custom', width: 1, height: 2 })).toEqual({ device: 'custom', width: 1, height: 2 })
  })
})

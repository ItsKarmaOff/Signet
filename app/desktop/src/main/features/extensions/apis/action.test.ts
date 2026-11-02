import { describe, expect, it, vi } from 'vitest'
import { ActionStates, parseColor, pngIcons, rgbaToBgra, setIconMethod } from './action'
import type { ApiCaller } from './types'

describe('ActionStates', () => {
  it('is enabled by default; disable without a tab is global; a tab overrides it', () => {
    const s = new ActionStates()
    expect(s.isEnabled('e', 1)).toBe(true)
    s.setEnabled('e', undefined, false)
    expect(s.isEnabled('e')).toBe(false)
    expect(s.isEnabled('e', 1)).toBe(false)
    s.setEnabled('e', 1, true)
    expect(s.isEnabled('e', 1)).toBe(true)
    expect(s.isEnabled('e', 2)).toBe(false)
    expect(s.isEnabled('other', 1)).toBe(true)
  })

  it('a per-tab disable leaves other tabs enabled and goes away with the tab', () => {
    const s = new ActionStates()
    s.setEnabled('e', 3, false)
    expect(s.isEnabled('e', 3)).toBe(false)
    expect(s.isEnabled('e', 4)).toBe(true)
    s.forgetTab(3)
    expect(s.isEnabled('e', 3)).toBe(true)
  })

  it('badge text color: white by default, per tab over global', () => {
    const s = new ActionStates()
    expect(s.textColor('e', 1)).toEqual([255, 255, 255, 255])
    s.setTextColor('e', undefined, '#000')
    s.setTextColor('e', 2, [1, 2, 3, 255])
    expect(s.textColor('e', 1)).toBe('#000')
    expect(s.textColor('e', 2)).toEqual([1, 2, 3, 255])
  })

  it('snapshot holds only what differs from the defaults, and tells listeners', () => {
    const s = new ActionStates()
    const changed = vi.fn()
    s.onChange(changed)
    expect(s.snapshot()).toEqual({})
    s.setEnabled('e', undefined, false)
    s.setEnabled('e', 5, true)
    s.setTextColor('f', 5, 'red')
    expect(s.snapshot()).toEqual({ e: { enabled: false, tabs: { 5: { enabled: true } } }, f: { tabs: { 5: { textColor: 'red' } } } })
    expect(changed).toHaveBeenCalledTimes(3)
    s.forgetExtension('e')
    expect(s.snapshot()).toEqual({ f: { tabs: { 5: { textColor: 'red' } } } })
  })

  it('router methods read the tab id from the details or a bare number and validate colors', () => {
    const s = new ActionStates()
    const m = s.methods()
    const c = { extensionId: 'e' } as ApiCaller
    m['action.disable'](c, 7)
    expect(m['action.isEnabled'](c, 7)).toBe(false)
    expect(m['action.isEnabled'](c, undefined)).toBe(true)
    m['action.setBadgeTextColor'](c, { color: [0, 0, 0, 255], tabId: 7 })
    expect(m['action.getBadgeTextColor'](c, { tabId: 7 })).toEqual([0, 0, 0, 255])
    expect(() => m['action.setBadgeTextColor'](c, { color: 12 })).toThrow()
    expect(parseColor(' #fff ')).toBe('#fff')
  })
})

const image = (w: number, h: number, fill: number[]) => ({ width: w, height: h, data: new Uint8ClampedArray(Array.from({ length: w * h }, () => fill).flat()) })

describe('imageData conversion', () => {
  it('swaps red and blue for the bitmap order', () => {
    const { bitmap, width, height } = rgbaToBgra(image(2, 1, [10, 20, 30, 40]))
    expect([width, height]).toEqual([2, 1])
    expect([...bitmap]).toEqual([30, 20, 10, 40, 30, 20, 10, 40])
  })
  it('rejects bad dimensions or data length', () => {
    expect(() => rgbaToBgra({ width: 2, height: 2, data: new Uint8ClampedArray(4) })).toThrow()
    expect(() => rgbaToBgra({ width: 0, height: 1, data: [] })).toThrow()
    expect(() => rgbaToBgra(null)).toThrow()
    expect(() => rgbaToBgra({ width: 9999, height: 1, data: [] })).toThrow()
  })
  it('one image or a dictionary of sizes gives PNG data URLs keyed by width', () => {
    const toPng = (_b: Buffer, w: number, h: number) => `png:${w}x${h}`
    expect(pngIcons(image(1, 1, [0, 0, 0, 0]), toPng)).toEqual({ '1': 'png:1x1' })
    expect(pngIcons({ '16': image(2, 2, [0, 0, 0, 0]), '32': image(4, 4, [0, 0, 0, 0]) }, toPng)).toEqual({ '2': 'png:2x2', '4': 'png:4x4' })
    expect(() => pngIcons({}, toPng)).toThrow()
    expect(() => pngIcons('x', toPng)).toThrow()
  })
  it('setIcon hands the icons and the tab to the button', () => {
    const apply = vi.fn()
    const run = setIconMethod({ toPng: () => 'png', apply })
    run({ extensionId: 'e' } as ApiCaller, { tabId: 3, imageData: image(1, 1, [1, 2, 3, 4]) })
    expect(apply).toHaveBeenCalledWith({ extensionId: 'e' }, 3, { '1': 'png' })
  })
})

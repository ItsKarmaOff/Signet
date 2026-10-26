import { beforeAll, describe, expect, it, vi } from 'vitest'

// flip.ts imports spring.ts, which reads `window.matchMedia` at import time.
let layoutPos: typeof import('./flip').layoutPos
beforeAll(async () => {
  vi.stubGlobal('window', { matchMedia: () => ({ matches: false }) })
  ;({ layoutPos } = await import('./flip'))
})

const rectEl = (left: number, top: number, parent: unknown = null, live = false) =>
  ({
    getBoundingClientRect: () => ({ left, top }),
    hasAttribute: (n: string) => live && n === 'data-flip-live',
    parentElement: parent
  }) as never

describe('layoutPos', () => {
  it('gives the position relative to the container', () => {
    const root = rectEl(100, 50)
    const el = rectEl(130, 90, root)
    expect(layoutPos(el, root)).toEqual({ x: 30, y: 40 })
  })

  it('removes the ongoing FLIP offset from animated elements (up to the excluded container)', () => {
    vi.stubGlobal('getComputedStyle', (n: { __m: { m41: number; m42: number } }) => ({ transform: n.__m }))
    vi.stubGlobal(
      'DOMMatrixReadOnly',
      class {
        m41: number
        m42: number
        constructor(t: { m41: number; m42: number }) {
          this.m41 = t.m41
          this.m42 = t.m42
        }
      }
    )
    const root = rectEl(0, 0)
    const parent = Object.assign(rectEl(0, 0, root, true), { __m: { m41: 4, m42: 6 } })
    const el = Object.assign(rectEl(50, 80, parent, true), { __m: { m41: 10, m42: 20 } })
    expect(layoutPos(el, root)).toEqual({ x: 50 - 10 - 4, y: 80 - 20 - 6 })
    vi.unstubAllGlobals()
  })
})

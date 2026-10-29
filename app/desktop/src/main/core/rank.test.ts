import { describe, expect, it } from 'vitest'
import { completable, frecency } from './rank'

const now = Date.now()

describe('frecency', () => {
  it('stays between 0 and 1', () => {
    expect(frecency(0, 0, 0, now)).toBeLessThan(0.01)
    expect(frecency(1000, 1000, now, now)).toBeCloseTo(1)
  })
  it('typed visits weigh more than plain visits', () => {
    expect(frecency(5, 5, now, now)).toBeGreaterThan(frecency(40, 0, now, now))
  })
})

describe('completable', () => {
  it('typed once, or visited at least 3 times', () => {
    expect(completable({ visits: 1, typed: 1 })).toBe(true)
    expect(completable({ visits: 3 })).toBe(true)
    expect(completable({ visits: 2 })).toBe(false)
    expect(completable({ visits: 2, typed: 0 })).toBe(false)
    expect(completable(undefined)).toBe(false)
  })
})

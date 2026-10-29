import { describe, expect, it } from 'vitest'
import { TypedNavigations } from './typed-nav'

describe('TypedNavigations', () => {
  it('counts the navigation the palette started, once', () => {
    const t = new TypedNavigations()
    t.mark('a', 'https://x.com')
    t.onStart('a', 'https://x.com/')
    expect(t.take('a')).toBe(true)
    expect(t.take('a')).toBe(false)
  })
  it('does not count before the navigation started', () => {
    const t = new TypedNavigations()
    t.mark('a', 'https://x.com/')
    expect(t.take('a')).toBe(false)
    expect(t.has('a')).toBe(true)
  })
  it('drops the marker when another navigation starts first', () => {
    const t = new TypedNavigations()
    t.mark('a', 'https://x.com/')
    t.onStart('a', 'https://other.com/')
    expect(t.has('a')).toBe(false)
    t.onStart('a', 'https://x.com/')
    expect(t.take('a')).toBe(false)
  })
  it('drops the marker when a second navigation starts before the commit', () => {
    const t = new TypedNavigations()
    t.mark('a', 'https://x.com/')
    t.onStart('a', 'https://x.com/')
    t.onStart('a', 'https://x.com/next')
    expect(t.take('a')).toBe(false)
  })
  it('keeps tabs apart and can be cleared', () => {
    const t = new TypedNavigations()
    t.mark('a', 'https://x.com/')
    t.mark('b', 'https://y.com/')
    t.onStart('b', 'https://y.com/')
    expect(t.take('a')).toBe(false)
    expect(t.take('b')).toBe(true)
    t.clear('a')
    expect(t.has('a')).toBe(false)
  })
})

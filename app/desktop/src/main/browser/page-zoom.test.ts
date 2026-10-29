import { describe, expect, it } from 'vitest'
import type { Session, WebContents } from 'electron'
import { PageZoom } from './page-zoom'

function page(url: string, ses: Session, factor = 1) {
  const wc = {
    session: ses,
    factor,
    isDestroyed: () => false,
    getURL: () => url,
    getZoomFactor: () => wc.factor,
    setZoomFactor: (f: number) => (wc.factor = f),
    getZoomLevel: () => Math.log(wc.factor) / Math.log(1.2),
    setZoomLevel: (l: number) => (wc.factor = 1.2 ** l)
  }
  return wc
}
const make = (defaultZoom: number | undefined, pages: ReturnType<typeof page>[], siteZoom: Record<string, number> = {}, privateSessions: Session[] = []) => {
  const state = { settings: { defaultZoom }, siteZoom }
  return new PageZoom({
    store: { state, schedule: () => {} } as never,
    views: { all: () => pages.map((p) => ({ webContents: p })) } as never,
    persistent: (ses) => !privateSessions.includes(ses)
  })
}

describe('PageZoom', () => {
  it('applies the default on commit, and leaves pages alone without a chosen default', () => {
    const s = {} as Session
    const a = page('https://a.com/', s)
    make(150, [a]).apply(a as unknown as WebContents)
    expect(a.factor).toBe(1.5)
    const b = page('https://b.com/', s, 2)
    make(undefined, [b]).apply(b as unknown as WebContents)
    expect(b.factor).toBe(2)
  })
  it('a site zoomed by the user keeps its zoom when the default changes; ⌘0 gives it the default back', () => {
    const s = {} as Session
    const mine = page('https://mine.com/', s, 2)
    const other = page('https://other.com/', s)
    const zoom = make(125, [mine, other])
    zoom.userZoomed(mine as unknown as WebContents)
    zoom.applyAll()
    expect(mine.factor).toBe(2)
    expect(other.factor).toBe(1.25)
    zoom.reset(mine as unknown as WebContents)
    expect(mine.factor).toBe(1.25)
    mine.factor = 3
    zoom.applyAll() // no longer a user-zoomed site
    expect(mine.factor).toBe(1.25)
  })
  it('keeps the user hosts per session', () => {
    const priv = {} as Session
    const main = {} as Session
    const a = page('https://a.com/', main, 2)
    const b = page('https://a.com/', priv, 2)
    const zoom = make(150, [a, b], {}, [priv])
    zoom.userZoomed(a as unknown as WebContents)
    zoom.applyAll()
    expect([a.factor, b.factor]).toEqual([2, 1.5])
  })
  it('reset: 100 % without a default or on an internal page, and never zooms internal pages on commit', () => {
    const s = {} as Session
    const a = page('https://a.com/', s, 2)
    make(undefined, [a]).reset(a as unknown as WebContents)
    expect(a.factor).toBe(1)
    const n = page('signet://note/1', s, 2)
    const zoom = make(150, [n])
    zoom.apply(n as unknown as WebContents)
    expect(n.factor).toBe(2)
    zoom.reset(n as unknown as WebContents)
    expect(n.factor).toBe(1)
  })

  it('remembers a user zoom, restores it after a restart even without a default, and forgets it on reset', () => {
    const s = {} as Session
    const siteZoom: Record<string, number> = {}
    const a = page('https://a.com/', s)
    const zoom = make(undefined, [a], siteZoom)
    a.factor = 1.2 ** 2
    zoom.userZoomed(a as unknown as WebContents)
    expect(siteZoom['a.com']).toBeCloseTo(2)
    // new run: empty in-memory sets, same saved zoom
    const again = page('https://a.com/x', s)
    make(150, [again], siteZoom).apply(again as unknown as WebContents)
    expect(again.factor).toBeCloseTo(1.44)
    const fresh = make(150, [again], siteZoom)
    fresh.reset(again as unknown as WebContents)
    expect(again.factor).toBe(1.5)
    expect(siteZoom['a.com']).toBeUndefined()
  })
  it('never saves the zoom of a private window', () => {
    const priv = {} as Session
    const siteZoom: Record<string, number> = {}
    const a = page('https://a.com/', priv, 2)
    const zoom = make(150, [a], siteZoom, [priv])
    zoom.userZoomed(a as unknown as WebContents)
    expect(siteZoom).toEqual({})
    zoom.applyAll()
    expect(a.factor).toBe(2) // still protected for this run
  })
})

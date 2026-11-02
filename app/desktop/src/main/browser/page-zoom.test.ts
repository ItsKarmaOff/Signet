import { describe, expect, it } from 'vitest'
import type { Session, WebContents } from 'electron'
import { PageZoom, type ZoomScope } from './page-zoom'

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
const make = (
  defaultZoom: number | undefined,
  pages: ReturnType<typeof page>[],
  siteZoom: Record<string, number> = {},
  privateSessions: Session[] = [],
  spaces: Map<Session, string> = new Map(),
  siteZoomBySpace: Record<string, Record<string, number>> = {},
  report?: (wc: WebContents, percent: number | undefined) => void
) => {
  const state = { settings: { defaultZoom }, siteZoom, siteZoomBySpace }
  return new PageZoom({
    store: { state, schedule: () => {} } as never,
    views: { all: () => pages.map((p) => ({ webContents: p })) } as never,
    report,
    scope: (ses): ZoomScope => {
      if (privateSessions.includes(ses)) return { kind: 'private' }
      const id = spaces.get(ses)
      return id ? { kind: 'space', id } : { kind: 'shared' }
    }
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

  it('an isolated space has its own zoom memory, apart from the shared one', () => {
    const normal = {} as Session
    const iso = {} as Session
    const spaces = new Map([[iso, 's1']])
    const shared: Record<string, number> = {}
    const bySpace: Record<string, Record<string, number>> = {}
    const a = page('https://a.com/', normal)
    const b = page('https://a.com/', iso)
    const zoom = make(undefined, [a, b], shared, [], spaces, bySpace)
    a.factor = 1.2 ** 2
    zoom.userZoomed(a as unknown as WebContents)
    expect(shared['a.com']).toBeCloseTo(2)
    expect(bySpace).toEqual({})
    b.factor = 1.2 ** 3
    zoom.userZoomed(b as unknown as WebContents)
    expect(bySpace.s1['a.com']).toBeCloseTo(3)
    expect(shared['a.com']).toBeCloseTo(2)
    // after a restart each session gets back its own level
    const a2 = page('https://a.com/x', normal)
    const b2 = page('https://a.com/x', iso)
    const again = make(undefined, [a2, b2], shared, [], spaces, bySpace)
    again.apply(a2 as unknown as WebContents)
    again.apply(b2 as unknown as WebContents)
    expect(a2.factor).toBeCloseTo(1.44)
    expect(b2.factor).toBeCloseTo(1.728)
  })
  it('another isolated space does not see the zoom, and reset only forgets its own memory', () => {
    const normal = {} as Session
    const one = {} as Session
    const two = {} as Session
    const spaces = new Map([
      [one, 's1'],
      [two, 's2']
    ])
    const shared: Record<string, number> = { 'a.com': 1 }
    const bySpace = { s1: { 'a.com': 2 }, s2: {} }
    const p1 = page('https://a.com/', one)
    const p2 = page('https://a.com/', two)
    const zoom = make(undefined, [p1, p2], shared, [], spaces, bySpace)
    zoom.apply(p1 as unknown as WebContents)
    zoom.apply(p2 as unknown as WebContents)
    expect(p1.factor).toBeCloseTo(1.2 ** 2)
    expect(p2.factor).toBe(1) // not the shared level 1
    void normal
    zoom.reset(p1 as unknown as WebContents)
    expect(bySpace.s1['a.com']).toBeUndefined()
    expect(shared['a.com']).toBe(1)
  })
  it('a private window never reads or writes any memory', () => {
    const priv = {} as Session
    const shared = { 'a.com': 1 }
    const bySpace: Record<string, Record<string, number>> = {}
    const a = page('https://a.com/', priv)
    const zoom = make(150, [a], shared, [priv], new Map(), bySpace)
    zoom.apply(a as unknown as WebContents)
    expect(a.factor).toBe(1.5) // the default, not the shared saved level
    a.factor = 2
    zoom.userZoomed(a as unknown as WebContents)
    expect(shared).toEqual({ 'a.com': 1 })
    expect(bySpace).toEqual({})
  })
  it('a saved zoom wins over the default, and a site without one gets the default', () => {
    const s = {} as Session
    const saved = page('https://a.com/', s)
    const other = page('https://b.com/', s)
    const zoom = make(150, [saved, other], { 'a.com': 1 })
    zoom.apply(saved as unknown as WebContents)
    zoom.apply(other as unknown as WebContents)
    expect(saved.factor).toBeCloseTo(1.2)
    expect(other.factor).toBe(1.5)
  })
})

describe('PageZoom reports', () => {
  it('tells the UI only when a page zoom changes, tab siblings included', () => {
    const s = {} as Session
    const a = page('https://a.com/', s)
    const b = page('https://a.com/other', s)
    const pdf = page('https://a.com/x.pdf', s)
    const calls: [unknown, number | undefined][] = []
    const zoom = make(undefined, [a, b, pdf], {}, [], new Map(), {}, (wc, p) => calls.push([wc, p]))
    zoom.apply(a as unknown as WebContents)
    zoom.apply(a as unknown as WebContents)
    expect(calls).toEqual([[a, 100]])
    a.factor = 1.2 ** 0.5
    b.factor = a.factor // Chromium shares the host zoom
    zoom.userZoomed(a as unknown as WebContents)
    zoom.userZoomed(a as unknown as WebContents)
    expect(calls.map((c) => [c[0] === a ? 'a' : c[0] === b ? 'b' : 'pdf', c[1]])).toEqual([
      ['a', 100],
      ['a', 110],
      ['b', 110],
      ['pdf', undefined]
    ])
  })
  it('reports the default applied at commit, and back to 100 on reset', () => {
    const s = {} as Session
    const a = page('https://a.com/', s)
    const seen: (number | undefined)[] = []
    const zoom = make(150, [a], {}, [], new Map(), {}, (_wc, p) => seen.push(p))
    zoom.apply(a as unknown as WebContents)
    expect(seen).toEqual([150])
    zoom.reset(a as unknown as WebContents)
    zoom.reset(a as unknown as WebContents)
    expect(seen).toEqual([150])
    a.factor = 2
    zoom.sync(a as unknown as WebContents)
    expect(seen).toEqual([150, 200])
  })
})

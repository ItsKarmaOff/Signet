import { describe, expect, it } from 'vitest'
import {
  cleanSiteZoom,
  defaultZoomFactor,
  rememberSiteZoom,
  SITE_ZOOM_MAX,
  fontPreferences,
  isFontSize,
  isMinFontSize,
  isZoomPercent,
  zoomHost
} from './pagesettings'

describe('fontPreferences', () => {
  it('defaults', () => {
    expect(fontPreferences({})).toEqual({ defaultFontSize: 16, defaultMonospaceFontSize: 13, minimumFontSize: 0 })
  })
  it('scales the monospace size', () => {
    expect(fontPreferences({ fontSize: 9 }).defaultMonospaceFontSize).toBe(7)
    expect(fontPreferences({ fontSize: 12 }).defaultMonospaceFontSize).toBe(10)
    expect(fontPreferences({ fontSize: 20 }).defaultMonospaceFontSize).toBe(16)
    expect(fontPreferences({ fontSize: 24 }).defaultMonospaceFontSize).toBe(20)
  })
  it('keeps a chosen minimum', () => {
    expect(fontPreferences({ minimumFontSize: 12 }).minimumFontSize).toBe(12)
  })
  it('ignores invalid values', () => {
    expect(fontPreferences({ fontSize: 17, minimumFontSize: '12' })).toEqual({ defaultFontSize: 16, defaultMonospaceFontSize: 13, minimumFontSize: 0 })
  })
})

describe('choice lists', () => {
  it('validate against the lists', () => {
    expect(isFontSize(20)).toBe(true)
    expect(isFontSize(15)).toBe(false)
    expect(isMinFontSize(0)).toBe(true)
    expect(isMinFontSize(7)).toBe(false)
    expect(isZoomPercent(33)).toBe(true)
    expect(isZoomPercent(101)).toBe(false)
    expect(isZoomPercent('100')).toBe(false)
  })
})

describe('zoomHost', () => {
  it('web pages only', () => {
    expect(zoomHost('https://Example.com/a')).toBe('example.com')
    expect(zoomHost('http://localhost:3000')).toBe('localhost')
    expect(zoomHost('signet://settings')).toBeNull()
    expect(zoomHost('file:///a.html')).toBeNull()
    expect(zoomHost('https://example.com/doc.pdf')).toBeNull()
    expect(zoomHost('not a url')).toBeNull()
  })
})

describe('defaultZoomFactor', () => {
  const base = { defaultPercent: 150, url: 'https://a.com/', userHosts: new Set<string>(), currentFactor: 1 }
  it('applies the default to a page at another zoom', () => {
    expect(defaultZoomFactor(base)).toBe(1.5)
  })
  it('does nothing when already at the default', () => {
    expect(defaultZoomFactor({ ...base, currentFactor: 1.5 })).toBeNull()
  })
  it('leaves a site the user zoomed alone', () => {
    expect(defaultZoomFactor({ ...base, userHosts: new Set(['a.com']) })).toBeNull()
    expect(defaultZoomFactor({ ...base, url: 'https://b.com/', userHosts: new Set(['a.com']) })).toBe(1.5)
  })
  it('does nothing without a chosen default', () => {
    expect(defaultZoomFactor({ ...base, defaultPercent: undefined })).toBeNull()
    expect(defaultZoomFactor({ ...base, defaultPercent: 123 })).toBeNull()
  })
  it('skips internal pages and PDFs', () => {
    expect(defaultZoomFactor({ ...base, url: 'signet://urls' })).toBeNull()
    expect(defaultZoomFactor({ ...base, url: 'https://a.com/x.pdf' })).toBeNull()
  })
  it('can return to 100 %', () => {
    expect(defaultZoomFactor({ ...base, defaultPercent: 100, currentFactor: 1.5 })).toBe(1)
  })
})

describe('site zoom memory', () => {
  it('cleans what is read from disk', () => {
    expect(cleanSiteZoom(null)).toEqual({})
    expect(cleanSiteZoom([1, 2])).toEqual({})
    expect(cleanSiteZoom({ 'a.com': 2, 'b.com': 'x', 'c.com': NaN, 'd.com': 50, '': 1 })).toEqual({ 'a.com': 2 })
  })
  it('keeps the most recent hosts within the bound', () => {
    const raw = Object.fromEntries(Array.from({ length: SITE_ZOOM_MAX + 5 }, (_, i) => [`h${i}.com`, 1]))
    const out = cleanSiteZoom(raw)
    expect(Object.keys(out)).toHaveLength(SITE_ZOOM_MAX)
    expect(out['h0.com']).toBeUndefined()
    expect(out[`h${SITE_ZOOM_MAX + 4}.com`]).toBe(1)
  })
  it('remember moves a host to the most recent place and evicts the oldest', () => {
    const map: Record<string, number> = { 'a.com': 1, 'b.com': 1 }
    rememberSiteZoom(map, 'a.com', 3)
    expect(Object.keys(map)).toEqual(['b.com', 'a.com'])
    for (let i = 0; i < SITE_ZOOM_MAX; i++) rememberSiteZoom(map, `x${i}.com`, 1)
    expect(Object.keys(map)).toHaveLength(SITE_ZOOM_MAX)
    expect(map['a.com']).toBeUndefined()
  })
})

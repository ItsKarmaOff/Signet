import { describe, expect, it } from 'vitest'
import type { HistoryEntry } from '../types'
import { addVisit, clearHistory, frecency, groupByDay, parseHistory, removeFromHistory, searchHistory, setHistoryTitle, topSites } from './history'

const DAY = 86_400_000
const NOW = new Date(2026, 5, 15, 12).getTime()

describe('history', () => {
  it('addVisit: adds, increments, ignores the fragment and non-web pages', () => {
    let h: HistoryEntry[] = []
    h = addVisit(h, { url: 'https://a.com/x', title: 'A' }, 1)
    h = addVisit(h, { url: 'https://b.com' }, 2)
    h = addVisit(h, { url: 'https://a.com/x#top' }, 3)
    expect(h.map((e) => e.url)).toEqual(['https://a.com/x#top', 'https://b.com'])
    expect(h[0]).toMatchObject({ visits: 2, title: 'A', lastVisit: 3 })
    expect(addVisit(h, { url: 'about:blank' })).toBe(h)
    expect(addVisit(h, { url: '' })).toBe(h)
  })

  it('addVisit : plafonne', () => {
    let h: HistoryEntry[] = []
    for (let i = 0; i < 10; i++) h = addVisit(h, { url: `https://s${i}.com` }, i, 5)
    expect(h).toHaveLength(5)
    expect(h[0].url).toBe('https://s9.com')
  })

  it('setHistoryTitle', () => {
    const h = addVisit([], { url: 'https://a.com' }, 1)
    expect(setHistoryTitle(h, 'https://a.com#x', ' Titre ')[0].title).toBe('Titre')
    expect(setHistoryTitle(h, 'https://zzz.com', 'T')).toBe(h)
  })

  it('frecency: recency and visits both count', () => {
    const recent: HistoryEntry = { url: 'https://r.com', title: '', visits: 1, lastVisit: NOW - DAY }
    const old: HistoryEntry = { url: 'https://o.com', title: '', visits: 1, lastVisit: NOW - 200 * DAY }
    const often: HistoryEntry = { ...old, visits: 15 }
    expect(frecency(recent, NOW)).toBeGreaterThan(frecency(old, NOW))
    expect(frecency(often, NOW)).toBeGreaterThan(frecency(old, NOW))
  })

  it('searchHistory: words, accents, prefix bonus, ranking', () => {
    const h: HistoryEntry[] = [
      { url: 'https://example.com/blog', title: 'Un blog sur le café', visits: 1, lastVisit: NOW },
      { url: 'https://cafe.example.org', title: 'Café du coin', visits: 5, lastVisit: NOW },
      { url: 'https://other.com', title: 'Autre', visits: 9, lastVisit: NOW }
    ]
    expect(searchHistory(h, 'cafe', NOW).map((e) => e.url)).toEqual(['https://cafe.example.org', 'https://example.com/blog'])
    expect(searchHistory(h, 'café blog', NOW)).toHaveLength(1)
    expect(searchHistory(h, '', NOW, 2)).toHaveLength(2)
    expect(searchHistory(h, 'zzz', NOW)).toEqual([])
  })

  it('removeFromHistory / clearHistory', () => {
    const h: HistoryEntry[] = [
      { url: 'https://a.com', title: '', visits: 1, lastVisit: 100 },
      { url: 'https://b.com', title: '', visits: 1, lastVisit: 10 }
    ]
    expect(removeFromHistory(h, 'https://a.com')).toHaveLength(1)
    expect(clearHistory(h)).toEqual([])
    expect(clearHistory(h, 50).map((e) => e.url)).toEqual(['https://b.com'])
  })

  it('topSites: one site per host', () => {
    const h = addVisit(addVisit(addVisit([], { url: 'https://www.a.com/1' }, NOW), { url: 'https://a.com/2' }, NOW), { url: 'https://b.com' }, NOW)
    expect(topSites(h, NOW)).toHaveLength(2)
  })

  it('groupByDay: today, yesterday, then dates', () => {
    const e = (url: string, t: number): HistoryEntry => ({ url, title: '', visits: 1, lastVisit: t })
    const days = groupByDay([e('https://a.com', NOW), e('https://b.com', NOW - DAY), e('https://c.com', NOW - 5 * DAY)], NOW)
    expect(days.map((d) => d.key)).toEqual(['today', 'yesterday', '2026-06-10'])
  })

  it('parseHistory: tolerant', () => {
    expect(parseHistory('nope')).toEqual([])
    const h = parseHistory([
      { url: 'https://a.com', visits: 0, lastVisit: 5 },
      null,
      { url: 'ftp://x' },
      { url: 'https://b.com', title: 3, lastVisit: 9, favicon: 'f' }
    ])
    expect(h.map((e) => e.url)).toEqual(['https://b.com', 'https://a.com'])
    expect(h[0]).toMatchObject({ title: '', favicon: 'f', visits: 1 })
  })
})

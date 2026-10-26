import { describe, expect, it } from 'vitest'
import { CLEAR_RANGES, clearHistory, clearRangeStart, countToClear, dayKeyDate, dayLabel, groupByDay, recentSites, restoreHistoryEntry } from './history'
import type { HistoryEntry } from '../types'

const at = (y: number, m: number, d: number, h = 12) => new Date(y, m - 1, d, h).getTime()
const NOW = at(2026, 10, 25, 15)
const entry = (url: string, lastVisit: number): HistoryEntry => ({ url, title: url, visits: 1, lastVisit })

describe('clear ranges', () => {
  const entries = [
    entry('https://a.test/1', NOW - 20 * 60_000),
    entry('https://a.test/2', at(2026, 10, 25, 9)),
    entry('https://b.test', at(2026, 10, 24, 22)),
    entry('https://c.test', at(2026, 10, 20))
  ]

  it('computes the start of each range', () => {
    expect(clearRangeStart('all', NOW)).toBeUndefined()
    expect(clearRangeStart('hour', NOW)).toBe(NOW - 3_600_000)
    expect(clearRangeStart('today', NOW)).toBe(at(2026, 10, 25, 0))
    expect(clearRangeStart('twoDays', NOW)).toBe(at(2026, 10, 24, 0))
    expect(CLEAR_RANGES).toHaveLength(4)
  })

  it('counts and clears what each range covers', () => {
    expect(countToClear(entries, 'hour', NOW)).toBe(1)
    expect(countToClear(entries, 'today', NOW)).toBe(2)
    expect(countToClear(entries, 'twoDays', NOW)).toBe(3)
    expect(countToClear(entries, 'all', NOW)).toBe(4)
    expect(clearHistory(entries, clearRangeStart('twoDays', NOW))).toEqual([entries[3]])
  })
})

describe('history labels and restore', () => {
  it('labels today, yesterday and older days in the given locale', () => {
    const days = groupByDay(
      [
        entry('https://a.test', NOW),
        entry('https://b.test', at(2026, 10, 24)),
        entry('https://c.test', at(2026, 10, 20)),
        entry('https://d.test', at(2025, 3, 2))
      ],
      NOW
    )
    const labels = { today: 'Today', yesterday: 'Yesterday' }
    expect(days.map((d) => dayLabel(d, labels, 'en', NOW))).toEqual(['Today', 'Yesterday', 'Tuesday, October 20', 'Sunday, March 2, 2025'])
    expect(dayLabel(days[2], labels, 'fr', NOW)).toBe('mardi 20 octobre')
  })

  it('parses day keys', () => {
    expect(dayKeyDate('2026-10-20')?.getDate()).toBe(20)
    expect(dayKeyDate('today')).toBeNull()
  })

  it('restores an entry in order and replaces a duplicate', () => {
    const list = [entry('https://a.test', 300), entry('https://c.test', 100)]
    const back = restoreHistoryEntry(list, entry('https://b.test', 200))
    expect(back.map((e) => e.url)).toEqual(['https://a.test', 'https://b.test', 'https://c.test'])
    expect(restoreHistoryEntry(back, entry('https://b.test', 400)).map((e) => e.url)).toEqual(['https://b.test', 'https://a.test', 'https://c.test'])
  })

  it('lists recent sites once per host', () => {
    const list = [entry('https://www.a.test/1', 300), entry('https://a.test/2', 200), entry('https://b.test', 100)]
    expect(recentSites(list, 5).map((e) => e.url)).toEqual(['https://www.a.test/1', 'https://b.test'])
    expect(recentSites(list, 1)).toHaveLength(1)
  })
})

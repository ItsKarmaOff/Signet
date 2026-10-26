// Browsing history: add, search, "frecency" ranking (frequency x recency), grouping by day.
// Private tabs never end up here (the caller must not call `addVisit` for them).
import { fold } from '../urls'
import type { HistoryEntry } from '../types'

export const HISTORY_CAP = 2000
const DAY = 86_400_000

/** Only web pages enter history (not `about:blank`, `data:`, etc.). */
export function isHistoryUrl(url: string): boolean {
  return /^https?:\/\/[^\s/]+/i.test(url)
}

/** Deduplication key: the address without the fragment (`#section`). */
export const historyKey = (url: string): string => url.replace(/#.*$/, '')

/** Recency weight, in steps (like Firefox's frecency). */
export function recencyWeight(ageMs: number): number {
  const days = ageMs / DAY
  if (days < 4) return 100
  if (days < 14) return 70
  if (days < 31) return 50
  if (days < 90) return 30
  return 10
}

/** Score of an entry: number of visits (capped at 20 so a heavily visited site does not crush everything) x recency weight. */
export function frecency(entry: HistoryEntry, now: number): number {
  return Math.min(entry.visits, 20) * recencyWeight(Math.max(0, now - entry.lastVisit))
}

/** Records a visit. Entries stay sorted from most recent to oldest; beyond `cap`, the oldest drop off. */
export function addVisit(
  entries: readonly HistoryEntry[],
  visit: { url: string; title?: string; favicon?: string },
  now = Date.now(),
  cap = HISTORY_CAP
): HistoryEntry[] {
  if (!isHistoryUrl(visit.url)) return entries as HistoryEntry[]
  const key = historyKey(visit.url)
  const previous = entries.find((e) => historyKey(e.url) === key)
  const next: HistoryEntry = {
    url: visit.url,
    title: visit.title?.trim() || previous?.title || '',
    favicon: visit.favicon ?? previous?.favicon,
    visits: (previous?.visits ?? 0) + 1,
    lastVisit: now
  }
  if (!next.favicon) delete next.favicon
  return [next, ...entries.filter((e) => e !== previous)].slice(0, cap)
}

/** Updates the title of an already saved entry (the title often arrives after the address). Without an entry, nothing changes. */
export function setHistoryTitle(entries: readonly HistoryEntry[], url: string, title: string): HistoryEntry[] {
  const key = historyKey(url)
  const text = title.trim()
  if (!text || !entries.some((e) => historyKey(e.url) === key && e.title !== text)) return entries as HistoryEntry[]
  return entries.map((e) => (historyKey(e.url) === key ? { ...e, title: text } : e))
}

export const removeFromHistory = (entries: readonly HistoryEntry[], url: string): HistoryEntry[] => entries.filter((e) => e.url !== url)

/** Clears everything, or only visits made since `since` (ms). */
export function clearHistory(entries: readonly HistoryEntry[], since?: number): HistoryEntry[] {
  return since === undefined ? [] : entries.filter((e) => e.lastVisit < since)
}

/** Entries containing all words of `query` (title or address, accent- and case-insensitive), best ranked first. Without a query: the most recent. */
export function searchHistory(entries: readonly HistoryEntry[], query: string, now = Date.now(), limit = 50): HistoryEntry[] {
  const words = fold(query).split(/\s+/).filter(Boolean)
  if (words.length === 0) return entries.slice(0, limit)
  const scored: { entry: HistoryEntry; score: number }[] = []
  for (const entry of entries) {
    const title = fold(entry.title)
    const url = fold(entry.url.replace(/^https?:\/\/(www\.)?/i, ''))
    if (!words.every((w) => title.includes(w) || url.includes(w))) continue
    // Bonus when the input starts the address ("git" -> github.com) or the title.
    const prefix = url.startsWith(words[0]) ? 2 : title.startsWith(words[0]) ? 1.5 : 1
    scored.push({ entry, score: frecency(entry, now) * prefix })
  }
  return scored
    .sort((a, b) => b.score - a.score || b.entry.lastVisit - a.entry.lastVisit)
    .slice(0, limit)
    .map((s) => s.entry)
}

/** The most visited sites (one entry per site), for the home screen. */
export function topSites(entries: readonly HistoryEntry[], now = Date.now(), limit = 8): HistoryEntry[] {
  const seen = new Set<string>()
  const out: HistoryEntry[] = []
  for (const e of [...entries].sort((a, b) => frecency(b, now) - frecency(a, now))) {
    let host: string
    try {
      host = new URL(e.url).hostname.replace(/^www\./, '')
    } catch {
      continue
    }
    if (seen.has(host)) continue
    seen.add(host)
    out.push(e)
    if (out.length >= limit) break
  }
  return out
}

export interface HistoryDay {
  /** `today`, `yesterday` or the local date `YYYY-MM-DD`: the screen translates or formats it. */
  key: string
  /** Start of the day (ms, local time). */
  start: number
  entries: HistoryEntry[]
}

/** Groups entries (already sorted by recency) by local day. */
export function groupByDay(entries: readonly HistoryEntry[], now = Date.now()): HistoryDay[] {
  const startOf = (t: number) => {
    const d = new Date(t)
    return new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime()
  }
  const today = startOf(now)
  const yesterday = startOf(today - 12 * 3_600_000)
  const days: HistoryDay[] = []
  for (const e of entries) {
    const start = startOf(e.lastVisit)
    let day = days.find((d) => d.start === start)
    if (!day) {
      const d = new Date(start)
      const iso = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`
      day = { key: start === today ? 'today' : start === yesterday ? 'yesterday' : iso, start, entries: [] }
      days.push(day)
    }
    day.entries.push(e)
  }
  return days.sort((a, b) => b.start - a.start)
}

/** Reloads a saved history: malformed entries ignored, duplicates merged, sorted most recent first, cap applied. */
export function parseHistory(raw: unknown, cap = HISTORY_CAP): HistoryEntry[] {
  if (!Array.isArray(raw)) return []
  const out: HistoryEntry[] = []
  for (const item of raw as unknown[]) {
    const e = item as Partial<HistoryEntry> | null
    if (!e || typeof e.url !== 'string' || !isHistoryUrl(e.url)) continue
    const entry: HistoryEntry = {
      url: e.url,
      title: typeof e.title === 'string' ? e.title : '',
      visits: typeof e.visits === 'number' && e.visits >= 1 ? Math.floor(e.visits) : 1,
      lastVisit: typeof e.lastVisit === 'number' && Number.isFinite(e.lastVisit) ? e.lastVisit : 0
    }
    if (typeof e.favicon === 'string' && e.favicon) entry.favicon = e.favicon
    out.push(entry)
  }
  return out.sort((a, b) => b.lastVisit - a.lastVisit).slice(0, cap)
}

/** Ranges offered by "Clear history". */
export type ClearRange = 'hour' | 'today' | 'twoDays' | 'all'
export const CLEAR_RANGES: readonly ClearRange[] = ['hour', 'today', 'twoDays', 'all']

/** Start (ms) of the range to clear: `clearHistory(entries, clearRangeStart(range))`. `undefined` = everything. "Today and yesterday" starts at midnight yesterday (local time). */
export function clearRangeStart(range: ClearRange, now = Date.now()): number | undefined {
  if (range === 'all') return undefined
  if (range === 'hour') return now - 3_600_000
  const d = new Date(now)
  return new Date(d.getFullYear(), d.getMonth(), d.getDate() - (range === 'twoDays' ? 1 : 0)).getTime()
}

/** Number of entries cleared by a range. */
export const countToClear = (entries: readonly HistoryEntry[], range: ClearRange, now = Date.now()): number =>
  entries.length - clearHistory(entries, clearRangeStart(range, now)).length

/** Puts an entry back in place (undo a deletion); an entry with the same address is replaced. */
export function restoreHistoryEntry(entries: readonly HistoryEntry[], entry: HistoryEntry, cap = HISTORY_CAP): HistoryEntry[] {
  return [...entries.filter((e) => e.url !== entry.url), entry].sort((a, b) => b.lastVisit - a.lastVisit).slice(0, cap)
}

/** Local date of a day key `YYYY-MM-DD` (`null` for `today` / `yesterday` or an invalid key). */
export function dayKeyDate(key: string): Date | null {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(key)
  return m ? new Date(Number(m[1]), Number(m[2]) - 1, Number(m[3])) : null
}

/** Title of a day group: "Today", "Yesterday" (labels provided, hence translated) or the full date in `locale` (year only if it is not the current year). */
export function dayLabel(day: HistoryDay, labels: { today: string; yesterday: string }, locale: string, now = Date.now()): string {
  if (day.key === 'today') return labels.today
  if (day.key === 'yesterday') return labels.yesterday
  const date = dayKeyDate(day.key) ?? new Date(day.start)
  const sameYear = date.getFullYear() === new Date(now).getFullYear()
  return new Intl.DateTimeFormat(locale, { weekday: 'long', day: 'numeric', month: 'long', ...(sameYear ? {} : { year: 'numeric' }) }).format(date)
}

/** The most recently visited sites, one per site (most recent first): the "Recently visited" strip. */
export function recentSites(entries: readonly HistoryEntry[], limit = 8): HistoryEntry[] {
  const seen = new Set<string>()
  const out: HistoryEntry[] = []
  for (const e of [...entries].sort((a, b) => b.lastVisit - a.lastVisit)) {
    let host: string
    try {
      host = new URL(e.url).hostname.replace(/^www\./, '')
    } catch {
      continue
    }
    if (seen.has(host)) continue
    seen.add(host)
    out.push(e)
    if (out.length >= limit) break
  }
  return out
}

/** Remembers the icon of a page already in history (the icon arrives after loading). Without an entry or without change, nothing moves. */
export function setHistoryFavicon(entries: readonly HistoryEntry[], url: string, favicon: string): HistoryEntry[] {
  const key = historyKey(url)
  if (!favicon || !entries.some((e) => historyKey(e.url) === key && e.favicon !== favicon)) return entries as HistoryEntry[]
  return entries.map((e) => (historyKey(e.url) === key ? { ...e, favicon } : e))
}

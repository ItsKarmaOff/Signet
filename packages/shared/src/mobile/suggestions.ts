// Command bar suggestions: open tabs, bookmarks, history, then search. Pure and ranked.
import type { Bookmark, HistoryEntry, TabInfo } from '../types'
import { toUrl } from '../urls'
import { searchBookmarks } from './bookmarks'
import { searchHistory } from './history'
import { searchUrlFor, type SearchEngineId } from './search'

export type Suggestion =
  | { kind: 'url'; url: string }
  /** `remote`: suggestion coming from the search engine (network), not from the input. */
  | { kind: 'search'; query: string; url: string; remote?: boolean }
  | { kind: 'tab'; tabId: string; title: string; url: string; favicon?: string }
  | { kind: 'bookmark'; id: string; title: string; url: string; favicon?: string }
  | { kind: 'history'; title: string; url: string; favicon?: string }

export interface SuggestSources {
  /** Tabs of the current context (private ones only in private browsing). */
  tabs: readonly TabInfo[]
  bookmarks: readonly Bookmark[]
  /** Empty in private browsing. */
  history: readonly HistoryEntry[]
  engine: SearchEngineId
  now?: number
}

/**
 * Suggestions for an input, without address duplicates: the typed address or the search first, then open tabs,
 * bookmarks and history. Empty input: recent tabs only.
 */
export function suggest(input: string, src: SuggestSources, limit = 8): Suggestion[] {
  const text = input.trim()
  const out: Suggestion[] = []
  const seen = new Set<string>()
  const push = (s: Suggestion) => {
    const key = s.kind === 'search' ? `q:${s.query}` : s.url
    if (seen.has(key) || out.length >= limit) return
    seen.add(key)
    out.push(s)
  }
  const words = text.toLowerCase().split(/\s+/).filter(Boolean)
  if (text) {
    const url = toUrl(text)
    push(url ? { kind: 'url', url } : { kind: 'search', query: text, url: searchUrlFor(src.engine, text) })
  }
  const tabs = [...src.tabs].filter((t) => t.url).sort((a, b) => b.lastActive - a.lastActive)
  for (const t of tabs) {
    if (words.every((w) => (t.title + ' ' + t.url).toLowerCase().includes(w)))
      push({ kind: 'tab', tabId: t.id, title: t.title || t.url, url: t.url, favicon: t.favicon })
  }
  if (!text) return out
  for (const b of searchBookmarks(src.bookmarks, text)) push({ kind: 'bookmark', id: b.id, title: b.title, url: b.url, favicon: b.favicon })
  for (const h of searchHistory(src.history, text, src.now, limit)) push({ kind: 'history', title: h.title || h.url, url: h.url, favicon: h.favicon })
  // A search is still offered as a last resort when the typed address is a URL.
  if (toUrl(text)) push({ kind: 'search', query: text, url: searchUrlFor(src.engine, text) })
  return out
}

/** Adds the engine's suggestions below the local results: no search duplicates, `max` at most. */
export function withRemoteSuggestions(local: readonly Suggestion[], remote: readonly string[], engine: SearchEngineId, max = 5): Suggestion[] {
  const out = [...local]
  const seen = new Set(local.filter((s) => s.kind === 'search').map((s) => (s as { query: string }).query.toLowerCase()))
  let added = 0
  for (const query of remote) {
    if (added >= max) break
    const key = query.toLowerCase()
    if (seen.has(key)) continue
    seen.add(key)
    out.push({ kind: 'search', query, url: searchUrlFor(engine, query), remote: true })
    added++
  }
  return out
}

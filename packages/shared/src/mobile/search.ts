// Search engines of the mobile app and conversion of an input into an address.
import { toUrl } from '../urls'

export type SearchEngineId = 'google' | 'duckduckgo' | 'bing' | 'ecosia' | 'brave' | 'qwant'

export interface SearchEngine {
  id: SearchEngineId
  /** Proper name: never translated. */
  name: string
  /** Address template; `%s` is replaced by the encoded query. */
  template: string
}

export const SEARCH_ENGINES: readonly SearchEngine[] = [
  { id: 'google', name: 'Google', template: 'https://www.google.com/search?q=%s' },
  { id: 'duckduckgo', name: 'DuckDuckGo', template: 'https://duckduckgo.com/?q=%s' },
  { id: 'bing', name: 'Bing', template: 'https://www.bing.com/search?q=%s' },
  { id: 'ecosia', name: 'Ecosia', template: 'https://www.ecosia.org/search?q=%s' },
  { id: 'brave', name: 'Brave', template: 'https://search.brave.com/search?q=%s' },
  { id: 'qwant', name: 'Qwant', template: 'https://www.qwant.com/?q=%s' }
]

export const DEFAULT_SEARCH_ENGINE: SearchEngineId = 'google'

export const isSearchEngineId = (v: unknown): v is SearchEngineId => SEARCH_ENGINES.some((e) => e.id === v)

export function searchEngine(id: SearchEngineId): SearchEngine {
  return SEARCH_ENGINES.find((e) => e.id === id) ?? SEARCH_ENGINES[0]
}

/** Address of the search `query` at engine `id`. */
export function searchUrlFor(id: SearchEngineId, query: string): string {
  return searchEngine(id).template.replace('%s', encodeURIComponent(query))
}

/** Input -> address to load: a URL if the input is one, otherwise a search; `null` if the input is empty. */
export function resolveInput(input: string, engine: SearchEngineId = DEFAULT_SEARCH_ENGINE): string | null {
  const text = input.trim()
  if (!text) return null
  return toUrl(text) ?? searchUrlFor(engine, text)
}

// ───────────── Search suggestions from the engine (network) ─────────────

/** Address of each engine's suggestions API (`%s` = encoded query). All answer in JSON without an API key. */
const SUGGEST_TEMPLATES: Record<SearchEngineId, string> = {
  google: 'https://suggestqueries.google.com/complete/search?client=firefox&q=%s',
  duckduckgo: 'https://duckduckgo.com/ac/?q=%s&type=list',
  bing: 'https://api.bing.com/osjson.aspx?query=%s',
  brave: 'https://search.brave.com/api/suggest?q=%s',
  ecosia: 'https://ac.ecosia.org/autocomplete?q=%s&type=list',
  qwant: 'https://api.qwant.com/v3/suggest?q=%s&locale=en_US&version=2'
}

export const SUGGESTION_LIMIT = 5

/** Address of the suggestions request for `query` at engine `id`. */
export function suggestionsUrlFor(id: SearchEngineId, query: string): string {
  return SUGGEST_TEMPLATES[id].replace('%s', encodeURIComponent(query))
}

const text = (v: unknown): string | null => {
  if (typeof v === 'string') return v.trim() || null
  if (v && typeof v === 'object') {
    const o = v as Record<string, unknown>
    for (const k of ['phrase', 'value', 'q', 'text']) if (typeof o[k] === 'string' && o[k].trim()) return o[k].trim()
  }
  return null
}

const clean = (list: unknown[], query: string, limit: number): string[] => {
  const out: string[] = []
  const seen = new Set([query.trim().toLowerCase()])
  for (const item of list) {
    const s = text(item)
    if (!s || s.length > 200) continue
    const key = s.toLowerCase()
    if (seen.has(key)) continue
    seen.add(key)
    out.push(s)
    if (out.length >= limit) break
  }
  return out
}

/** OpenSearch format: `["query", ["s1", "s2", …]]` (Google, Bing, DuckDuckGo with `type=list`, Brave, Ecosia). */
function parseOpenSearch(json: unknown, query: string, limit: number): string[] {
  return Array.isArray(json) && Array.isArray(json[1]) ? clean(json[1], query, limit) : []
}

/** Qwant : `{ status: "success", data: { items: [{ value: "…" }] } }`. */
function parseQwant(json: unknown, query: string, limit: number): string[] {
  const items = (json as { data?: { items?: unknown } } | null)?.data?.items
  return Array.isArray(items) ? clean(items, query, limit) : []
}

/** DuckDuckGo without `type=list`: `[{ "phrase": "…" }]`. Accepted for tolerance. */
function parseDuckDuckGo(json: unknown, query: string, limit: number): string[] {
  if (Array.isArray(json) && json.length > 0 && typeof json[0] === 'object' && !Array.isArray(json[0])) return clean(json, query, limit)
  return parseOpenSearch(json, query, limit)
}

/** Suggestions (text only, no duplicates, without the query itself) from a JSON response of engine `id`. Unexpected response: empty list. */
export function parseSuggestions(id: SearchEngineId, json: unknown, query: string, limit = SUGGESTION_LIMIT): string[] {
  try {
    if (id === 'qwant') return parseQwant(json, query, limit)
    if (id === 'duckduckgo') return parseDuckDuckGo(json, query, limit)
    return parseOpenSearch(json, query, limit)
  } catch {
    return []
  }
}

// Search keywords and custom engines (desktop command bar). Pure: shared with the settings UI and the main process.
import { SEARCH_ENGINES, SUGGEST_TEMPLATES, type SearchEngineId } from './mobile/search'

export const KEYWORD_MAX = 32
export const ENGINE_NAME_MAX = 60
export const CUSTOM_ENGINES_MAX = 50
const TEMPLATE_MAX = 2048

/** Default keyword of each built-in engine. */
export const BUILTIN_KEYWORDS: Record<SearchEngineId, string> = {
  google: 'g',
  duckduckgo: 'ddg',
  bing: 'b',
  ecosia: 'eco',
  brave: 'br',
  qwant: 'q'
}

export type EngineSource = 'builtin' | 'preset' | 'custom'

/** Engine added by the user. `suggest`: OpenSearch JSON suggestions address (`%s` = query). */
export interface CustomEngine {
  id: string
  name: string
  keyword: string
  template: string
  suggest?: string
}

/** Change of a built-in or preset engine: another keyword, or (presets only) turned off. */
export interface KeywordOverride {
  keyword?: string
  disabled?: true
}
export type KeywordOverrides = Record<string, KeywordOverride>

export interface EngineConfig {
  searchEngines?: CustomEngine[]
  searchKeywords?: KeywordOverrides
}

/** An engine as the command bar sees it. */
export interface EngineSpec {
  id: string
  /** Proper name: never translated. */
  name: string
  source: EngineSource
  template: string
  suggest?: string
  keyword: string
  /** Keyword without override (built-ins and presets). */
  defaultKeyword?: string
  enabled: boolean
}

/** Site searches on by default. The Wikipedia host follows the interface language. */
export function wikipediaHost(locale: string | undefined): string {
  return /^fr(?:[-_]|$)/i.test(locale ?? '') ? 'fr.wikipedia.org' : 'en.wikipedia.org'
}

interface Preset {
  id: string
  name: string
  keyword: string
  template: (locale: string | undefined) => string
  suggest?: (locale: string | undefined) => string
}

const PRESETS: readonly Preset[] = [
  {
    id: 'youtube',
    name: 'YouTube',
    keyword: 'yt',
    template: () => 'https://www.youtube.com/results?search_query=%s',
    suggest: () => 'https://suggestqueries.google.com/complete/search?client=firefox&ds=yt&q=%s'
  },
  {
    id: 'wikipedia',
    name: 'Wikipedia',
    keyword: 'w',
    template: (l) => `https://${wikipediaHost(l)}/wiki/Special:Search?search=%s`,
    suggest: (l) => `https://${wikipediaHost(l)}/w/api.php?action=opensearch&format=json&namespace=0&limit=5&search=%s`
  },
  { id: 'github', name: 'GitHub', keyword: 'gh', template: () => 'https://github.com/search?q=%s' }
]

/** `%s` replaced (everywhere) by the encoded query. */
export function expandTemplate(template: string, query: string): string {
  return template.split('%s').join(encodeURIComponent(query))
}

/** http(s) address containing `%s`. */
export function isSearchTemplate(v: unknown): v is string {
  if (typeof v !== 'string' || v.length > TEMPLATE_MAX || !v.includes('%s')) return false
  try {
    const u = new URL(v.split('%s').join('x'))
    return u.protocol === 'http:' || u.protocol === 'https:'
  } catch {
    return false
  }
}

/** Keyword as stored (trimmed, lowercase), or null if invalid (empty, spaces, too long). */
export function normalizeKeyword(v: unknown): string | null {
  if (typeof v !== 'string') return null
  const k = v.trim().toLowerCase()
  return k && k.length <= KEYWORD_MAX && !/\s/.test(k) ? k : null
}

/** Every engine, in the order built-ins, presets, custom; keywords as overridden. Expects cleaned settings. */
export function resolveEngines(cfg: EngineConfig, locale?: string): EngineSpec[] {
  const over = cfg.searchKeywords ?? {}
  const out: EngineSpec[] = SEARCH_ENGINES.map((e) => ({
    id: e.id,
    name: e.name,
    source: 'builtin',
    template: e.template,
    suggest: SUGGEST_TEMPLATES[e.id],
    keyword: over[e.id]?.keyword ?? BUILTIN_KEYWORDS[e.id],
    defaultKeyword: BUILTIN_KEYWORDS[e.id],
    enabled: true
  }))
  for (const p of PRESETS)
    out.push({
      id: p.id,
      name: p.name,
      source: 'preset',
      template: p.template(locale),
      suggest: p.suggest?.(locale),
      keyword: over[p.id]?.keyword ?? p.keyword,
      defaultKeyword: p.keyword,
      enabled: over[p.id]?.disabled !== true
    })
  for (const c of cfg.searchEngines ?? []) out.push({ ...c, source: 'custom', enabled: true })
  return out
}

/** Keywords cleaned (see `cleanEngineConfig`): invalid or clashing entries dropped, the first one wins. */
export function cleanEngineConfig(raw: { searchEngines?: unknown; searchKeywords?: unknown }): Required<EngineConfig> {
  const base = resolveEngines({})
  const keywords = new Map(base.map((e) => [e.id, e.keyword]))
  const taken = (k: string, self?: string) => [...keywords].some(([id, v]) => id !== self && v === k)
  const searchKeywords: KeywordOverrides = {}
  const over =
    raw.searchKeywords && typeof raw.searchKeywords === 'object' && !Array.isArray(raw.searchKeywords) ? (raw.searchKeywords as Record<string, unknown>) : {}
  for (const e of base) {
    const o = Object.hasOwn(over, e.id) ? over[e.id] : undefined
    if (!o || typeof o !== 'object') continue
    const entry: KeywordOverride = {}
    const k = normalizeKeyword((o as { keyword?: unknown }).keyword)
    if (k && k !== e.defaultKeyword && !taken(k, e.id)) {
      entry.keyword = k
      keywords.set(e.id, k)
    }
    if (e.source === 'preset' && (o as { disabled?: unknown }).disabled === true) entry.disabled = true
    if (entry.keyword || entry.disabled) searchKeywords[e.id] = entry
  }
  const searchEngines: CustomEngine[] = []
  const ids = new Set<string>()
  const list = Array.isArray(raw.searchEngines) ? raw.searchEngines : []
  for (const c of list) {
    if (searchEngines.length >= CUSTOM_ENGINES_MAX || !c || typeof c !== 'object') continue
    const { id, name, keyword, template, suggest } = c as Record<string, unknown>
    const k = normalizeKeyword(keyword)
    const n = typeof name === 'string' ? name.trim() : ''
    if (typeof id !== 'string' || !/^custom-[a-z0-9]{4,32}$/.test(id) || ids.has(id)) continue
    if (!n || n.length > ENGINE_NAME_MAX || !k || taken(k) || !isSearchTemplate(template)) continue
    ids.add(id)
    keywords.set(id, k)
    searchEngines.push({ id, name: n, keyword: k, template, ...(isSearchTemplate(suggest) && { suggest }) })
  }
  return { searchEngines, searchKeywords }
}

/** Default engine to keep: it must exist and be enabled; otherwise undefined (= Google). */
export function cleanDefaultEngine(id: unknown, engines: readonly EngineSpec[]): string | undefined {
  return typeof id === 'string' && engines.some((e) => e.id === id && e.enabled) ? id : undefined
}

export type EngineDraftError = 'name' | 'keyword' | 'keywordTaken' | 'template' | 'suggest'

/** Checks the add or edit form of a custom engine. `selfId`: the engine being edited. */
export function validateEngineDraft(
  d: { name: string; keyword: string; template: string; suggest: string },
  engines: readonly EngineSpec[],
  selfId?: string
): EngineDraftError | null {
  const name = d.name.trim()
  if (!name || name.length > ENGINE_NAME_MAX) return 'name'
  const k = normalizeKeyword(d.keyword)
  if (!k) return 'keyword'
  if (engines.some((e) => e.id !== selfId && e.keyword === k)) return 'keywordTaken'
  if (!isSearchTemplate(d.template.trim())) return 'template'
  if (d.suggest.trim() && !isSearchTemplate(d.suggest.trim())) return 'suggest'
  return null
}

/** A keyword typed in the command bar, followed by a query. */
export interface KeywordMatch {
  engine: EngineSpec
  /** Text after the keyword, trimmed (empty while the user has only typed the keyword and a space). */
  query: string
}

/**
 * `keyword query`: the first token must equal an enabled engine's keyword (case-insensitive) AND be followed by
 * whitespace. `w.org` or a lone `w` are not keyword searches.
 */
export function parseKeyword(input: string, engines: readonly EngineSpec[]): KeywordMatch | null {
  const m = /^\s*(\S+)\s([\s\S]*)$/.exec(input)
  if (!m) return null
  const token = m[1].toLowerCase()
  const engine = engines.find((e) => e.enabled && e.keyword === token)
  return engine ? { engine, query: m[2].trim() } : null
}

/** The engine whose keyword is exactly what was typed (no space yet): the bar offers "Press Tab to search …". */
export function keywordHint(input: string, engines: readonly EngineSpec[]): EngineSpec | null {
  const token = input.trim().toLowerCase()
  if (!token || /\s/.test(token)) return null
  return engines.find((e) => e.enabled && e.keyword === token) ?? null
}

/** Overrides with `change` applied to `engine` (built-in or preset): a default keyword or an enabled preset leaves no entry. */
export function withOverride(
  overrides: KeywordOverrides,
  engine: Pick<EngineSpec, 'id' | 'defaultKeyword'>,
  change: { keyword?: string; disabled?: boolean }
): KeywordOverrides {
  const next: KeywordOverrides = { ...overrides }
  const cur = { ...next[engine.id] }
  if ('keyword' in change) {
    if (change.keyword && change.keyword !== engine.defaultKeyword) cur.keyword = change.keyword
    else delete cur.keyword
  }
  if ('disabled' in change) {
    if (change.disabled) cur.disabled = true
    else delete cur.disabled
  }
  if (cur.keyword || cur.disabled) next[engine.id] = cur
  else delete next[engine.id]
  return next
}

/** The engine with this id, else Google (deleted or unknown default). */
export function engineById(engines: readonly EngineSpec[], id: string | undefined): EngineSpec {
  return engines.find((e) => e.id === id && e.enabled) ?? engines.find((e) => e.id === 'google') ?? engines[0]
}

/** Address of the search for `query` at `engine` (plain template; desktop adds `hl` to Google on its own). */
export function searchUrlAt(engine: Pick<EngineSpec, 'template'>, query: string): string {
  return expandTemplate(engine.template, query)
}

/** Address of the suggestions request of `engine` for `query`, or null if it has none. */
export function suggestionsUrlAt(engine: Pick<EngineSpec, 'suggest'>, query: string): string | null {
  return engine.suggest ? expandTemplate(engine.suggest, query) : null
}

/** Address of the engine's icon (its site's `/favicon.ico`), or undefined. */
export function engineFavicon(e: Pick<EngineSpec, 'template'>): string | undefined {
  try {
    return `${new URL(e.template.split('%s').join('x')).origin}/favicon.ico`
  } catch {
    return undefined
  }
}

/** What the command bar needs to know about a keyword. */
export interface KeywordInfo {
  id: string
  keyword: string
  name: string
  favicon?: string
}

export const keywordInfos = (engines: readonly EngineSpec[]): KeywordInfo[] =>
  engines.filter((e) => e.enabled).map((e) => ({ id: e.id, keyword: e.keyword, name: e.name, favicon: engineFavicon(e) }))

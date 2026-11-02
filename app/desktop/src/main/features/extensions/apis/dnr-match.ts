import { DNR_ENUMS } from '@shared/extension-apis'
import type { Rule } from './dnr'

/**
 * Pure `declarativeNetRequest` matching: rule compilation, the domain index, Chrome's resolution between matching rules,
 * redirect transforms and header operations. No Electron here, so it is unit-tested directly.
 */

// ---------------------------------------------------------------- requests

const RESOURCE_TYPES = Object.values(DNR_ENUMS.ResourceType) as string[]
const TYPE_BIT = new Map(RESOURCE_TYPES.map((t, i) => [t, 1 << i]))
const ALL_TYPES = (1 << RESOURCE_TYPES.length) - 1
const MAIN_FRAME_BIT = TYPE_BIT.get(DNR_ENUMS.ResourceType.MAIN_FRAME)!
/** What a rule matches when it names no resource type: everything but navigations of the top frame. */
const DEFAULT_TYPES = ALL_TYPES & ~MAIN_FRAME_BIT

const ELECTRON_TYPES: Record<string, string> = {
  mainFrame: 'main_frame',
  subFrame: 'sub_frame',
  stylesheet: 'stylesheet',
  script: 'script',
  image: 'image',
  font: 'font',
  object: 'object',
  xhr: 'xmlhttprequest',
  ping: 'ping',
  cspReport: 'csp_report',
  media: 'media',
  webSocket: 'websocket'
}

/** DNR resource type of an Electron `resourceType`. */
export const dnrType = (electronType: string | undefined): string => (electronType && ELECTRON_TYPES[electronType]) || 'other'

export interface DnrRequest {
  url: string
  /** Lowercase host name of the URL. */
  host: string
  /** Origin of the page or frame that made the request, if known. */
  initiator?: string
  initiatorHost?: string
  /** DNR resource type (`main_frame`, `xmlhttprequest`...). */
  type: string
  /** Lowercase HTTP method. */
  method: string
  /** Tab (webContents id) or -1. */
  tabId: number
  /** Lowercased URL, filled on first use. */
  lower?: string
  thirdParty?: boolean
}

const NETWORK_SCHEMES = new Set(['http:', 'https:', 'ws:', 'wss:', 'ftp:'])

/** Builds a request, or null for anything DNR does not see (data:, blob:, chrome-extension:...). */
export function makeRequest(input: { url: string; type?: string; method?: string; initiator?: string; tabId?: number }): DnrRequest | null {
  let u: URL
  try {
    u = new URL(input.url)
  } catch {
    return null
  }
  if (!NETWORK_SCHEMES.has(u.protocol)) return null
  let initiatorHost: string | undefined
  let initiator = input.initiator
  if (initiator) {
    try {
      const i = new URL(initiator)
      initiatorHost = i.hostname.toLowerCase() || undefined
      initiator = i.origin === 'null' ? initiator : i.origin
    } catch {
      initiator = undefined
    }
  }
  return {
    url: u.href,
    host: u.hostname.toLowerCase(),
    initiator,
    initiatorHost,
    type: input.type ?? 'other',
    method: (input.method ?? 'get').toLowerCase(),
    tabId: input.tabId ?? -1
  }
}

// ---------------------------------------------------------------- domains

/** Second-level labels that act as a public suffix under a country code (`co.uk`, `com.au`). */
const SLD = new Set(['co', 'com', 'org', 'net', 'gov', 'edu', 'ac', 'or', 'ne', 'go', 'gob', 'mil'])
/** A few suffixes that the country-code heuristic does not cover. */
const SUFFIXES = new Set(['github.io', 'gitlab.io', 'herokuapp.com', 'blogspot.com', 'appspot.com', 'pages.dev', 'netlify.app', 'vercel.app', 'web.app'])

/** Approximate eTLD+1 (no public suffix list in the app): enough to tell first from third party. */
export function registrableDomain(host: string): string {
  if (/^[\d.]+$/.test(host) || host.includes(':')) return host
  const labels = host.split('.')
  if (labels.length <= 2) return host
  const last2 = labels.slice(-2).join('.')
  if (SUFFIXES.has(last2)) return labels.slice(-3).join('.')
  const tld = labels[labels.length - 1]
  const keep = tld.length === 2 && SLD.has(labels[labels.length - 2]) ? 3 : 2
  return labels.slice(-keep).join('.')
}

/** `host` is one of `domains` or a subdomain of one. */
const inDomains = (host: string, domains: string[]): boolean => domains.some((d) => host === d || host.endsWith(`.${d}`))

function isThirdParty(q: DnrRequest): boolean {
  if (q.thirdParty === undefined) q.thirdParty = !!q.initiatorHost && registrableDomain(q.host) !== registrableDomain(q.initiatorHost)
  return q.thirdParty
}

// ---------------------------------------------------------------- urlFilter

type UrlTest = (url: string, lower: string) => boolean

const escapeRe = (s: string) => s.replace(/[.+?^${}()|[\]\\/-]/g, '\\$&')

/** Compiles Chrome's urlFilter syntax: `||` domain anchor, `|` anchors, `^` separator, `*` wildcard. */
export function urlFilterTest(filter: string, caseSensitive: boolean): UrlTest {
  let body = filter
  let domainAnchor = false
  let startAnchor = false
  let endAnchor = false
  if (body.startsWith('||')) {
    domainAnchor = true
    body = body.slice(2)
  } else if (body.startsWith('|')) {
    startAnchor = true
    body = body.slice(1)
  }
  if (body.endsWith('|')) {
    endAnchor = true
    body = body.slice(0, -1)
  }
  const plain = !/[*^]/.test(body)
  const text = caseSensitive ? body : body.toLowerCase()
  const subject = (url: string, lower: string) => (caseSensitive ? url : lower)
  if (plain && !domainAnchor) {
    if (startAnchor && endAnchor) return (u, l) => subject(u, l) === text
    if (startAnchor) return (u, l) => subject(u, l).startsWith(text)
    if (endAnchor) return (u, l) => subject(u, l).endsWith(text)
    return (u, l) => subject(u, l).includes(text)
  }
  let source = ''
  for (const c of body) source += c === '*' ? '.*' : c === '^' ? '(?:[^a-zA-Z0-9_.%-]|$)' : escapeRe(c)
  const prefix = domainAnchor ? '^[a-z][a-z0-9+.-]*:\\/\\/(?:[^/?#]*\\.)?' : startAnchor ? '^' : ''
  const re = new RegExp(prefix + source + (endAnchor ? '$' : ''), caseSensitive ? '' : 'i')
  // Cheap rejection before the regex: the longest literal run must be present.
  const literal = body.split(/[*^]/).reduce((a, b) => (b.length > a.length ? b : a), '')
  const lit = caseSensitive ? literal : literal.toLowerCase()
  if (!lit) return (u) => re.test(u)
  return (u, l) => subject(u, l).includes(lit) && re.test(u)
}

/** Host that a `||host^`-style filter requires, to file the rule under it; null when the filter is not that specific. */
export function anchoredHost(filter: string): string | null {
  const m = /^\|\|([a-z0-9.-]+)(?=[\^/:?]|\|$)/i.exec(filter)
  const host = m?.[1].toLowerCase()
  return host && !host.startsWith('.') && !host.endsWith('.') ? host : null
}

// ---------------------------------------------------------------- compiled rules

const lowerList = (v: unknown): string[] | null => (Array.isArray(v) && v.length ? v.map((d) => String(d).toLowerCase()) : null)
const set = (v: unknown): Set<string> | null => (Array.isArray(v) && v.length ? new Set(v.map((m) => String(m).toLowerCase())) : null)
const numbers = (v: unknown): number[] | null => (Array.isArray(v) && v.length ? v.filter((n): n is number => typeof n === 'number') : null)

export interface CompiledRule {
  ext: string
  /** Ruleset id: a static one, `_dynamic` or `_session`. */
  set: string
  id: number
  priority: number
  type: string
  raw: Rule
  typeMask: number
  methods: Set<string> | null
  excludedMethods: Set<string> | null
  requestDomains: string[] | null
  excludedRequestDomains: string[] | null
  initiatorDomains: string[] | null
  excludedInitiatorDomains: string[] | null
  domainType: string | null
  tabIds: number[] | null
  excludedTabIds: number[] | null
  urlFilter: string | null
  regexFilter: string | null
  caseSensitive: boolean
  /** Index buckets (hosts the request must be on or below); empty = generic. */
  keys: string[]
  /** Lazy matchers. */
  urlTest?: UrlTest
  regex?: RegExp | null
  /** Lookup stamp, to report a rule filed under several buckets once. */
  seen?: number
}

/** Compiles one rule; null when its condition cannot be satisfied or is invalid (skipped like Chrome). */
export function compileRule(ext: string, rulesetId: string, rule: Rule): CompiledRule | null {
  const c = rule.condition as Record<string, unknown>
  const urlFilter = typeof c.urlFilter === 'string' && c.urlFilter ? c.urlFilter : null
  const regexFilter = typeof c.regexFilter === 'string' && c.regexFilter ? c.regexFilter : null
  if (urlFilter && regexFilter) return null
  let typeMask = DEFAULT_TYPES
  const allowed = Array.isArray(c.resourceTypes) && c.resourceTypes.length ? (c.resourceTypes as string[]) : null
  if (allowed) typeMask = allowed.reduce((m, t) => m | (TYPE_BIT.get(t) ?? 0), 0)
  if (Array.isArray(c.excludedResourceTypes)) for (const t of c.excludedResourceTypes as string[]) typeMask &= ~(TYPE_BIT.get(t) ?? 0)
  const requestDomains = lowerList(c.requestDomains)
  const caseSensitive = c.isUrlFilterCaseSensitive === true
  const sessionOnly = rulesetId === '_session'
  const host = requestDomains ? null : urlFilter ? anchoredHost(urlFilter) : null
  return {
    ext,
    set: rulesetId,
    id: rule.id,
    priority: typeof rule.priority === 'number' ? rule.priority : 1,
    type: rule.action.type,
    raw: rule,
    typeMask,
    methods: set(c.requestMethods),
    excludedMethods: set(c.excludedRequestMethods),
    requestDomains,
    excludedRequestDomains: lowerList(c.excludedRequestDomains),
    initiatorDomains: lowerList(c.initiatorDomains ?? c.domains),
    excludedInitiatorDomains: lowerList(c.excludedInitiatorDomains ?? c.excludedDomains),
    domainType: typeof c.domainType === 'string' ? c.domainType : null,
    tabIds: sessionOnly ? numbers(c.tabIds) : null,
    excludedTabIds: sessionOnly ? numbers(c.excludedTabIds) : null,
    urlFilter,
    regexFilter,
    caseSensitive,
    keys: requestDomains ?? (host ? [host] : [])
  }
}

function regexOf(r: CompiledRule): RegExp | null {
  if (r.regex === undefined) {
    try {
      r.regex = new RegExp(r.regexFilter!, r.caseSensitive ? '' : 'i')
    } catch {
      r.regex = null // invalid: the rule never matches
    }
  }
  return r.regex
}

/** Whether the rule's condition holds for the request. */
export function ruleMatches(r: CompiledRule, q: DnrRequest): boolean {
  if (!(r.typeMask & (TYPE_BIT.get(q.type) ?? TYPE_BIT.get('other')!))) return false
  if (r.methods && !r.methods.has(q.method)) return false
  if (r.excludedMethods?.has(q.method)) return false
  if (r.requestDomains && !inDomains(q.host, r.requestDomains)) return false
  if (r.excludedRequestDomains && inDomains(q.host, r.excludedRequestDomains)) return false
  if (r.initiatorDomains && (!q.initiatorHost || !inDomains(q.initiatorHost, r.initiatorDomains))) return false
  if (r.excludedInitiatorDomains && q.initiatorHost && inDomains(q.initiatorHost, r.excludedInitiatorDomains)) return false
  if (r.domainType && isThirdParty(q) !== (r.domainType === DNR_ENUMS.DomainType.THIRD_PARTY)) return false
  if (r.tabIds && !r.tabIds.includes(q.tabId)) return false
  if (r.excludedTabIds?.includes(q.tabId)) return false
  if (r.urlFilter) {
    const test = (r.urlTest ??= urlFilterTest(r.urlFilter, r.caseSensitive))
    return test(q.url, (q.lower ??= q.url.toLowerCase()))
  }
  if (r.regexFilter) return regexOf(r)?.test(q.url) ?? false
  return true
}

// ---------------------------------------------------------------- index

let stamp = 0

/**
 * Rules filed by the host they require (`requestDomains`, or the host of a leading `||host^`), plus the rest in a generic
 * list. A request looks up its host and every parent domain, so the cost does not grow with the number of rules that are
 * about other sites. Built once per rule change, never per request.
 */
export class RuleIndex {
  private byHost = new Map<string, CompiledRule[]>()
  private generic: CompiledRule[] = []
  readonly size: number

  constructor(rules: CompiledRule[]) {
    this.size = rules.length
    for (const r of rules) {
      if (!r.keys.length) this.generic.push(r)
      else {
        for (const key of r.keys) {
          const list = this.byHost.get(key)
          if (list) list.push(r)
          else this.byHost.set(key, [r])
        }
      }
    }
  }

  /** Number of rules that have to be looked at for a host (index tests). */
  candidates(host: string): number {
    let n = this.generic.length
    let h = host
    for (;;) {
      n += this.byHost.get(h)?.length ?? 0
      const dot = h.indexOf('.')
      if (dot < 0) return n
      h = h.slice(dot + 1)
    }
  }

  match(q: DnrRequest): CompiledRule[] {
    const out: CompiledRule[] = []
    const id = ++stamp
    const visit = (list: CompiledRule[] | undefined) => {
      if (!list) return
      for (const r of list) {
        if (r.seen === id) continue
        r.seen = id
        if (ruleMatches(r, q)) out.push(r)
      }
    }
    let h = q.host
    for (;;) {
      visit(this.byHost.get(h))
      const dot = h.indexOf('.')
      if (dot < 0) break
      h = h.slice(dot + 1)
    }
    visit(this.generic)
    return out
  }
}

// ---------------------------------------------------------------- resolution

export interface HeaderOp {
  header: string
  operation: string
  value?: string
}

export interface Resolution {
  action: 'none' | 'block' | 'allow' | 'redirect' | 'upgrade'
  /** The rule that decided (absent for an allowance inherited from the frame). */
  rule?: CompiledRule
  /** `allowAllRequests` rules that matched a frame request: remember them for the frame. */
  frameAllows: { ext: string; priority: number }[]
  requestHeaders: HeaderOp[]
  responseHeaders: HeaderOp[]
  /** Every rule that took effect, for the matched-rules log. */
  used: CompiledRule[]
}

export interface ResolveContext {
  /** Lower comes first: Chrome gives precedence to the extension installed most recently. */
  rank(ext: string): number
  /** Priority of the `allowAllRequests` already granted to the request's frame, per extension. */
  frameAllows?: Map<string, number>
  /** Whether the extension may apply this rule here (permissions). */
  canAct(rule: CompiledRule): boolean
}

const ACTION_RANK: Record<string, number> = { allow: 0, allowAllRequests: 1, block: 2, upgradeScheme: 3, redirect: 4 }
const SOURCE_RANK = (r: CompiledRule) => ACTION_RANK[r.type] ?? 9

interface Candidate {
  priority: number
  rank: number
  rule?: CompiledRule
  /** 'allow' for allow and allowAllRequests. */
  kind: 'block' | 'allow' | 'redirect' | 'upgrade'
}

const better = (a: Candidate, b: Candidate | undefined): boolean => !b || a.priority > b.priority || (a.priority === b.priority && a.rank < b.rank)

/** Chrome's rule resolution; see the module comment of dnr-engine.ts for the cross-extension approximation. */
export function resolve(matches: CompiledRule[], q: DnrRequest, ctx: ResolveContext): Resolution {
  const result: Resolution = { action: 'none', frameAllows: [], requestHeaders: [], responseHeaders: [], used: [] }
  const byExt = new Map<string, CompiledRule[]>()
  for (const r of matches) {
    if (!ctx.canAct(r)) continue
    const list = byExt.get(r.ext)
    if (list) list.push(r)
    else byExt.set(r.ext, [r])
  }
  const exts = [...new Set([...byExt.keys(), ...(ctx.frameAllows?.keys() ?? [])])].sort((a, b) => ctx.rank(a) - ctx.rank(b))
  const allowedAt = new Map<string, number>()
  let decided = false
  const isFrame = q.type === 'main_frame' || q.type === 'sub_frame'
  for (const ext of exts) {
    let best: Candidate | undefined
    for (const r of byExt.get(ext) ?? []) {
      if (r.type === 'modifyHeaders') continue
      const kind =
        r.type === 'allow' || r.type === 'allowAllRequests' ? 'allow' : r.type === 'block' ? 'block' : r.type === 'upgradeScheme' ? 'upgrade' : 'redirect'
      const c: Candidate = { priority: r.priority, rank: SOURCE_RANK(r), rule: r, kind }
      if (better(c, best)) best = c
    }
    const inherited = ctx.frameAllows?.get(ext)
    if (inherited !== undefined) {
      const c: Candidate = { priority: inherited, rank: ACTION_RANK.allowAllRequests, kind: 'allow' }
      if (better(c, best)) best = c
    }
    if (!best) continue
    if (best.kind === 'allow') {
      allowedAt.set(ext, best.priority)
      if (best.rule?.type === 'allowAllRequests' && isFrame) result.frameAllows.push({ ext, priority: best.priority })
    }
    if (decided) continue
    decided = true
    result.action = best.kind
    result.rule = best.rule
    if (best.rule) result.used.push(best.rule)
  }
  // Inherited allowances of extensions without any matching rule still hide their lower-priority header changes.
  if (result.action === 'block' || result.action === 'redirect' || result.action === 'upgrade') return result
  // Header changes: those with a priority above the extension's own allowance.
  const header: { rule: CompiledRule; rank: number }[] = []
  for (const ext of exts) {
    const floor = allowedAt.get(ext) ?? 0
    for (const r of byExt.get(ext) ?? []) if (r.type === 'modifyHeaders' && r.priority > floor) header.push({ rule: r, rank: ctx.rank(ext) })
  }
  header.sort((a, b) => a.rank - b.rank || b.rule.priority - a.rule.priority)
  result.requestHeaders = collectOps(header, 'requestHeaders')
  result.responseHeaders = collectOps(header, 'responseHeaders')
  for (const h of header) {
    const a = h.rule.raw.action
    if ((Array.isArray(a.requestHeaders) && a.requestHeaders.length) || (Array.isArray(a.responseHeaders) && a.responseHeaders.length)) result.used.push(h.rule)
  }
  return result
}

/**
 * Operations in precedence order. A `set` or `remove` of a header locks it against anything from a lower priority or
 * another extension; `append` and equal-priority operations combine.
 */
function collectOps(rules: { rule: CompiledRule; rank: number }[], key: 'requestHeaders' | 'responseHeaders'): HeaderOp[] {
  const ops: HeaderOp[] = []
  const locked = new Map<string, { rank: number; priority: number }>()
  for (const { rule, rank } of rules) {
    const list = rule.raw.action[key]
    if (!Array.isArray(list)) continue
    for (const op of list as HeaderOp[]) {
      if (!op || typeof op.header !== 'string') continue
      const name = op.header.toLowerCase()
      const lock = locked.get(name)
      if (lock && (lock.rank !== rank || lock.priority > rule.priority)) continue
      if (op.operation !== 'remove' && typeof op.value !== 'string') continue
      ops.push(op)
      if (op.operation !== 'append' && !lock) locked.set(name, { rank, priority: rule.priority })
    }
  }
  return ops
}

/** Applies header operations to a copy. `multi`: values are arrays (response headers). */
export function applyHeaderOps<T extends string | string[]>(headers: Record<string, T>, ops: HeaderOp[], multi: boolean): Record<string, T> {
  const out: Record<string, string | string[]> = { ...headers }
  const find = (name: string) => Object.keys(out).filter((k) => k.toLowerCase() === name)
  for (const op of ops) {
    const name = op.header.toLowerCase()
    const keys = find(name)
    if (op.operation === 'remove') {
      for (const k of keys) delete out[k]
    } else if (op.operation === 'set') {
      for (const k of keys) delete out[k]
      out[op.header] = multi ? [op.value!] : op.value!
    } else if (op.operation === 'append') {
      const key = keys[0]
      if (key === undefined) out[op.header] = multi ? [op.value!] : op.value!
      else if (multi) out[key] = [...(out[key] as string[]), op.value!]
      else out[key] = `${out[key] as string}, ${op.value}`
    }
  }
  return out as Record<string, T>
}

// ---------------------------------------------------------------- redirects

interface Transform {
  scheme?: string
  host?: string
  port?: string
  path?: string
  query?: string
  queryTransform?: { removeParams?: string[]; addOrReplaceParams?: { key: string; value: string; replaceOnly?: boolean }[] }
  fragment?: string
  username?: string
  password?: string
}

const safeDecode = (s: string): string => {
  try {
    return decodeURIComponent(s.replace(/\+/g, ' '))
  } catch {
    return s
  }
}

function transformQuery(search: string, t: NonNullable<Transform['queryTransform']>): string {
  let parts = search.replace(/^\?/, '').split('&').filter(Boolean)
  const keyOf = (p: string) => safeDecode(p.split('=')[0])
  if (t.removeParams?.length) {
    const remove = new Set(t.removeParams)
    parts = parts.filter((p) => !remove.has(keyOf(p)))
  }
  for (const add of t.addOrReplaceParams ?? []) {
    const entry = `${encodeURIComponent(add.key)}=${encodeURIComponent(add.value)}`
    if (parts.some((p) => keyOf(p) === add.key)) parts = parts.map((p) => (keyOf(p) === add.key ? entry : p))
    else if (!add.replaceOnly) parts.push(entry)
  }
  return parts.length ? `?${parts.join('&')}` : ''
}

/** Result of `redirect.transform` on a URL; null if it cannot be applied. */
export function applyTransform(url: string, t: Transform): string | null {
  try {
    const u = new URL(url)
    if (typeof t.scheme === 'string') u.protocol = t.scheme
    if (typeof t.username === 'string') u.username = t.username
    if (typeof t.password === 'string') u.password = t.password
    if (typeof t.host === 'string') u.hostname = t.host
    if (typeof t.port === 'string') u.port = t.port
    if (typeof t.path === 'string') u.pathname = t.path
    if (typeof t.query === 'string') u.search = t.query
    if (t.queryTransform) u.search = transformQuery(u.search, t.queryTransform)
    if (typeof t.fragment === 'string') u.hash = t.fragment
    return u.href
  } catch {
    return null
  }
}

/** `regexSubstitution`: `\0`..`\9` are the groups of the match, `\\` a backslash. */
export function substitute(re: RegExp, url: string, replacement: string): string | null {
  const m = re.exec(url)
  if (!m) return null
  return replacement.replace(/\\([0-9\\])/g, (_all, c: string) => (c === '\\' ? '\\' : (m[Number(c)] ?? '')))
}

/** The URL a redirect rule sends the request to; null when there is none (invalid, or the same URL). */
export function redirectTarget(rule: CompiledRule, q: DnrRequest): string | null {
  const a = (rule.raw.action as { redirect?: Record<string, unknown> }).redirect
  if (!a || typeof a !== 'object') return null
  let target: string | null = null
  try {
    if (typeof a.url === 'string') {
      const u = new URL(a.url)
      target = u.protocol === 'javascript:' ? null : u.href
    } else if (typeof a.extensionPath === 'string' && a.extensionPath.startsWith('/')) {
      target = `chrome-extension://${rule.ext}${a.extensionPath}`
    } else if (a.transform && typeof a.transform === 'object') {
      target = applyTransform(q.url, a.transform as Transform)
    } else if (typeof a.regexSubstitution === 'string') {
      const re = rule.regexFilter ? regexOf(rule) : null
      const replaced = re ? substitute(re, q.url, a.regexSubstitution) : null
      target = replaced ? new URL(replaced).href : null
    }
  } catch {
    return null
  }
  return target && target !== q.url ? target : null
}

/** http becomes https and ws becomes wss (port 80 goes with it); null when the URL is already secure. */
export function upgradedUrl(url: string): string | null {
  try {
    const u = new URL(url)
    if (u.protocol !== 'http:' && u.protocol !== 'ws:') return null
    const port = u.port
    u.protocol = u.protocol === 'http:' ? 'https:' : 'wss:'
    if (port === '80') u.port = ''
    return u.href
  } catch {
    return null
  }
}

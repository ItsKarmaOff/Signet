// Conversion of a filter list (Adblock Plus syntax: EasyList, EasyPrivacy) to Safari / WebKit's "content blocker" format.
// Pure logic: the script app/mobile/scripts/content-rules.mjs reads the lists and writes the JSON; the app only ships the result.
// We prefer dropping a rule over converting it wrongly: anything not safely expressible is ignored (and counted).

export type ResourceType = 'document' | 'image' | 'style-sheet' | 'script' | 'font' | 'raw' | 'svg-document' | 'media' | 'popup' | 'ping' | 'other'

export interface RuleTrigger {
  'url-filter': string
  'url-filter-is-case-sensitive'?: boolean
  'if-domain'?: string[]
  'unless-domain'?: string[]
  'resource-type'?: ResourceType[]
  'load-type'?: ('first-party' | 'third-party')[]
}

export interface SafariRule {
  trigger: RuleTrigger
  action: { type: 'block' | 'ignore-previous-rules' } | { type: 'css-display-none'; selector: string }
}

export interface ConvertStats {
  lines: number
  network: number
  exceptions: number
  cosmetic: number
  skipped: number
}

export interface ConvertResult {
  rules: SafariRule[]
  stats: ConvertStats
}

export interface ConvertOptions {
  /** Also converts site-specific hiding (`site.com##.ad`). More faithful, but many rules. Default: true. */
  siteCosmetic?: boolean
  /** Size of the batches of generic selectors grouped into one rule. Default: 120. */
  cosmeticChunk?: number
}

const ALL_TYPES: ResourceType[] = ['document', 'image', 'style-sheet', 'script', 'font', 'raw', 'svg-document', 'media', 'popup', 'ping', 'other']

const TYPE_OPTIONS: Record<string, ResourceType[]> = {
  script: ['script'],
  image: ['image', 'svg-document'],
  stylesheet: ['style-sheet'],
  css: ['style-sheet'],
  xmlhttprequest: ['raw'],
  xhr: ['raw'],
  media: ['media'],
  font: ['font'],
  ping: ['ping'],
  popup: ['popup'],
  other: ['other'],
  object: ['other'],
  'object-subrequest': ['other'],
  subdocument: ['document'],
  frame: ['document']
}

/** Options with no effect on address matching (ignored). */
const HARMLESS_OPTIONS = new Set(['important', 'all'])

const SAFE_PSEUDO = new Set([
  'not',
  'first-child',
  'last-child',
  'only-child',
  'first-of-type',
  'last-of-type',
  'nth-child',
  'nth-of-type',
  'nth-last-child',
  'empty'
])

const isAscii = (s: string): boolean => /^[\x20-\x7e]*$/.test(s)

/** Escapes a literal text for WebKit's regular expression subset. */
function escapeLiteral(s: string): string {
  return s.replace(/[.+?()[\]{}|\\$^*]/g, '\\$&')
}

/** Adblock filter pattern → WebKit regular expression; `null` if inexpressible. */
export function patternToRegex(pattern: string): string | null {
  let p = pattern
  if (!isAscii(p)) return null
  if (p.length > 2 && p.startsWith('/') && p.endsWith('/')) return null // regular expression: outside the subset
  let prefix = ''
  if (p.startsWith('||')) {
    p = p.slice(2)
    prefix = '^[^:]+://+([^/]+\\.)?'
  } else if (p.startsWith('|')) {
    p = p.slice(1)
    prefix = '^'
  }
  let suffix = ''
  if (p.endsWith('|') && !p.endsWith('\\|')) {
    p = p.slice(0, -1)
    suffix = '$'
  }
  // A leading or trailing `*` is useless (substring search).
  p = p.replace(/^\*+/, '').replace(/\*+$/, '')
  if (/[|]/.test(p)) return null
  let body = ''
  for (const ch of p) {
    if (ch === '*') body += '.*'
    else if (ch === '^') body += '[^a-zA-Z0-9_.%-]'
    else body += escapeLiteral(ch)
  }
  if (!prefix && !suffix && p.replace(/[*^]/g, '').length < 3) return null // too generic: would break pages
  if (prefix === '^' && !body) return null
  if (!body && !prefix) return null
  return prefix + body + suffix
}

interface NetworkOptions {
  types?: ResourceType[]
  thirdParty?: boolean
  firstParty?: boolean
  ifDomain?: string[]
  unlessDomain?: string[]
  caseSensitive?: boolean
  subdocument?: boolean
}

/** Reads the option list of a network filter; `null` if any of them is not safely expressible. */
function parseOptions(raw: string): NetworkOptions | null {
  const out: NetworkOptions = {}
  let include: ResourceType[] | null = null
  let exclude: ResourceType[] = []
  for (const opt of raw.split(',')) {
    const o = opt.trim().toLowerCase()
    if (!o) continue
    if (HARMLESS_OPTIONS.has(o)) continue
    if (o === 'third-party' || o === '3p') out.thirdParty = true
    else if (o === '~third-party' || o === '~3p' || o === 'first-party' || o === '1p') out.firstParty = true
    else if (o === 'match-case') out.caseSensitive = true
    else if (o.startsWith('domain=')) {
      for (const d of o.slice(7).split('|')) {
        const neg = d.startsWith('~')
        const name = (neg ? d.slice(1) : d).trim()
        if (!/^[a-z0-9.-]+$/.test(name) || !name) return null
        ;(neg ? (out.unlessDomain ??= []) : (out.ifDomain ??= [])).push('*' + name)
      }
    } else if (o in TYPE_OPTIONS) {
      include = [...(include ?? []), ...TYPE_OPTIONS[o]]
      if (o === 'subdocument' || o === 'frame') out.subdocument = true
    } else if (o.startsWith('~') && o.slice(1) in TYPE_OPTIONS) {
      exclude = [...exclude, ...TYPE_OPTIONS[o.slice(1)]]
    } else return null
  }
  if (include || exclude.length) {
    const base = include ?? ALL_TYPES.filter((t) => t !== 'document')
    out.types = [...new Set(base)].filter((t) => !exclude.includes(t))
    if (!out.types.length) return null
  }
  if (out.thirdParty && out.firstParty) return null
  return out
}

function networkRule(line: string, exception: boolean): SafariRule | null {
  let pattern = line
  let optionText = ''
  const dollar = pattern.lastIndexOf('$')
  if (dollar >= 0 && !/^\/.*\/$/.test(pattern)) {
    const tail = pattern.slice(dollar + 1)
    if (/^[~a-z0-9_,=.|*-]+$/i.test(tail) && !tail.includes('*')) {
      optionText = tail
      pattern = pattern.slice(0, dollar)
    }
  }
  const opts = optionText ? parseOptions(optionText) : {}
  if (!opts) return null
  const filter = patternToRegex(pattern)
  if (!filter) return null
  // A "document" rule with no third-party condition would block legitimate navigations to that domain.
  if (opts.subdocument && !opts.thirdParty) return null
  const trigger: RuleTrigger = { 'url-filter': filter }
  if (opts.caseSensitive) trigger['url-filter-is-case-sensitive'] = true
  if (opts.ifDomain) trigger['if-domain'] = opts.ifDomain
  else if (opts.unlessDomain) trigger['unless-domain'] = opts.unlessDomain
  // With no type specified: an anchored domain (`||site^`) blocks everything, pages and frames included; a plain path pattern spares pages (`document`).
  if (opts.types) trigger['resource-type'] = opts.types
  else if (!filter.startsWith('^[^:]+://+')) trigger['resource-type'] = ALL_TYPES.filter((t) => t !== 'document')
  if (opts.thirdParty) trigger['load-type'] = ['third-party']
  else if (opts.firstParty) trigger['load-type'] = ['first-party']
  return { trigger, action: exception ? { type: 'ignore-previous-rules' } : { type: 'block' } }
}

/** True if the CSS selector is simple enough for WebKit to surely accept it. */
export function isSafeSelector(sel: string): boolean {
  const s = sel.trim()
  if (!s || s.length > 400 || !isAscii(s)) return false
  if (/[{}]|\/\*|\\|;|<|>>/.test(s)) return false
  if (/^[>+~,]|[>+~,]$/.test(s)) return false
  // Balanced quotes.
  if ((s.match(/"/g)?.length ?? 0) % 2 || (s.match(/'/g)?.length ?? 0) % 2) return false
  // Outside quotes: balanced parentheses and brackets, known pseudo-classes only.
  const bare = s.replace(/"[^"]*"|'[^']*'/g, '""')
  let paren = 0
  let bracket = 0
  for (const ch of bare) {
    if (ch === '(') paren++
    else if (ch === ')') paren--
    else if (ch === '[') bracket++
    else if (ch === ']') bracket--
    if (paren < 0 || bracket < 0) return false
  }
  if (paren || bracket) return false
  if (/::/.test(bare)) return false
  for (const m of bare.matchAll(/:([a-zA-Z-]+)/g)) {
    if (!SAFE_PSEUDO.has(m[1].toLowerCase())) return false
  }
  if (/:not\(\s*:not|:not\([^)]*[,>+~ ]/.test(bare)) return false // :not() simple uniquement
  return /^[\w\s.#>+~,[\]="'^$*|:()%!@/-]+$/.test(s)
}

/** Converts the text of a filter list. */
export function convertFilterList(text: string, options: ConvertOptions = {}): ConvertResult {
  const siteCosmetic = options.siteCosmetic ?? true
  const chunk = options.cosmeticChunk ?? 120
  const stats: ConvertStats = { lines: 0, network: 0, exceptions: 0, cosmetic: 0, skipped: 0 }
  const blocks: SafariRule[] = []
  const exceptions: SafariRule[] = []
  const generic = new Set<string>()
  const perSite = new Map<string, Set<string>>()
  const seen = new Set<string>()

  for (const rawLine of text.split(/\r?\n/)) {
    const line = rawLine.trim()
    if (!line || line.startsWith('!') || line.startsWith('[')) continue
    stats.lines++
    const cosmetic = /^([^#$]*)##(.+)$/.exec(line)
    if (cosmetic) {
      const [, domainPart, selector] = cosmetic
      if (!isSafeSelector(selector) || selector.startsWith('+js(')) {
        stats.skipped++
        continue
      }
      if (!domainPart) {
        generic.add(selector.trim())
        stats.cosmetic++
        continue
      }
      const positives = domainPart
        .split(',')
        .map((d) => d.trim().toLowerCase())
        .filter((d) => d && !d.startsWith('~'))
      if (!siteCosmetic || !positives.length || positives.some((d) => !/^[a-z0-9.-]+$/.test(d))) {
        stats.skipped++
        continue
      }
      for (const d of positives) {
        const set = perSite.get(d) ?? new Set<string>()
        set.add(selector.trim())
        perSite.set(d, set)
      }
      stats.cosmetic++
      continue
    }
    if (/#[@?$%]#|\$\$/.test(line)) {
      stats.skipped++
      continue
    }
    const isException = line.startsWith('@@')
    const rule = networkRule(isException ? line.slice(2) : line, isException)
    if (!rule) {
      stats.skipped++
      continue
    }
    const key = JSON.stringify(rule)
    if (seen.has(key)) continue
    seen.add(key)
    ;(isException ? exceptions : blocks).push(rule)
    if (isException) stats.exceptions++
    else stats.network++
  }

  const hiding: SafariRule[] = []
  const g = [...generic]
  for (let i = 0; i < g.length; i += chunk) {
    hiding.push({ trigger: { 'url-filter': '.*' }, action: { type: 'css-display-none', selector: g.slice(i, i + chunk).join(', ') } })
  }
  for (const [domain, sels] of perSite) {
    const list = [...sels]
    for (let i = 0; i < list.length; i += chunk) {
      hiding.push({
        trigger: { 'url-filter': '.*', 'if-domain': ['*' + domain] },
        action: { type: 'css-display-none', selector: list.slice(i, i + chunk).join(', ') }
      })
    }
  }
  // Order required by WebKit: exceptions come after what they cancel.
  return { rules: [...blocks, ...hiding, ...exceptions], stats }
}

// Reading of RSS 2.0 and Atom feeds, with no dependency on or access to Electron (pure, testable functions).
import type { LiveFolder, LiveSource } from './types'

/** Feed item: address, title, date (ms, 0 if unknown). */
export interface FeedEntry {
  url: string
  title: string
  date: number
}

export interface Feed {
  title: string
  entries: FeedEntry[]
}

const ENTITIES: Record<string, string> = { amp: '&', lt: '<', gt: '>', quot: '"', apos: "'", nbsp: ' ' }

/** Decodes common XML/HTML entities (`&amp;`, `&#39;`, `&#x2019;`…). */
export function decodeEntities(text: string): string {
  return text.replace(/&(#x[0-9a-f]+|#\d+|[a-z]+);/gi, (m, code: string) => {
    if (code[0] === '#') {
      const n = code[1] === 'x' || code[1] === 'X' ? parseInt(code.slice(2), 16) : parseInt(code.slice(1), 10)
      return Number.isFinite(n) && n > 0 && n < 0x110000 ? String.fromCodePoint(n) : m
    }
    return ENTITIES[code.toLowerCase()] ?? m
  })
}

/** Text of a node: CDATA unwrapped, HTML tags removed, entities decoded, whitespace collapsed. */
function text(raw: string | undefined): string {
  if (!raw) return ''
  let s = raw.replace(/<!\[CDATA\[([\s\S]*?)\]\]>/g, '$1')
  // A title may contain escaped HTML (`&lt;b&gt;`): decode once before removing tags.
  s = decodeEntities(s.replace(/<[^>]+>/g, ''))
  return s
    .replace(/<[^>]+>/g, '')
    .replace(/\s+/g, ' ')
    .trim()
}

/** Content of the first `name` tag (with or without namespace prefix) in `xml`. */
function tag(xml: string, name: string): string | undefined {
  const m = new RegExp(`<(?:[\\w-]+:)?${name}(?:\\s[^>]*)?>([\\s\\S]*?)</(?:[\\w-]+:)?${name}>`, 'i').exec(xml)
  return m?.[1]
}

/** Attributes of an opening tag (`<link rel="alternate" href="…">`). */
export function attributes(open: string): Record<string, string> {
  const out: Record<string, string> = {}
  for (const m of open.matchAll(/([\w:-]+)\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s"'>]+))/g)) out[m[1].toLowerCase()] = decodeEntities(m[2] ?? m[3] ?? m[4] ?? '')
  return out
}

function absolute(url: string, base: string): string | null {
  try {
    const u = new URL(url.trim(), base)
    return u.protocol === 'https:' || u.protocol === 'http:' ? u.href : null
  } catch {
    return null
  }
}

function time(raw: string | undefined): number {
  const n = Date.parse(text(raw))
  return Number.isFinite(n) ? n : 0
}

/** Reads an RSS 2.0 (or RSS 1.0 / RDF) or Atom feed; null if it is not a feed. */
export function parseFeed(xml: string, base: string): Feed | null {
  const atom = /<feed[\s>]/i.test(xml) && /<entry[\s>]/i.test(xml)
  const rss = /<(rss|rdf:RDF)[\s>]/i.test(xml) || /<channel[\s>]/i.test(xml)
  if (!atom && !rss && !/<feed[\s>]/i.test(xml)) return null
  const entries: FeedEntry[] = []
  if (atom || /<feed[\s>]/i.test(xml)) {
    const head = xml.split(/<entry[\s>]/i)[0]
    const title = text(tag(head, 'title'))
    for (const m of xml.matchAll(/<entry[\s>]([\s\S]*?)<\/entry>/gi)) {
      const body = m[1]
      // Link: rel="alternate" (or no rel) preferred, otherwise the first.
      const links = [...body.matchAll(/<link\b([^>]*)\/?>/gi)].map((l) => attributes(l[1]))
      const link = links.find((a) => !a.rel || a.rel === 'alternate') ?? links[0]
      const url = link?.href ? absolute(link.href, base) : null
      if (!url) continue
      entries.push({ url, title: text(tag(body, 'title')) || url, date: time(tag(body, 'updated') ?? tag(body, 'published')) })
    }
    return { title, entries }
  }
  const channel = tag(xml, 'channel') ?? xml
  const title = text(tag(channel.split(/<item[\s>]/i)[0], 'title'))
  for (const m of xml.matchAll(/<item[\s>]([\s\S]*?)<\/item>/gi)) {
    const body = m[1]
    let link = text(tag(body, 'link'))
    // No <link>: a <guid> that is an address.
    if (!link) {
      const guid = text(tag(body, 'guid'))
      if (/^https?:\/\//i.test(guid)) link = guid
    }
    const url = link ? absolute(link, base) : null
    if (!url) continue
    entries.push({ url, title: text(tag(body, 'title')) || url, date: time(tag(body, 'pubDate') ?? tag(body, 'date') ?? tag(body, 'updated')) })
  }
  return { title, entries }
}

/** Address of the feed announced by an HTML page (`<link rel="alternate" type="application/rss+xml" href="…">`), or null. */
export function discoverFeed(html: string, base: string): string | null {
  for (const m of html.matchAll(/<link\b([^>]*)>/gi)) {
    const a = attributes(m[1])
    if (!/\balternate\b/i.test(a.rel ?? '')) continue
    if (!/^application\/(rss|atom)\+xml$/i.test((a.type ?? '').trim())) continue
    const url = a.href ? absolute(a.href, base) : null
    if (url) return url
  }
  return null
}

/** True if the text looks like an HTML page rather than a feed. */
export function looksLikeHtml(body: string): boolean {
  return /^\s*(<!doctype html|<html[\s>])/i.test(body.slice(0, 2000))
}

/** Validated source of a live folder (read from disk or coming from the UI), or null. */
export function cleanLiveSource(value: unknown): LiveSource | null {
  if (!value || typeof value !== 'object') return null
  const v = value as Partial<{ kind: string; url: unknown; query: unknown }>
  if (v.kind === 'feed' && typeof v.url === 'string' && /^https?:\/\/\S+$/i.test(v.url.trim())) return { kind: 'feed', url: v.url.trim().slice(0, 2000) }
  if (v.kind === 'github' && (v.query === 'review-requested' || v.query === 'mine')) return { kind: 'github', query: v.query }
  return null
}

/** State of a live folder re-read from disk: valid source, addresses seen; never "update in progress". */
export function cleanLiveFolder(value: unknown): LiveFolder | undefined {
  if (!value || typeof value !== 'object') return undefined
  const v = value as Partial<LiveFolder>
  const source = cleanLiveSource(v.source)
  if (!source) return undefined
  const live: LiveFolder = { source }
  if (Number.isFinite(v.refreshedAt)) live.refreshedAt = Number(v.refreshedAt)
  if (typeof v.error === 'string' && v.error) live.error = v.error.slice(0, 300)
  if (Array.isArray(v.seen)) live.seen = v.seen.filter((u): u is string => typeof u === 'string').slice(-200)
  return live
}

import type { SpaceRoute } from './types'

const escape = (s: string) => s.replace(/[.+?^${}()|[\]\\]/g, '\\$&')

/** `*` = any sequence of characters except `/`. */
const glob = (s: string) => escape(s).replace(/\*/g, '[^/]*')

/** Cleans a hand-typed pattern: no scheme, lowercase, no trailing `/`. */
export function normalizePattern(input: string): string {
  return input
    .trim()
    .toLowerCase()
    .replace(/^[a-z][a-z0-9+.-]*:\/\//, '')
    .replace(/\/+$/, '')
}

/** Does the pattern (host, with an optional path) match this URL? Equal host or subdomain. */
export function matchPattern(pattern: string, url: string): boolean {
  let parsed: URL
  try {
    parsed = new URL(url)
  } catch {
    return false
  }
  if (!/^https?:$/.test(parsed.protocol)) return false
  const clean = normalizePattern(pattern)
  if (!clean) return false
  const slash = clean.indexOf('/')
  const hostPattern = slash < 0 ? clean : clean.slice(0, slash)
  const pathPattern = slash < 0 ? '' : clean.slice(slash)
  const host = parsed.hostname.toLowerCase()
  if (!new RegExp(`^(?:[^/]*\\.)?${glob(hostPattern)}$`).test(host)) return false
  return !pathPattern || new RegExp(`^${glob(pathPattern)}(?:[/?#]|$)`).test(parsed.pathname.toLowerCase())
}

/** First rule that matches this URL. */
export function matchRoute(url: string, routes: SpaceRoute[]): SpaceRoute | undefined {
  return routes.find((r) => matchPattern(r.pattern, url))
}

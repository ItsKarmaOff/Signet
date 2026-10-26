import type { DevAlias } from './types'

/** Host of a development server: the machine itself, a name reserved for tests, or a local network address. */
export function isLocalHost(hostname: string): boolean {
  const h = hostname.toLowerCase().replace(/^\[|\]$/g, '')
  if (h === 'localhost' || h.endsWith('.localhost') || h.endsWith('.local') || h.endsWith('.test')) return true
  if (h === '::1' || h === '0.0.0.0') return true
  const ip = /^(\d{1,3})\.(\d{1,3})\.\d{1,3}\.\d{1,3}$/.exec(h)
  if (!ip) return false
  const [a, b] = [Number(ip[1]), Number(ip[2])]
  return a === 127 || a === 10 || (a === 192 && b === 168) || (a === 172 && b >= 16 && b <= 31)
}

/** http(s) page served by a development server (see `isLocalHost`). */
export function isLocalDevUrl(url: string): boolean {
  if (!/^https?:/i.test(url)) return false
  try {
    return isLocalHost(new URL(url).hostname)
  } catch {
    return false
  }
}

const ALIAS_NAME = /^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?(?:\.(?:test|localhost))?$/

/** Valid name (lowercase, digits, hyphens; optional `.test` or `.localhost` suffix). */
export function isAliasName(name: string): boolean {
  return ALIAS_NAME.test(name)
}

/**
 * Target address of a local name, from what is typed: `7353`, `localhost:7353`, `127.0.0.1:3000/app` or a full
 * address. Local addresses only (`isLocalDevUrl`); returns null otherwise. No query or fragment, no trailing `/`.
 */
export function aliasTarget(input: string): string | null {
  let text = input.trim()
  if (/^\d{2,5}$/.test(text)) text = `localhost:${text}`
  if (!/^https?:\/\//i.test(text)) text = `http://${text}`
  try {
    const url = new URL(text)
    if (!isLocalDevUrl(url.href)) return null
    return `${url.origin}${url.pathname}`.replace(/\/+$/, '')
  } catch {
    return null
  }
}

/** Address from an input that starts with a local name (`mysite`, `mysite/admin?x=1`, `http://mysite.test/…`), or null. */
export function resolveAlias(input: string, aliases: readonly DevAlias[] | undefined): string | null {
  if (!aliases?.length) return null
  const m = /^(?:https?:\/\/)?([a-z0-9.-]+)([/?#].*)?$/i.exec(input.trim())
  if (!m) return null
  const alias = aliases.find((a) => a.name === m[1].toLowerCase())
  if (!alias) return null
  const rest = m[2] ?? ''
  return !rest || /^[/?#]/.test(rest) ? `${alias.target}${rest}` : `${alias.target}/${rest}`
}

/** Local name matching an address (same origin, path under the target's), or null. */
export function aliasFor(url: string, aliases: readonly DevAlias[] | undefined): DevAlias | null {
  if (!aliases?.length || !/^https?:/i.test(url)) return null
  return aliases.find((a) => url === a.target || url.startsWith(`${a.target}/`) || url.startsWith(`${a.target}?`) || url.startsWith(`${a.target}#`)) ?? null
}

// HTTPS-only mode: which http:// addresses are upgraded to https://. Pure (no Electron), unit tested.
import { isHttpsFallbackError } from '@signet/shared/urls'

/** Hosts typed as IPv6 literals come bracketed from `URL.hostname`. */
const unbracket = (host: string) => host.replace(/^\[|\]$/g, '')

function isPrivateIpv4(h: string): boolean {
  const m = /^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})$/.exec(h)
  if (!m) return false
  const [a, b] = [Number(m[1]), Number(m[2])]
  return a === 0 || a === 127 || a === 10 || (a === 172 && b >= 16 && b <= 31) || (a === 192 && b === 168) || (a === 169 && b === 254)
}

function isLocalIpv6(h: string): boolean {
  if (!h.includes(':')) return false
  if (h === '::1' || h === '::') return true
  // IPv4-mapped (`::ffff:7f00:1`, as URL normalizes it): the address inside decides.
  const mapped = /^::ffff:([0-9a-f]{1,4}):([0-9a-f]{1,4})$/.exec(h)
  if (mapped) {
    const hi = parseInt(mapped[1], 16)
    const lo = parseInt(mapped[2], 16)
    return isPrivateIpv4(`${hi >> 8}.${hi & 255}.${lo >> 8}.${lo & 255}`)
  }
  const first = parseInt(h.split(':')[0] || '0', 16)
  if (!Number.isFinite(first)) return false
  return (first & 0xfe00) === 0xfc00 || (first & 0xffc0) === 0xfe80 // unique local fc00::/7, link-local fe80::/10
}

/**
 * Host that stays on http even in HTTPS-only mode: `localhost` and `*.localhost`, loopback, private and link-local
 * addresses, `.local`, and single-label names (an intranet).
 */
export function isExemptHost(hostname: string): boolean {
  const h = unbracket(hostname.toLowerCase()).replace(/\.$/, '')
  if (!h) return true
  if (h === 'localhost' || h.endsWith('.localhost') || h.endsWith('.local')) return true
  if (isPrivateIpv4(h) || isLocalIpv6(h)) return true
  return !h.includes('.') && !h.includes(':') // single label
}

export interface UpgradeOptions {
  /** Hosts the user allowed through the warning (per session). */
  allowedHosts?: ReadonlySet<string>
  /** Targets of the local names (dev aliases): their hosts stay on http. */
  aliasTargets?: readonly string[]
}

/**
 * The https:// address to open instead of an http:// one, or null to leave it as is (not http, exempt host, allowed host).
 * Same host, path, query and fragment; an explicit port is kept.
 */
export function httpsUpgrade(url: string, opts: UpgradeOptions = {}): string | null {
  let u: URL
  try {
    u = new URL(url)
  } catch {
    return null
  }
  if (u.protocol !== 'http:') return null
  const host = u.hostname.toLowerCase()
  if (isExemptHost(host) || opts.allowedHosts?.has(host)) return null
  for (const target of opts.aliasTargets ?? []) {
    try {
      if (new URL(target).hostname.toLowerCase() === host) return null
    } catch {
      // invalid target: ignored
    }
  }
  u.protocol = 'https:'
  return u.href
}

/** Load failure that says the site has no working secure connection (rather than no site at all, like a DNS error). */
export function isInsecureSiteError(code: number): boolean {
  return code === -103 || isHttpsFallbackError(code)
}

/** True if `failedUrl` is the https:// version (same host) of the http:// address that was upgraded. */
export function isUpgradeOf(httpUrl: string, failedUrl: string): boolean {
  try {
    const a = new URL(httpUrl)
    const b = new URL(failedUrl)
    return a.protocol === 'http:' && b.protocol === 'https:' && a.hostname.toLowerCase() === b.hostname.toLowerCase()
  } catch {
    return false
  }
}

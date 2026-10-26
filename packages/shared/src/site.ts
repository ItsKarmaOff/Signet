import { getDomain } from 'tldts'

const IPV4 = /^\d+(\.\d+){3}$/

/** Registrable domain according to the Public Suffix List (private suffixes included): `www.bbc.co.uk` → `bbc.co.uk` (with the port for localhost and IP addresses). */
export function siteOf(url: string): string {
  let host: string
  let port: string
  try {
    const u = new URL(url)
    host = u.hostname.replace(/^www\./i, '').toLowerCase()
    port = u.port
  } catch {
    return ''
  }
  // Distinct ports = distinct applications (localhost:3000 and localhost:5173 do not share the same Boost).
  if (IPV4.test(host) || host.includes(':') || host === 'localhost') return port ? `${host}:${port}` : host
  // null for single-label hosts or `.lan`/`.local`: keep the host as is.
  return getDomain(host, { allowPrivateDomains: true }) ?? host
}

// Secure DNS (Settings › Privacy): DNS over HTTPS servers offered, shared by main and the UI.

/** DNS over HTTPS providers offered (same servers as Chrome); any other https address can be typed. */
export const DNS_PROVIDERS = [
  { id: 'cloudflare', name: 'Cloudflare (1.1.1.1)', template: 'https://chrome.cloudflare-dns.com/dns-query' },
  { id: 'google', name: 'Google (Public DNS)', template: 'https://dns.google/dns-query{?dns}' },
  { id: 'quad9', name: 'Quad9 (9.9.9.9)', template: 'https://dns.quad9.net/dns-query' },
  { id: 'nextdns', name: 'NextDNS', template: 'https://chromium.dns.nextdns.io' },
  { id: 'cleanbrowsing', name: 'CleanBrowsing (Family Filter)', template: 'https://doh.cleanbrowsing.org/doh/family-filter{?dns}' }
] as const

/** Server used by `secure` mode when none was chosen. */
export const DEFAULT_DNS_SERVER: string = DNS_PROVIDERS[0].template

/** True for a usable DNS over HTTPS server template: an https address, without credentials. */
export function isDohTemplate(value: unknown): value is string {
  if (typeof value !== 'string' || value.length > 2048) return false
  try {
    const u = new URL(value.replace(/\{\?dns\}$/, ''))
    return u.protocol === 'https:' && !!u.hostname && !u.username && !u.password
  } catch {
    return false
  }
}

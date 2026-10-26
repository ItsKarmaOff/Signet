// "localhost" addresses: what the phone sends to the computer instead of looking it up on itself.

/** The machine itself: `localhost`, `*.localhost`, 127.0.0.0/8, `::1`, `0.0.0.0`. */
export function isLoopbackHost(hostname: string): boolean {
  const h = hostname.toLowerCase().replace(/^\[|\]$/g, '')
  if (h === 'localhost' || h.endsWith('.localhost')) return true
  if (h === '::1' || h === '0:0:0:0:0:0:0:1' || h === '0.0.0.0') return true
  return /^127\.\d{1,3}\.\d{1,3}\.\d{1,3}$/.test(h)
}

/** http(s) or ws(s) address pointing at the machine itself. */
export function isLoopbackUrl(url: string): boolean {
  if (!/^(https?|wss?):/i.test(url)) return false
  try {
    return isLoopbackHost(new URL(url).hostname)
  } catch {
    return false
  }
}

/**
 * Address to open on the phone: `127.0.0.1`, `[::1]` and `0.0.0.0` become `localhost` (the only name the phone's WebView
 * sends to the computer), the rest is unchanged. Addresses that are not local are returned as is.
 */
export function toLinkLocalhost(url: string): string {
  if (!isLoopbackUrl(url)) return url
  try {
    const u = new URL(url)
    const h = u.hostname.toLowerCase().replace(/^\[|\]$/g, '')
    if (h === 'localhost' || h.endsWith('.localhost')) return url
    // Rebuilt by hand: React Native's URL has no setters.
    return `${u.protocol}//localhost${u.port ? `:${u.port}` : ''}${u.pathname}${u.search}${u.hash}`
  } catch {
    return url
  }
}

/** Port of a local address (80 / 443 by default); null if the address is not local. */
export function loopbackPort(url: string): number | null {
  if (!isLoopbackUrl(url)) return null
  try {
    const u = new URL(url)
    if (u.port) return Number(u.port)
    return /^(https|wss):/i.test(u.protocol) ? 443 : 80
  } catch {
    return null
  }
}

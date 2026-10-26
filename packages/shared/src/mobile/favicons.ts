// Site icons: pick the best among the page's `<link>` tags, and the fallback chain when it is missing or fails to load.
// The injected script sends `{ type: 'icons', base, icons: [{ href, rel, sizes?, type? }] }`.

export interface IconCandidate {
  href: string
  /** Value of the `rel` attribute (`icon`, `shortcut icon`, `apple-touch-icon`…). */
  rel: string
  /** Attribut `sizes` (`32x32`, `16x16 32x32`, `any`). */
  sizes?: string
  type?: string
}

export interface IconsMessage {
  type: 'icons'
  base: string
  icons: IconCandidate[]
}

const MAX_CANDIDATES = 40

/** Largest side announced by `sizes`; 0 if absent, 512 for `any` (vector). */
export function parseIconSize(sizes: string | undefined): number {
  if (!sizes) return 0
  let best = 0
  for (const token of sizes.toLowerCase().split(/\s+/)) {
    if (token === 'any') best = Math.max(best, 512)
    const m = /^(\d+)x(\d+)$/.exec(token)
    if (m) best = Math.max(best, Math.min(Number(m[1]), Number(m[2])))
  }
  return best
}

const relTokens = (rel: string): string[] => rel.toLowerCase().split(/\s+/).filter(Boolean)

/** Resolves `href` against `base`; only http(s) addresses are kept (no `data:`, often huge). */
export function resolveIconUrl(href: string, base?: string): string | null {
  try {
    const url = base ? new URL(href.trim(), base) : new URL(href.trim())
    return /^https?:$/.test(url.protocol) ? url.href : null
  } catch {
    return null
  }
}

/**
 * Best icon: an `apple-touch-icon` of at least 64 px (180 assumed when no size is announced) is preferred, as it is sharp and has no transparency;
 * otherwise the icon closest to 128 px above 64 px, otherwise the largest small icon.
 */
export function pickBestIcon(candidates: readonly IconCandidate[], base?: string): string | null {
  let best: { url: string; score: number } | null = null
  for (const c of candidates.slice(0, MAX_CANDIDATES)) {
    if (!c || typeof c.href !== 'string' || typeof c.rel !== 'string') continue
    const tokens = relTokens(c.rel)
    const apple = tokens.some((t) => t === 'apple-touch-icon' || t === 'apple-touch-icon-precomposed')
    if (!apple && !tokens.includes('icon')) continue
    const url = resolveIconUrl(c.href, base)
    if (!url) continue
    const declared = parseIconSize(c.sizes)
    let score: number
    if (apple) {
      const size = declared || 180
      score = size >= 64 ? 3000 - Math.abs(size - 180) : 1000 + size
    } else {
      const size = declared || 32
      score = size >= 64 ? 2000 - Math.abs(size - 128) : 1000 + size
      if (c.type === 'image/svg+xml' && !declared) score = 1100
    }
    if (!best || score > best.score) best = { url, score }
  }
  return best?.url ?? null
}

/** Reads a WebView message; `null` if it is not an icon list. */
export function parseIconsMessage(data: string): IconsMessage | null {
  try {
    const m = JSON.parse(data) as Partial<IconsMessage> | null
    if (!m || m.type !== 'icons' || !Array.isArray(m.icons)) return null
    const icons: IconCandidate[] = []
    for (const i of m.icons.slice(0, MAX_CANDIDATES)) {
      if (!i || typeof i !== 'object' || typeof i.href !== 'string' || typeof i.rel !== 'string') continue
      icons.push({
        href: i.href,
        rel: i.rel,
        ...(typeof i.sizes === 'string' && i.sizes ? { sizes: i.sizes } : {}),
        ...(typeof i.type === 'string' && i.type ? { type: i.type } : {})
      })
    }
    return { type: 'icons', base: typeof m.base === 'string' ? m.base : '', icons }
  } catch {
    return null
  }
}

/** Widest fallback address: DuckDuckGo's icon service. */
export const duckDuckGoIcon = (host: string): string => `https://icons.duckduckgo.com/ip3/${host}.ico`

/**
 * Icons to try in order: the one chosen for the page, the site's `/apple-touch-icon.png`, then DuckDuckGo.
 * Each candidate is tried as long as the previous one fails to load; after the last, the caller shows a letter.
 */
export function iconChain(stored: string | undefined, pageUrl: string | undefined): string[] {
  const chain: string[] = []
  if (stored) chain.push(stored)
  try {
    const u = new URL(pageUrl ?? '')
    if (/^https?:$/.test(u.protocol) && u.hostname) {
      chain.push(`${u.origin}/apple-touch-icon.png`, duckDuckGoIcon(u.hostname))
    }
  } catch {
    // missing address or unreadable search template: no network fallback
  }
  return [...new Set(chain)]
}

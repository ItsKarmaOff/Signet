// Web page settings (Settings › Appearance › Web pages): choice lists and mappings, pure (main and renderer).

/** Default font size choices in px (Chrome's five levels). 16 is the default. */
export const FONT_SIZES = [9, 12, 16, 20, 24] as const
export const DEFAULT_FONT_SIZE = 16

/** Minimum font size choices in px; 0 = none (the default). */
export const MIN_FONT_SIZES = [0, 6, 8, 10, 12, 14, 16, 18, 20, 24] as const

/** Default page zoom choices in percent. 100 is the default. */
export const ZOOM_PERCENTS = [25, 33, 50, 67, 75, 80, 90, 100, 110, 125, 150, 175, 200, 250, 300, 400, 500] as const
export const DEFAULT_ZOOM = 100

const isOneOf = (list: readonly number[], v: unknown): v is number => typeof v === 'number' && list.includes(v)

export const isFontSize = (v: unknown): v is number => isOneOf(FONT_SIZES, v)
export const isMinFontSize = (v: unknown): v is number => isOneOf(MIN_FONT_SIZES, v)
export const isZoomPercent = (v: unknown): v is number => isOneOf(ZOOM_PERCENTS, v)

/** The `webPreferences` font fields of a tab's view. */
export interface FontPreferences {
  defaultFontSize: number
  defaultMonospaceFontSize: number
  minimumFontSize: number
}

/**
 * Font preferences from the settings. Like Chrome, the monospace size stays 3 px under the default one at 16 px
 * (13 for 16) and follows the same ratio. An invalid or missing value falls back to the default.
 */
export function fontPreferences(settings: { fontSize?: unknown; minimumFontSize?: unknown }): FontPreferences {
  const size = isFontSize(settings.fontSize) ? settings.fontSize : DEFAULT_FONT_SIZE
  return {
    defaultFontSize: size,
    defaultMonospaceFontSize: Math.round((size * 13) / 16),
    minimumFontSize: isMinFontSize(settings.minimumFontSize) ? settings.minimumFontSize : 0
  }
}

/** Host whose zoom Chromium shares between tabs: the host name of an http(s) page, null for anything else (internal pages, files, PDFs). */
export function zoomHost(url: string): string | null {
  try {
    const u = new URL(url)
    if (u.protocol !== 'http:' && u.protocol !== 'https:') return null
    if (/\.pdf$/i.test(u.pathname)) return null // the PDF viewer has its own zoom
    return u.hostname.toLowerCase() || null
  } catch {
    return null
  }
}

/** Most hosts whose zoom is remembered (the oldest are forgotten first). */
export const SITE_ZOOM_MAX = 500
/** Zoom levels Chromium accepts: factors 0.25 to 5 are about -7.6 to 8.8 (factor = 1.2 ** level). */
const ZOOM_LEVEL_MIN = -8
const ZOOM_LEVEL_MAX = 9

/** Zoom levels the user chose per host, oldest first (object order). */
export type SiteZoom = Record<string, number>

/** Zoom memory read from disk: valid hosts and levels only, at most `SITE_ZOOM_MAX` (the last ones). */
export function cleanSiteZoom(raw: unknown): SiteZoom {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) return {}
  const entries = Object.entries(raw as Record<string, unknown>).filter(
    (e): e is [string, number] =>
      e[0].length > 0 && e[0].length <= 253 && typeof e[1] === 'number' && Number.isFinite(e[1]) && e[1] >= ZOOM_LEVEL_MIN && e[1] <= ZOOM_LEVEL_MAX
  )
  return Object.fromEntries(entries.slice(-SITE_ZOOM_MAX))
}

/** Remembers a host's zoom level as the most recent one, within the bound. Mutates and returns `map`. */
export function rememberSiteZoom(map: SiteZoom, host: string, level: number): SiteZoom {
  delete map[host]
  map[host] = level
  const extra = Object.keys(map).length - SITE_ZOOM_MAX
  if (extra > 0) for (const k of Object.keys(map).slice(0, extra)) delete map[k]
  return map
}

/**
 * Zoom factor to apply to a page that just committed (or when the default changes), or null to leave it alone:
 * no default chosen yet, not a web page, a site whose zoom the user chose, or already at the default.
 */
export function defaultZoomFactor(opts: { defaultPercent: unknown; url: string; userHosts: ReadonlySet<string>; currentFactor: number }): number | null {
  if (!isZoomPercent(opts.defaultPercent)) return null
  const host = zoomHost(opts.url)
  if (!host || opts.userHosts.has(host)) return null
  const target = opts.defaultPercent / 100
  return Math.abs(opts.currentFactor - target) < 0.005 ? null : target
}

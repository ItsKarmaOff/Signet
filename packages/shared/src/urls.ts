// Addresses and search: pure functions (no Electron or Node), shared by desktop and mobile.
const KNOWN_SCHEME = /^(https?|file|ftp|about|data|view-source|devtools):/i
const OTHER_SCHEME = /^[a-z][a-z0-9+.-]*:\/\//i
const LOCAL_HOST = /^(localhost|(\d{1,3}\.){3}\d{1,3})(:\d+)?([/?#]\S*)?$/i
const DOMAIN = /^([a-z0-9-]+\.)+[a-z]{2,}(:\d+)?([/?#]\S*)?$/i
// Short name with port (`nas:8080`): a local network host, never a search.
const HOST_PORT = /^[a-z0-9-]+:\d{1,5}([/?#]\S*)?$/i

/** Address of the app's interface (`signetui:`), reserved for its own views: never typed nor loaded as is in a tab. */
export function isUiAddress(url: string): boolean {
  // eslint-disable-next-line no-control-regex -- control characters are intended
  return /^[\s\u0000-\u001f]*signetui:/i.test(url)
}

/** Turns an input into a URL, or returns null if it is rather a search. */
export function toUrl(input: string): string | null {
  const text = input.trim()
  if (!text || /\s/.test(text)) return null
  const internal = /^signet:(?:\/\/)?(\S*)$/i.exec(text)
  if (internal) return `signet://${internal[1]}`
  // Never javascript: (nor with leading spaces or control characters): it is a search.
  // eslint-disable-next-line no-control-regex -- control characters are intended
  if (/^[\s\u0000-\u001f]*javascript:/i.test(input)) return null
  // Never the interface address itself (signetui:): it does not load in a tab.
  if (isUiAddress(text)) return null
  if (KNOWN_SCHEME.test(text) || OTHER_SCHEME.test(text)) return text
  if (LOCAL_HOST.test(text) || HOST_PORT.test(text)) return `http://${text}`
  if (DOMAIN.test(text)) return `https://${text}`
  return null
}

/** Network/TLS errors occurring before any response: retry over http (except DNS, -105). */
export function isHttpsFallbackError(code: number): boolean {
  if (code <= -200 && code >= -299) return true // ERR_CERT_*
  return [-100, -101, -102, -107, -109, -113, -118, -7, -324].includes(code)
}

/** Input without scheme that `toUrl` promoted to https:// (so eligible for the http fallback). */
export function isUpgradedInput(input: string, url: string | null): boolean {
  return !!url && url.startsWith('https://') && !/^https:/i.test(input.trim())
}

/** Fallback http URL for an https:// address that just failed, if it is indeed the typed page (not a redirect). */
export function httpFallback(pending: string, failedUrl: string): string | null {
  try {
    const a = new URL(pending)
    const b = new URL(failedUrl)
    if (a.protocol !== 'https:' || b.protocol !== 'https:' || a.host !== b.host || a.pathname !== b.pathname) return null
    a.protocol = 'http:'
    return a.href
  } catch {
    return null
  }
}

/** Language of results pages (`hl`), taken from the first site language (Settings › General). */
let searchLanguage = ''

/**
 * Google picks the language of its pages from the IP address (in Korea: Korean), not from the announced languages:
 * like Chrome, the search specifies the wanted language. `zh` and `pt` keep their region (zh-CN, zh-TW, pt-BR…).
 */
/** Preferred language as is (`fr-FR`), for search suggestions. */
let searchLocale = ''

/** `kl` parameter of DuckDuckGo (`fr-FR` → `fr-fr`, `en-GB` → `uk-en`); empty without region. */
export function suggestRegion(): string {
  const m = /^([a-z]{2,3})-([a-z]{2})$/i.exec(searchLocale)
  if (!m) return ''
  const region = m[2].toLowerCase() === 'gb' ? 'uk' : m[2].toLowerCase()
  return region === m[1].toLowerCase() ? region + '-' + region : `${region}-${m[1].toLowerCase()}`
}

export function setSearchLanguage(code: string | undefined): void {
  const c = (code ?? '').trim()
  searchLocale = c
  searchLanguage = /^(zh|pt)-/i.test(c) ? c : c.split('-')[0].toLowerCase()
}

export function searchUrl(query: string): string {
  const hl = searchLanguage ? `&hl=${encodeURIComponent(searchLanguage)}` : ''
  return `https://www.google.com/search?q=${encodeURIComponent(query)}${hl}`
}

export { siteOf } from './site'

/** Lowercase without accents, to compare labels. */
export function fold(text: string): string {
  return text.normalize('NFD').replace(/\p{M}/gu, '').toLowerCase()
}

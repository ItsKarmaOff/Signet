// Basic mobile content blocking: common ad and tracker domains, and a stylesheet that hides ad containers.
// Limit: a WebView can only filter navigations (page and frames), not subresources (images, scripts, XHR).

/** Blocked domains (the domain and all its subdomains). Deliberately short list: the most widespread ad networks and trackers. */
export const BLOCKED_DOMAINS: readonly string[] = [
  'doubleclick.net',
  'googlesyndication.com',
  'googleadservices.com',
  'google-analytics.com',
  'googletagmanager.com',
  'googletagservices.com',
  'adservice.google.com',
  'adnxs.com',
  'adsrvr.org',
  'advertising.com',
  'amazon-adsystem.com',
  'criteo.com',
  'criteo.net',
  'outbrain.com',
  'taboola.com',
  'rubiconproject.com',
  'pubmatic.com',
  'openx.net',
  'casalemedia.com',
  'moatads.com',
  'scorecardresearch.com',
  'quantserve.com',
  'adform.net',
  'smartadserver.com',
  'yieldmo.com',
  'media.net',
  'bidswitch.net',
  'hotjar.com',
  'mixpanel.com',
  'segment.io',
  'branch.io',
  'facebook.net',
  'connect.facebook.net',
  'ads-twitter.com',
  'analytics.twitter.com',
  'ads.linkedin.com',
  'ads.pinterest.com',
  'adsymptotic.com',
  'zedo.com',
  'popads.net',
  'popcash.net',
  'propellerads.com',
  'exoclick.com',
  'trafficjunky.net',
  'revcontent.com',
  'mgid.com'
]

/** Host name of an http(s) address, lowercase and without trailing dot; `null` for everything else (about:, data:, invalid address). */
export function hostOf(url: string): string | null {
  const m = /^https?:\/\/([^/?#]*)/i.exec(url.trim())
  if (!m) return null
  const host = m[1]
    .slice(m[1].lastIndexOf('@') + 1)
    .replace(/:\d*$/, '')
    .toLowerCase()
    .replace(/\.$/, '')
  return host || null
}

/** True if the address points to one of `domains` or one of their subdomains (`a.b.doubleclick.net`, not `notdoubleclick.net`). */
export function isBlockedUrl(url: string, domains: readonly string[] = BLOCKED_DOMAINS): boolean {
  const host = hostOf(url)
  if (!host) return false
  return domains.some((d) => host === d || host.endsWith('.' + d))
}

/** Selectors of the most common ad containers. */
export const COSMETIC_SELECTORS: readonly string[] = [
  'ins.adsbygoogle',
  '.adsbygoogle',
  '[id^="google_ads_"]',
  '[id^="div-gpt-ad"]',
  '[data-ad-slot]',
  '[data-google-query-id]',
  'iframe[src*="doubleclick.net"]',
  'iframe[src*="googlesyndication.com"]',
  'iframe[id^="google_ads_iframe"]',
  '.ad-container',
  '.ad-banner',
  '.ad-slot',
  '.advert',
  '.advertisement',
  '.sponsored-content',
  '.taboola',
  '#taboola-below-article',
  '.OUTBRAIN',
  '[id^="outbrain"]',
  '.sticky-ad',
  '#sticky-ad'
]

/** CSS that hides ad containers. */
export const COSMETIC_CSS = `${COSMETIC_SELECTORS.join(',')}{display:none!important}`

/** Script to run before the page loads: adds the hiding stylesheet. Idempotent. */
export function cosmeticScript(): string {
  return `(function(){try{if(window.__signetCosmetic)return;window.__signetCosmetic=1;var s=document.createElement('style');s.textContent=${JSON.stringify(COSMETIC_CSS)};(document.head||document.documentElement).appendChild(s);}catch(e){}})();true;`
}

// Identity announced to pages: user agent and client hints, applied through the debugging protocol along with the languages.
import { app, type WebContents } from 'electron'

/**
 * Reduced version, like Chrome and CEF since the user-agent reduction: `Chrome/156.0.0.0`. Electron, on the other hand,
 * announces the full version (`Chrome/156.0.8078.48`), which no Chrome sends anymore.
 */
export function reducedUserAgent(ua: string): string {
  return ua.replace(/\s(Electron|signet)\/\S+/gi, '').replace(/Chrome\/(\d+)\.[\d.]+/, 'Chrome/$1.0.0.0')
}

/**
 * Client hints computed by Chromium itself (GREASE brand specific to each version, macOS version, architecture),
 * read once in an interface page, which is not overridden. Inventing them (`Not=A?Brand`, empty system
 * version) gives a browser that looks like no real Chromium.
 */
let native: object | null = null

const READ_HINTS = `(async () => {
  const d = navigator.userAgentData
  if (!d) return null
  const h = await d.getHighEntropyValues(['architecture', 'bitness', 'model', 'platformVersion', 'fullVersionList', 'uaFullVersion', 'wow64'])
  return { brands: d.brands, fullVersionList: h.fullVersionList, fullVersion: h.uaFullVersion, platform: d.platform, platformVersion: h.platformVersion,
    architecture: h.architecture, model: h.model, mobile: d.mobile, bitness: h.bitness, wow64: h.wow64 }
})()`

/** Reads the native hints in a loaded interface view; true if they are known (only once). */
export async function captureNativeHints(wc: WebContents): Promise<boolean> {
  if (native) return false
  try {
    const hints = (await wc.executeJavaScript(READ_HINTS)) as { brands?: unknown[] } | null
    if (!hints || !Array.isArray(hints.brands) || hints.brands.length === 0) return false
    native = hints
    return true
  } catch {
    return false
  }
}

/**
 * Default parameters of `Emulation.setUserAgentOverride`: the app's agent and the languages, with the native hints.
 * An override without hints erases them (`Sec-CH-UA` missing, empty `navigator.userAgentData.brands`): before they are
 * read, views therefore receive the agent alone, then the full identity as soon as they are known.
 */
export function defaultIdentity(acceptLanguage: string): object {
  return { userAgent: app.userAgentFallback, acceptLanguage, ...(native && { userAgentMetadata: native }) }
}

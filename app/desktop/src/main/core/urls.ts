import { join } from 'node:path'
import type { WebContents } from 'electron'
import { DEFAULT_SEARCH_ENGINE, parseSuggestions } from '@signet/shared/mobile/search'
import {
  cleanEngineConfig,
  expandTemplate,
  keywordHint,
  keywordInfos,
  parseKeyword,
  resolveEngines,
  type EngineConfig,
  type EngineSpec,
  type KeywordInfo,
  type KeywordMatch
} from '@signet/shared/searchengines'
import { searchUrl, suggestRegion } from '@signet/shared/urls'

// The pure part (toUrl, searchUrl, fold, siteOf…) lives in @signet/shared; it is re-exported for main-process code.
export * from '@signet/shared/urls'

/** Chosen search engine id (Settings › General); Google until something is set. */
let searchEngineId: string = DEFAULT_SEARCH_ENGINE
let engineConfig: EngineConfig = {}
let engineLocale = 'en'

/** Changes the default engine (at startup and on each setting change); an unknown or disabled one falls back to Google. */
export function setSearchEngine(id: unknown): void {
  searchEngineId = typeof id === 'string' ? id : DEFAULT_SEARCH_ENGINE
}

/** Custom engines and keyword overrides (already cleaned by `cleanSettings`). */
export function setSearchEngines(config: EngineConfig): void {
  engineConfig = cleanEngineConfig(config)
}

/** Interface language: the Wikipedia preset follows it. */
export function setSearchLocale(locale: string): void {
  engineLocale = locale
}

/** Every engine with its current keyword. */
export function searchEngineList(): EngineSpec[] {
  return resolveEngines(engineConfig, engineLocale)
}

/** The default engine; Google if the chosen one is gone or turned off. */
function defaultEngine(): EngineSpec {
  const all = searchEngineList()
  return all.find((e) => e.id === searchEngineId && e.enabled) ?? all.find((e) => e.id === DEFAULT_SEARCH_ENGINE)!
}

/** URL of the search for `query` at `engine`; Google keeps the page language (`hl`). */
export function searchUrlAt(engine: EngineSpec, query: string): string {
  return engine.id === 'google' ? searchUrl(query) : expandTemplate(engine.template, query)
}

/** URL of the search for `query` at the default engine. */
export function engineSearchUrl(query: string): string {
  return searchUrlAt(defaultEngine(), query)
}

/** Name of the default engine ("Google", "DuckDuckGo"…), for labels. */
export function engineName(): string {
  return defaultEngine().name
}

/** `keyword query` typed in the bar: the engine and its results URL (query may be empty). */
export function keywordSearch(input: string): (KeywordMatch & { url: string }) | null {
  const m = parseKeyword(input, searchEngineList())
  return m && { ...m, url: searchUrlAt(m.engine, m.query) }
}

/** Engine whose keyword is exactly `input`, for the "Press Tab" hint. */
export function keywordHintFor(input: string): EngineSpec | null {
  return keywordHint(input, searchEngineList())
}

/** Keywords offered to the command bar. */
export function keywordList(): KeywordInfo[] {
  return keywordInfos(searchEngineList())
}

/** Suggestions request of `engine` (default engine if omitted), or null if it has no suggestions address. DuckDuckGo receives the `kl` region. */
export function engineSuggestions(query: string, engine: EngineSpec = defaultEngine()): { id: string; url: string } | null {
  if (!engine.suggest) return null
  const kl = engine.id === 'duckduckgo' ? suggestRegion() : ''
  return { id: engine.id, url: expandTemplate(engine.suggest, query) + (kl ? `&kl=${kl}` : '') }
}

export { parseSuggestions }

/** Path of the preload shared by all interface views. */
export const preloadPath = join(__dirname, '../preload/index.js')

/** Page script for drag-to-navigate (injected into tabs). */
export const swipePath = join(__dirname, '../preload/swipe.js')

/** Page script that sets `navigator.globalPrivacyControl` (registered only while Global Privacy Control is on; all frames). */
export const gpcPath = join(__dirname, '../preload/gpc.js')

/** Password page script: on-demand filling and login detection (main frame only). */
export const passwordsPath = join(__dirname, '../preload/passwords.js')

/** Page script that fills `window.chrome` like Chrome (without it, Google refuses sign-in; all frames). */
export const identityPath = join(__dirname, '../preload/identity.js')

/** Page script that intercepts WebAuthn requests (passkeys; Linux, or on demand in development). */
export const webauthnPath = join(__dirname, '../preload/webauthn.js')

/** Page script for notes and Easels (active only on their interface route). */
export const notesPath = join(__dirname, '../preload/notes.js')

/** Preload for extension contexts (service workers and pages): adds the chrome.* APIs Electron lacks (proxy, privacy, alarms…). */
export const extensionApisPath = join(__dirname, '../preload/extension-apis.js')

/** Loads the React interface into a webContents, on the given route ('', '#overlay', '#little'). */
export function loadRoute(wc: WebContents, hash: string): void {
  const dev = process.env.ELECTRON_RENDERER_URL
  const done = dev ? wc.loadURL(dev + hash) : wc.loadURL('signetui://app/index.html' + hash)
  done.catch(() => {})
}

/** Interface comparison target, computed on first call (the environment is then fixed). */
let appTarget: { dev: { origin: string; pathname: string } | null; pathname: string } | null = null

function appTargetOf(): NonNullable<typeof appTarget> {
  if (!appTarget) {
    const dev = process.env.ELECTRON_RENDERER_URL
    const d = dev ? new URL(dev) : null
    appTarget = {
      dev: d ? { origin: d.origin, pathname: d.pathname } : null,
      pathname: '/index.html'
    }
  }
  return appTarget
}

/** True if the URL is the app interface's (excluding fragment): never a web page. */
export function isAppUrl(url: string | undefined | null): boolean {
  if (!url) return false
  try {
    const u = new URL(url)
    const t = appTargetOf()
    if (t.dev) return u.origin === t.dev.origin && u.pathname === t.dev.pathname
    return u.protocol === 'signetui:' && u.host === 'app' && u.pathname === t.pathname
  } catch {
    return false
  }
}

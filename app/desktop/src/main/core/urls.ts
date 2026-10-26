import { join } from 'node:path'
import type { WebContents } from 'electron'
import { DEFAULT_SEARCH_ENGINE, isSearchEngineId, searchEngine, searchUrlFor, suggestionsUrlFor, type SearchEngineId } from '@signet/shared/mobile/search'
import { searchUrl, suggestRegion } from '@signet/shared/urls'

// The pure part (toUrl, searchUrl, fold, siteOf…) lives in @signet/shared; it is re-exported for main-process code.
export * from '@signet/shared/urls'

/** Chosen search engine (Settings › General); Google until something is set. */
let searchEngineId: SearchEngineId = DEFAULT_SEARCH_ENGINE

/** Changes the engine (at startup and on each setting change); an unknown value falls back to Google. */
export function setSearchEngine(id: unknown): void {
  searchEngineId = isSearchEngineId(id) ? id : DEFAULT_SEARCH_ENGINE
}

/** URL of the search for `query` at the chosen engine; Google keeps the page language (`hl`). */
export function engineSearchUrl(query: string): string {
  return searchEngineId === 'google' ? searchUrl(query) : searchUrlFor(searchEngineId, query)
}

/** Name of the chosen engine ("Google", "DuckDuckGo"…), for labels. */
export function engineName(): string {
  return searchEngine(searchEngineId).name
}

/** Engine whose suggestions are queried, with the query URL (DuckDuckGo receives the `kl` region). */
export function engineSuggestions(query: string): { id: SearchEngineId; url: string } {
  const kl = searchEngineId === 'duckduckgo' ? suggestRegion() : ''
  return { id: searchEngineId, url: suggestionsUrlFor(searchEngineId, query) + (kl ? `&kl=${kl}` : '') }
}

/** Path of the preload shared by all interface views. */
export const preloadPath = join(__dirname, '../preload/index.js')

/** Page script for drag-to-navigate (injected into tabs). */
export const swipePath = join(__dirname, '../preload/swipe.js')

/** Password page script: on-demand filling and login detection (all frames). */
export const passwordsPath = join(__dirname, '../preload/passwords.js')

/** Page script that fills `window.chrome` like Chrome (without it, Google refuses sign-in; all frames). */
export const identityPath = join(__dirname, '../preload/identity.js')

/** Page script for notes and Easels (active only on their interface route). */
export const notesPath = join(__dirname, '../preload/notes.js')

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

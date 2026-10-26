// Mobile app settings: type, default values and sanitizing (tolerant: any malformed value falls back to the default).
import type { Theme, UiLanguage } from '../types'
import { toUrl } from '../urls'
import { DEFAULT_SEARCH_ENGINE, isSearchEngineId, type SearchEngineId } from './search'

export type HomepageMode = 'start' | 'blank' | 'custom'
export type NewTabPosition = 'start' | 'end'

/** App icons offered (the same as on desktop). `aurore` is the default icon. */
export const APP_ICON_IDS = ['aurore', 'ocean', 'corail', 'menthe', 'nuit', 'papier', 'graphite', 'or'] as const
export type AppIconId = (typeof APP_ICON_IDS)[number]
export const DEFAULT_APP_ICON: AppIconId = 'aurore'
export const isAppIconId = (v: unknown): v is AppIconId => APP_ICON_IDS.includes(v as AppIconId)

export interface MobileSettings {
  searchEngine: SearchEngineId
  theme: Theme
  /** `system` follows the device language. */
  language: UiLanguage
  /** Basic content blocking (common ads and trackers). */
  contentBlocking: boolean
  /** Lock private tabs behind Face ID / Touch ID / device passcode. */
  privateLock: boolean
  /** Page for new tabs: app home, blank page or chosen address (`homepageUrl`). */
  homepage: HomepageMode
  homepageUrl: string
  /** Open links that ask for a new window (`target=_blank`) in a new tab. */
  openLinksInNewTab: boolean
  /** Search engine suggestions while typing (the input is sent to it; never in private browsing). */
  searchSuggestions: boolean
  /** Offer to save passwords typed in pages. */
  offerToSavePasswords: boolean
  /** Retours haptiques. */
  haptics: boolean
  /** Master switch of the link with computers ("Sync"). Off: no link connects, the links stay. */
  linkEnabled: boolean
  /** App icon on the home screen (iOS / Android, installed app only). */
  appIcon: AppIconId
}

export const DEFAULT_MOBILE_SETTINGS: MobileSettings = {
  searchEngine: DEFAULT_SEARCH_ENGINE,
  theme: 'system',
  language: 'system',
  contentBlocking: true,
  privateLock: true,
  homepage: 'start',
  homepageUrl: '',
  openLinksInNewTab: true,
  searchSuggestions: true,
  offerToSavePasswords: true,
  haptics: true,
  linkEnabled: true,
  appIcon: DEFAULT_APP_ICON
}

const oneOf = <T extends string>(v: unknown, allowed: readonly T[], fallback: T): T => (allowed.includes(v as T) ? (v as T) : fallback)
const bool = (v: unknown, fallback: boolean) => (typeof v === 'boolean' ? v : fallback)

/** Valid home page address (http or https), otherwise an empty string. */
export function cleanHomepageUrl(v: unknown): string {
  if (typeof v !== 'string') return ''
  const text = v.trim()
  return /^https?:\/\/\S+$/i.test(text) ? text.slice(0, 2048) : ''
}

/** Settings read from storage or a partial update: unknown fields ignored, invalid values replaced by the default. */
export function sanitizeSettings(raw: unknown, base: MobileSettings = DEFAULT_MOBILE_SETTINGS): MobileSettings {
  const r = (raw && typeof raw === 'object' ? raw : {}) as Record<string, unknown>
  const homepageUrl = 'homepageUrl' in r ? cleanHomepageUrl(r.homepageUrl) : base.homepageUrl
  const homepage = oneOf<HomepageMode>(r.homepage ?? base.homepage, ['start', 'blank', 'custom'], base.homepage)
  return {
    searchEngine: isSearchEngineId(r.searchEngine) ? r.searchEngine : base.searchEngine,
    theme: oneOf<Theme>(r.theme ?? base.theme, ['system', 'light', 'dark'], base.theme),
    language: oneOf<UiLanguage>(r.language ?? base.language, ['system', 'en', 'fr'], base.language),
    contentBlocking: bool(r.contentBlocking, base.contentBlocking),
    privateLock: bool(r.privateLock, base.privateLock),
    // A "chosen" home page without a valid address makes no sense: fall back to the app's home.
    homepage: homepage === 'custom' && !homepageUrl ? 'start' : homepage,
    homepageUrl,
    openLinksInNewTab: bool(r.openLinksInNewTab, base.openLinksInNewTab),
    searchSuggestions: bool(r.searchSuggestions, base.searchSuggestions),
    offerToSavePasswords: bool(r.offerToSavePasswords, base.offerToSavePasswords),
    haptics: bool(r.haptics, base.haptics),
    linkEnabled: bool(r.linkEnabled, base.linkEnabled),
    appIcon: isAppIconId(r.appIcon) ? r.appIcon : base.appIcon
  }
}

/** Applies a partial update to already valid settings. */
export const patchSettings = (current: MobileSettings, patch: Partial<MobileSettings>): MobileSettings => sanitizeSettings({ ...current, ...patch }, current)

/** URL to open in a new tab: `''` = the app's home screen (or blank page), otherwise the chosen address. */
export function newTabUrl(s: MobileSettings): string {
  return s.homepage === 'custom' ? s.homepageUrl : ''
}

/** Home page input -> valid http(s) address (`example.com` becomes `https://example.com`), or `null`. */
export function parseHomepageInput(input: string): string | null {
  const url = toUrl(input)
  if (!url || !/^https?:\/\/[^\s/]+\.?[^\s/]*/i.test(url)) return null
  try {
    new URL(url)
  } catch {
    return null
  }
  return cleanHomepageUrl(url) || null
}

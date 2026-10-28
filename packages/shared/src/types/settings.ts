import type { UiLanguage } from '../i18n'
import type { DevAlias, DevResponsive, DevToolsPrefs } from './devtools'
import type { PasswordManager } from './passwords'
import type { SpaceRoute } from './spaces'
import type { SearchEngineId } from '../mobile/search'

export interface Settings {
  /** Hours of inactivity before a day tab is archived. */
  archiveAfterHours: number
  /** Open in Peek the links that leave the domain of a pinned or favorite tab. */
  peekExternalLinks: boolean
  /** Block ads and trackers. */
  adblock: boolean
  /** Id of the app icon variant (see `APP_ICONS`). */
  appIcon: string
  /** Compact mode: sidebar and toolbar hidden, revealed when hovering the edges. */
  compactMode: boolean
  /** Automatic picture-in-picture when leaving a tab whose video is playing. */
  autoPip: boolean
  /** Routing rules sending new tabs to a space. */
  routes: SpaceRoute[]
  /** Preferred languages announced to sites, in order of preference (BCP 47 codes: `fr-FR`, `en`…). */
  languages: string[]
  /** Interface language; missing = `system` (follows macOS). */
  uiLanguage?: UiLanguage
  /** Engine for searches typed in the command bar (see `SEARCH_ENGINES`). Missing = `google`. */
  searchEngine?: SearchEngineId
  /** Downloads folder (absolute path). Missing = the system's Downloads folder. */
  downloadDir?: string
  /** Ask where to save each download. Missing = no (saved directly to `downloadDir`). */
  askDownloadPath?: boolean
  /** First-launch introduction finished or skipped. Missing on an existing state = already seen (only a fresh state shows it). */
  onboarded?: boolean
  /** Two-finger swipe on a page to go to the previous or next page. Missing = enabled. */
  swipeNavigation?: boolean
  /** Minutes out of view before a tab is put to sleep (view destroyed, reloaded on return); 0 = never. Missing = 30. */
  tabSleepMinutes?: number
  /** Position of new tabs among the day tabs: at the top (`top`, default) or the bottom of the list. */
  newTabPosition?: 'top' | 'bottom'
  /** Tab layout: sidebar (`vertical`, default) or a row at the top of the window (`horizontal`). */
  tabLayout?: 'vertical' | 'horizontal'
  /** Order of extension buttons in the bar (identifiers); extensions missing from the list go to the end. */
  extensionOrder?: string[]
  /**
   * Pinned extensions: only these have a button in the toolbar. The others stay active and open from
   * the Extensions button (puzzle piece). Missing = none; an installed or imported extension is never pinned automatically.
   */
  pinnedExtensions?: string[]
  /** Ask for confirmation before quitting (hold ⌘Q or press twice). Missing = enabled. */
  confirmQuit?: boolean
  /** Bookmarks bar below the address bar (⌘⇧B). Missing = shown. */
  showBookmarksBar?: boolean
  /** Theme of the interface and pages: follows macOS (`system`, default), light or dark. */
  theme?: Theme
  /** Amount of the space tint on backgrounds. Missing = `normal`. */
  tint?: TintStrength
  /** Side of the sidebar (vertical layout). Missing = `left`. */
  sidebarSide?: 'left' | 'right'
  /** Height of the sidebar's tab rows. Missing = `normal`. */
  density?: Density
  /** Interface text size. Missing = `normal`. */
  textSize?: TextSize
  /** Translucent (blur) effect on backgrounds. Missing = enabled. */
  transparency?: boolean
  /** Spell checking in page fields. Missing = enabled. */
  spellcheck?: boolean
  /** Reader mode presentation. Missing = default values (`DEFAULT_READER`). */
  reader?: ReaderPrefs
  /** Gestionnaire de mots de passe. Absent = `signet` (trousseau de Signet). */
  passwordManager?: PasswordManager
  /** Offer to save typed passwords (Signet's password vault). Missing = enabled. */
  offerSavePasswords?: boolean
  /** Origins for which saving is never offered. */
  neverSavePasswords?: string[]
  /** Modified shortcuts (see app/desktop/src/shared/shortcuts.ts): chosen accelerator, or null for none. Missing key = default. */
  shortcuts?: Record<string, string | null>
  /** Tab preview on hover (sidebar, tab row). Missing = enabled. */
  tabPreview?: boolean
  /** Developer mode: port and console error counter on local pages. Missing = off. */
  devMode?: boolean
  /** Developer mode: local pages kept out of history (and so of suggestions). Missing = recorded. */
  devKeepOutOfHistory?: boolean
  /** Local names: `mysite` typed in the command bar opens its local target; the address bar shows the name. */
  devAliases?: DevAlias[]
  /** Folders of unpacked extensions, reloaded at each launch. */
  unpackedExtensions?: string[]
  /** JSON responses shown as a tree (JSON viewer). Missing = enabled. */
  jsonViewer?: boolean
  /** Look for new versions automatically (packaged app). Missing = enabled. */
  checkUpdates?: boolean
  /** Last responsive-mode device, restored on next open. */
  responsive?: DevResponsive
  /** Location and sizes of the developer tools. Missing = on the right, original sizes. */
  devtools?: DevToolsPrefs
  /** Graphics acceleration (GPU) when available. Applied at the next launch. Missing = enabled. */
  hardwareAcceleration?: boolean
  /** Secure DNS (DNS over HTTPS). Missing = `automatic`. */
  secureDns?: SecureDnsMode
  /** DNS over HTTPS server template, used in `secure` mode. */
  secureDnsServer?: string
}

/**
 * Secure DNS: `automatic` upgrades the system's DNS provider to DNS over HTTPS when it supports it, `secure`
 * only uses the chosen server, `off` never uses DNS over HTTPS.
 */
export type SecureDnsMode = 'off' | 'automatic' | 'secure'

/** Reader mode presentation, set from its bar and remembered for all pages. */
export interface ReaderPrefs {
  font: 'sans' | 'serif'
  /** Text size in px (14 to 28). */
  size: number
  theme: 'auto' | 'light' | 'sepia' | 'dark'
  width: 'narrow' | 'normal' | 'wide'
}

export const DEFAULT_READER: ReaderPrefs = { font: 'serif', size: 19, theme: 'auto', width: 'normal' }

export type Theme = 'system' | 'light' | 'dark'

export type TintStrength = 'subtle' | 'normal' | 'strong'

export type Density = 'compact' | 'normal' | 'roomy'

export type TextSize = 'small' | 'normal' | 'large'

/** Appearance settings applied to all interface views (`data-*` attributes on `<html>`). */
export interface Appearance {
  tint: TintStrength
  density: Density
  text: TextSize
  glass: boolean
}

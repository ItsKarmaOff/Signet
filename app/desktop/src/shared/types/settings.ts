import type { ImportKind } from './data'
import type { ExtensionInfo } from './extensions'
import type { Boost } from './mods'
import type { Settings } from '@signet/shared/types'

/** App icon variants. The files are in resources/icons/<id>.png; names come from i18n (`appIcon.<id>`). */
export const APP_ICONS = [
  { id: 'aurore' },
  { id: 'ocean' },
  { id: 'corail' },
  { id: 'menthe' },
  { id: 'nuit' },
  { id: 'papier' },
  { id: 'graphite' },
  { id: 'or' }
] as const

export type BrowsingDataKind = 'history' | 'archive' | 'downloads' | 'cache' | 'cookies' | 'permissions'

/** Everything the Settings window shows. Pushed on `settings:info` to the `#settings` view. */
export interface SettingsInfo {
  settings: Settings
  version: string
  /** False in development: some settings (default browser) only work when packaged. */
  packaged: boolean
  defaultBrowser: boolean
  appIcons: { id: string; name: string; /** 128 px preview as a data URL. */ preview: string }[]
  /** Import sources on this system; `available` = present on disk, `kinds` = data found. */
  importSources: { id: string; title: string; available: boolean; kinds: ImportKind[] }[]
  boosts: Boost[]
  extensions: ExtensionInfo[]
  /** Entry counts, for the "Clear data" labels. */
  counts: { history: number; archive: number; downloads: number; permissions: number }
  hue: number
  /** Folder where downloads go (the Settings one, or the system one if unset or no longer there). */
  downloadDir: string
}

// Interface translation with i18next. JSON catalogs in packages/shared/src/locales/<language>/<namespace>.json:
// `main` for the main process (menus, dialogs, command bar), `ui` for the React interface, `mobile` for the React Native app.
// English is the reference; a key missing in French falls back to English.
// Catalogs are not imported here: each process bundles only its own (main: `main`, interface: `ui`, mobile: `mobile`).
import i18next, { type i18n, type Resource } from 'i18next'

export type Locale = 'en' | 'fr'
/** User choice: `system` follows the macOS language. */
export type UiLanguage = 'system' | Locale

export const LOCALES: readonly { id: Locale; name: string }[] = [
  { id: 'en', name: 'English' },
  { id: 'fr', name: 'Français' }
]

/** First supported system language, otherwise English. */
export function resolveLocale(pref: UiLanguage | undefined, systemLanguages: readonly string[]): Locale {
  if (pref && pref !== 'system') return pref
  for (const tag of systemLanguages) {
    const base = tag.toLowerCase().split(/[-_]/)[0]
    if (LOCALES.some((l) => l.id === base)) return base as Locale
  }
  return 'en'
}

/** True on macOS (main process: `process.platform`; interface: the browser). */
const platform = (globalThis as { process?: { platform?: string } }).process?.platform
const navigator = (globalThis as { navigator?: { platform: string } }).navigator
export const IS_MAC = platform ? platform === 'darwin' : !!navigator && /Mac/i.test(navigator.platform)

/** i18next context depending on the system: absent on Mac (base text), `windows` or `linux` (keys `name_windows`, `name_linux`). */
export const OS_CONTEXT: 'windows' | 'linux' | undefined = IS_MAC
  ? undefined
  : (platform ? platform === 'win32' : !!navigator && /Win/i.test(navigator.platform))
    ? 'windows'
    : 'linux'

const MODIFIERS: Record<string, string> = { '⌃': 'Ctrl', '⌥': 'Alt', '⇧': 'Shift', '⌘': 'Ctrl' }

/**
 * Shortcuts written Mac-style (⇧⌘T, ⌥⌘B, ⌘,) -> Windows and Linux (Ctrl+Shift+T, Ctrl+Alt+B, Ctrl+,):
 * ⌘ and ⌃ become Ctrl, ⌥ Alt, ⇧ Shift, and the key follows. Unchanged on Mac.
 * Two Mac spellings that give the same thing elsewhere ("⌃⇧S or ⌘⇧S") become just one.
 */
export function platformKeys(text: string, mac = IS_MAC): string {
  if (mac || !/[⌃⌥⇧⌘⇥]/.test(text)) return text
  return text
    .replace(/([⌃⌥⇧⌘]+)(\S)/g, (_, mods: string, key: string) => {
      const set = new Set([...mods].map((g) => MODIFIERS[g]))
      return [...['Ctrl', 'Alt', 'Shift'].filter((m) => set.has(m)), key].join('+')
    })
    .replace(/(Ctrl\+[^\s)]+) (?:ou|or) \1/g, '$1')
    .replace(/⇥/g, 'Tab')
}

/** Creates an independent i18next instance (one per process or view). Shortcuts in texts follow the system. */
export function createI18n(locale: Locale, defaultNS: 'main' | 'ui' | 'mobile', resources: Resource): i18n {
  const instance = i18next.createInstance()
  instance.use({ type: 'postProcessor', name: 'platformKeys', process: (value: string) => platformKeys(value) })
  void instance.init({
    postProcess: ['platformKeys'],
    resources,
    lng: locale,
    fallbackLng: 'en',
    ns: [defaultNS],
    defaultNS,
    interpolation: { escapeValue: false },
    initAsync: false,
    returnNull: false
  })
  return instance
}

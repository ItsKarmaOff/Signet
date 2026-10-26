// App translations: `mobile` catalogs from the shared package, language from settings (`system` = device language).
// This module does not depend on the store (the store uses it): `state/persistence.ts` calls `applyLanguage`
// at startup and on every setting change. In a component: `const t = useT()`.
import { useSyncExternalStore } from 'react'
import { getLocales } from 'expo-localization'
import { createI18n, resolveLocale, type Locale, type UiLanguage } from '@signet/shared/i18n'
import en from '@signet/shared/locales/en/mobile.json'
import fr from '@signet/shared/locales/fr/mobile.json'

const systemTags = () => getLocales().map((l) => l.languageTag)

export const i18n = createI18n(resolveLocale('system', systemTags()), 'mobile', { en: { mobile: en }, fr: { mobile: fr } })

let version = 0
const listeners = new Set<() => void>()

/** Switches the app to the chosen language; components using `useT` / `useLocale` re-render. */
export function applyLanguage(pref: UiLanguage): void {
  const next = resolveLocale(pref, systemTags())
  if (next === i18n.language) return
  void i18n.changeLanguage(next)
  version++
  listeners.forEach((l) => l())
}

const subscribe = (l: () => void) => {
  listeners.add(l)
  return () => listeners.delete(l)
}

/** Translates a key from the `mobile` catalog (`browser.back`, `tabs.count` with `{ count }`…). Outside a component: use as is. */
export function t(key: string, options?: Record<string, unknown>): string {
  return i18n.t(key, options)
}

export type TFunction = typeof t

/** Translation hook: returns `t` and re-renders the component when the language changes. */
export function useT(): TFunction {
  useSyncExternalStore(subscribe, () => version)
  return t
}

/** Displayed language (`en` or `fr`), for formatting dates and numbers (`toLocaleDateString(locale)`). */
export function useLocale(): Locale {
  useSyncExternalStore(subscribe, () => version)
  return i18n.language as Locale
}

// Main-process translation: current language, typed `t()`, broadcast to the UI views.
import { app, webContents } from 'electron'
import type { i18n, ParseKeys, TOptions } from 'i18next'
import enMain from '@signet/shared/locales/en/main.json'
import frMain from '@signet/shared/locales/fr/main.json'
import { createI18n, resolveLocale, type Locale, type UiLanguage } from '@signet/shared/i18n'
import { pushUi } from '../ipc/push'

let pref: UiLanguage | undefined
let current: Locale | null = null
let instance: i18n | null = null
const listeners = new Set<(locale: Locale) => void>()

function compute(): Locale {
  return resolveLocale(pref, app.getPreferredSystemLanguages())
}

function ensure(): i18n {
  if (!instance) {
    current = compute()
    instance = createI18n(current, 'main', { en: { main: enMain }, fr: { main: frMain } })
  }
  return instance
}

/** Remembers the saved preference (when the state loads), without notifying. */
export function initLocale(preference: UiLanguage | undefined): void {
  pref = preference
  const next = compute()
  if (instance && next !== current) void instance.changeLanguage(next)
  current = next
  ensure()
}

export function currentLocale(): Locale {
  ensure()
  return current as Locale
}

/** Translated text from the `main` catalog; an unknown key fails at compile time. */
export function t(key: ParseKeys<'main'>, options?: TOptions): string {
  return ensure().t(key, { ...options, ns: 'main' } as never) as unknown as string
}

/** Called when the effective language changes (menu, views, window titles). */
export function onLocaleChange(fn: (locale: Locale) => void): void {
  listeners.add(fn)
}

/** Applies the user's choice; notifies only if the effective language changes. */
export function setUiLanguage(preference: UiLanguage | undefined): void {
  const before = currentLocale()
  pref = preference
  const next = compute()
  if (next === before) return
  current = next
  void ensure().changeLanguage(next)
  for (const fn of listeners) fn(next)
}

/** React UI views (not the tabs' web pages): those that load the app route. */
export function isUiContents(wc: Electron.WebContents): boolean {
  if (wc.isDestroyed()) return false
  const url = wc.getURL()
  const dev = process.env.ELECTRON_RENDERER_URL
  return dev ? url.startsWith(dev) : url.startsWith('signetui://app/index.html')
}

/** Pushes the language to all UI views on the `locale` channel. */
export function broadcastLocale(): void {
  for (const wc of webContents.getAllWebContents()) if (isUiContents(wc)) pushUi(wc, 'locale', currentLocale())
}

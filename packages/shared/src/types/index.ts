// Data model common to all apps (desktop, mobile): tabs, spaces, bookmarks, settings, history.
// No IPC, window or DOM types here (see README).
import type { Locale, UiLanguage } from '../i18n'
export type { Locale, UiLanguage }

export * from './devtools'
export * from './library'
export * from './passwords'
export * from './settings'
export * from './spaces'
export * from './tabs'

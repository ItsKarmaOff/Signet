import { app, type Session, type WebContents } from 'electron'
import { defaultIdentity } from './identity'
import { setSearchLanguage } from '../core/urls'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'

/** Regional language followed by its base language, without duplicates: [en-US, fr-FR] → [en-US, en, fr-FR, fr]. */
export function expandLanguages(codes: string[]): string[] {
  const out: string[] = []
  const add = (c: string) => {
    if (c && !out.includes(c)) out.push(c)
  }
  for (const raw of codes) {
    const code = String(raw).trim().replace(/_/g, '-')
    if (!/^[A-Za-z]{2,3}(-[A-Za-z0-9]{2,8})*$/.test(code)) continue
    add(code)
    add(code.split('-')[0].toLowerCase())
  }
  return out
}

/** Default languages: the system's, or English if the system gives none. */
export function defaultLanguages(): string[] {
  let system: string[] = []
  try {
    system = app.getPreferredSystemLanguages()
  } catch {
    // unavailable: fallback below
  }
  const list = expandLanguages(system)
  return list.length ? list : ['en-US', 'en']
}

/** Valid (non-empty) list or, failing that, the system's. */
export function normalizeLanguages(value: unknown): string[] {
  const list = Array.isArray(value) ? expandOnlyValid(value.filter((v): v is string => typeof v === 'string')) : []
  return list.length ? list : defaultLanguages()
}

function expandOnlyValid(codes: string[]): string[] {
  const out: string[] = []
  for (const c of codes) if (/^[A-Za-z]{2,3}(-[A-Za-z0-9]{2,8})*$/.test(c) && !out.includes(c)) out.push(c)
  return out
}

/** Simple list `fr-FR,fr,en-US,en`: Chromium adds the weights itself (q=0.9, 0.8…, floor 0.1). */
export function acceptLanguage(languages: string[]): string {
  return languages.join(',')
}

/** Applies the user agent and the languages to a session (only place that calls setUserAgent). */
export function applyLanguages(ses: Session, languages: string[]): void {
  ses.setUserAgent(app.userAgentFallback, acceptLanguage(languages))
  setSearchLanguage(languages[0])
  // Spell checker: on Mac, the system's picks the language by itself; elsewhere, we follow the preferred languages.
  if (process.platform !== 'darwin') {
    const available = ses.availableSpellCheckerLanguages
    const pick = (code: string) =>
      available.find((a) => a.toLowerCase() === code.toLowerCase()) ?? available.find((a) => a.split('-')[0] === code.split('-')[0])
    const wanted = [...new Set(languages.map(pick).filter((x): x is string => !!x))].slice(0, 3)
    if (wanted.length) ses.setSpellCheckerLanguages(wanted)
  }
}

/** Enables or disables spell checking in the fields of a session's pages. */
export function applySpellcheck(ses: Session, enabled: boolean): void {
  ses.setSpellCheckerEnabled(enabled)
}

/**
 * Makes a view announce the languages to navigator.language(s): Chromium otherwise derives them from the system language.
 * Goes through the debugging protocol (the header, for its part, is set per session in applyLanguages).
 */
export function emulateLanguages(wc: WebContents, languages: string[]): void {
  if (wc.isDestroyed()) return
  try {
    if (!wc.debugger.isAttached()) wc.debugger.attach('1.3')
    // With Chromium's client hints: without them, the override erases them (see identity.ts).
    void wc.debugger.sendCommand('Emulation.setUserAgentOverride', defaultIdentity(acceptLanguage(languages))).catch(() => {})
  } catch {
    // debugger already taken by another tool: keep the default behavior
  }
}

/** First language, read from state.json before `ready` (the Store does not exist yet). */
export function startupLanguage(): string {
  try {
    const raw = JSON.parse(readFileSync(join(app.getPath('userData'), 'state.json'), 'utf8'))
    startupRaw = raw
    return normalizeLanguages(raw?.settings?.languages)[0]
  } catch {
    return defaultLanguages()[0]
  }
}

let startupRaw: unknown

/** Saved settings read by `startupLanguage` (before `ready`), without consuming them. */
export function startupSettings(): Record<string, unknown> | null {
  const s = (startupRaw as { settings?: unknown } | undefined)?.settings
  return s && typeof s === 'object' ? (s as Record<string, unknown>) : null
}

/** state.json already parsed by `startupLanguage`, to be consumed only once (so the Store avoids a second parse). */
export function takeStartupState(): unknown {
  const raw = startupRaw
  startupRaw = undefined
  return raw
}

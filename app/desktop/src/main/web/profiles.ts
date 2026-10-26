import { app, session, type Session } from 'electron'
import { readFileSync, rmSync, writeFileSync } from 'node:fs'
import { readdir, rm } from 'node:fs/promises'
import { join } from 'node:path'
import type { Space } from '@shared/types'
import type { Browser } from '../browser/browser'
import { applyLanguages, applySpellcheck } from './languages'
import { webauthnInterceptEnabled } from './webauthn'
import { attachInternal } from './internal'
import { attachUiProtocol } from '../browser/uiprotocol'
import { identityPath, notesPath, passwordsPath, swipePath, webauthnPath } from '../core/urls'

let browser: Browser | null = null
const configured = new WeakSet<Session>()

export const partitionOf = (spaceId: string) => `persist:space-${spaceId}`

export const isIsolated = (space: Space | undefined | null) => space?.profile === 'isolated'

/** To call once, when the `Browser` is ready: configures the default session. */
export function initProfiles(b: Browser): void {
  browser = b
  configureSession(session.defaultSession)
  void wipePending(b.store.state.spaces)
}

/*
 * Isolated space deleted: its cookies wait for the end of the undo window. Its id is noted on disk during
 * that time; if the app quits before, the next launch erases its partition. Only the noted spaces are touched.
 */
const pendingFile = () => join(app.getPath('userData'), 'pending-wipes.json')

function readPending(): string[] {
  try {
    const ids: unknown = JSON.parse(readFileSync(pendingFile(), 'utf8'))
    return Array.isArray(ids) ? ids.filter((id): id is string => typeof id === 'string') : []
  } catch {
    return []
  }
}

function writePending(ids: string[]): void {
  try {
    if (ids.length) writeFileSync(pendingFile(), JSON.stringify(ids))
    else rmSync(pendingFile(), { force: true })
  } catch {
    // at worst, the partition stays on disk
  }
}

export function markPendingWipe(spaceId: string): void {
  writePending([...new Set([...readPending(), spaceId])])
}

export function clearPendingWipe(spaceId: string): void {
  writePending(readPending().filter((id) => id !== spaceId))
}

async function wipePending(spaces: Space[]): Promise<void> {
  const ids = readPending()
  if (!ids.length) return
  const alive = new Set(spaces.map((s) => s.id))
  const dir = join(app.getPath('userData'), 'Partitions')
  const names = await readdir(dir).catch(() => [] as string[])
  for (const id of ids) {
    if (alive.has(id)) continue // space came back (backup restored…): keep its data
    const target = `space-${id}`.toLowerCase()
    for (const name of names) if (name.toLowerCase() === target) await rm(join(dir, name), { recursive: true, force: true }).catch(() => {})
  }
  writePending(readPending().filter((id) => !ids.includes(id)))
}

/**
 * A space's session: its own partition if isolated, the default session otherwise
 * (shared space, favorites, unknown space).
 */
export function sessionFor(spaceId: string | null): Session {
  const space = spaceId ? browser?.store.state.spaces.find((s) => s.id === spaceId) : undefined
  const ses = space && isIsolated(space) ? session.fromPartition(partitionOf(space.id)) : session.defaultSession
  configureSession(ses)
  return ses
}

/** `window.chrome` as in Chrome, in all frames: without it, Google refuses the sign-in. */
function registerIdentity(ses: Session): void {
  try {
    ses.registerPreloadScript({ id: 'signet-identity', type: 'frame', filePath: identityPath })
  } catch (e) {
    console.error('Identity: page script unavailable', e)
  }
}

/** Passkey requests answered by Signet (cancelled for now) where Electron would leave the page hanging. */
function registerWebAuthn(ses: Session): void {
  if (!webauthnInterceptEnabled()) return
  try {
    ses.registerPreloadScript({ id: 'signet-webauthn', type: 'frame', filePath: webauthnPath })
  } catch (e) {
    console.error('WebAuthn: page script unavailable', e)
  }
}

/** Settings common to all sessions, applied only once per session. */
export function configureSession(ses: Session): void {
  if (!browser || configured.has(ses)) return
  configured.add(ses)
  applyLanguages(ses, browser.store.state.settings.languages)
  applySpellcheck(ses, browser.store.state.settings.spellcheck !== false)
  browser.permissions.attach(ses)
  browser.adblock.attach(ses)
  browser.downloads.attach(ses)
  browser.devMode.watchSession(ses)
  browser.extensions.attach(ses)
  attachInternal(ses, browser)
  attachUiProtocol(ses)
  registerIdentity(ses)
  registerWebAuthn(ses)
  // Swipe to navigate: the script decides by itself whether to act (a tab's main frame), the main process filters the rest.
  try {
    ses.registerPreloadScript({ id: 'signet-swipe', type: 'frame', filePath: swipePath })
  } catch (e) {
    console.error('Swipe to navigate: script unavailable', e)
  }
  try {
    ses.registerPreloadScript({ id: 'signet-passwords', type: 'frame', filePath: passwordsPath })
  } catch (e) {
    console.error('Mots de passe : script de page indisponible', e)
  }
  // Notes and Easels: not in private windows (nothing is saved there).
  try {
    ses.registerPreloadScript({ id: 'signet-notes', type: 'frame', filePath: notesPath })
  } catch (e) {
    console.error('Notes : script de page indisponible', e)
  }
}

/** Sessions of the open private windows (in memory, one per window). */
const privateSessions = new Set<Session>()

/**
 * A private window's session: like the others (languages, blocker, downloads, internal pages,
 * swipe to navigate), but without extensions and with permissions forgotten at close.
 */
export function configurePrivateSession(ses: Session): void {
  if (!browser || configured.has(ses)) return
  configured.add(ses)
  privateSessions.add(ses)
  applyLanguages(ses, browser.store.state.settings.languages)
  applySpellcheck(ses, browser.store.state.settings.spellcheck !== false)
  browser.permissions.attach(ses, { ephemeral: true })
  browser.adblock.attach(ses)
  browser.downloads.attach(ses, { private: true })
  attachInternal(ses, browser)
  attachUiProtocol(ses)
  registerIdentity(ses)
  registerWebAuthn(ses)
  try {
    ses.registerPreloadScript({ id: 'signet-swipe', type: 'frame', filePath: swipePath })
  } catch (e) {
    console.error('Swipe to navigate: script unavailable', e)
  }
  try {
    ses.registerPreloadScript({ id: 'signet-passwords', type: 'frame', filePath: passwordsPath })
  } catch (e) {
    console.error('Mots de passe : script de page indisponible', e)
  }
}

/** Private window closed: erases everything its session kept in memory and releases it. */
export function releasePrivateSession(ses: Session): void {
  privateSessions.delete(ses)
  browser?.adblock.detach(ses)
  void ses
    .clearStorageData()
    .then(() => ses.clearCache())
    .then(() => ses.clearAuthCache())
    .catch(() => {})
}

/** Reapplies spell checking to all sessions (Settings toggle). */
export function reapplySpellcheck(enabled: boolean): void {
  for (const ses of allSessions()) applySpellcheck(ses, enabled)
}

/** Reapplies the languages to all sessions (list change in Settings). */
export function reapplyLanguages(languages: string[]): void {
  for (const ses of allSessions()) applyLanguages(ses, languages)
}

/**
 * A space's session without configuring it (or loading extensions): to compare or erase.
 * Configuration happens on the first `sessionFor`.
 */
export function peekSession(space: Space | undefined | null): Session {
  return space && isIsolated(space) ? session.fromPartition(partitionOf(space.id)) : session.defaultSession
}

/**
 * Default session, isolated spaces' partitions and open private sessions, without configuring them: those never opened
 * have no extensions loaded (to erase data or reapply a setting, this makes no difference).
 */
export function allSessions(): Session[] {
  const spaces = browser?.store.state.spaces ?? []
  return [session.defaultSession, ...spaces.filter(isIsolated).map((s) => peekSession(s)), ...privateSessions]
}

/** Erases cookies, storage and cache of a space's partition (no effect if it is not isolated). */
export async function clearSpaceData(spaceId: string): Promise<void> {
  const ses = session.fromPartition(partitionOf(spaceId))
  await ses.clearStorageData()
  await ses.clearCache()
}

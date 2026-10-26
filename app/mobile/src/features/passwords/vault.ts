// Password vault: one iOS Keychain / Android Keystore item per login (expo-secure-store),
// accessible when the device is unlocked and never backed up nor migrated to another device.
// Memory: only the index (origin, username, dates). Passwords are only read on demand and never kept,
// neither in the zustand store, nor in AsyncStorage, nor in logs.
import { useSyncExternalStore } from 'react'
import * as SecureStore from 'expo-secure-store'
import {
  addNever,
  credentialsForPage,
  MAX_FIELD,
  newCredentialId,
  normalizeOrigin,
  planImport,
  type Credential,
  type CredentialMeta,
  type ImportRow
} from '@signet/shared/mobile/passwords'

const OPTIONS: SecureStore.SecureStoreOptions = { keychainAccessible: SecureStore.WHEN_UNLOCKED_THIS_DEVICE_ONLY }
const ITEM = 'signet.pw.item.'
const IDS = 'signet.pw.ids'
const NEVER = 'signet.pw.never'
/** Items per list chunk (a Keychain item stays small). */
const CHUNK = 30

let loaded: Promise<void> | null = null
let metas: CredentialMeta[] = []
let never: string[] = []
let queue: Promise<unknown> = Promise.resolve()
const listeners = new Set<() => void>()
const emit = () => listeners.forEach((l) => l())

const meta = ({ password: _password, ...rest }: Credential): CredentialMeta => rest

// One writer at a time: the index (ids) is re-read, modified and rewritten.
const serial = <T>(fn: () => Promise<T>): Promise<T> => {
  const run = queue.then(fn, fn)
  queue = run.catch(() => undefined)
  return run
}

async function readList(base: string): Promise<string[]> {
  const out: string[] = []
  for (let i = 0; i < 1000; i++) {
    const raw = await SecureStore.getItemAsync(`${base}.${i}`, OPTIONS)
    if (raw === null) break
    try {
      const part = JSON.parse(raw) as unknown
      if (Array.isArray(part)) out.push(...part.filter((x): x is string => typeof x === 'string'))
    } catch {
      /* unreadable chunk: ignored */
    }
  }
  return out
}

async function writeList(base: string, list: readonly string[]): Promise<void> {
  const parts = Math.ceil(list.length / CHUNK)
  for (let i = 0; i < parts; i++) await SecureStore.setItemAsync(`${base}.${i}`, JSON.stringify(list.slice(i * CHUNK, (i + 1) * CHUNK)), OPTIONS)
  // Morceaux en trop (liste raccourcie), y compris un premier morceau vide.
  for (let i = parts; i < 1000; i++) {
    if ((await SecureStore.getItemAsync(`${base}.${i}`, OPTIONS)) === null) break
    await SecureStore.deleteItemAsync(`${base}.${i}`, OPTIONS).catch(() => undefined)
  }
}

function parseCredential(raw: string | null): Credential | null {
  if (!raw) return null
  try {
    const v = JSON.parse(raw) as Partial<Credential>
    if (typeof v.id !== 'string' || typeof v.origin !== 'string' || typeof v.password !== 'string') return null
    return {
      id: v.id,
      origin: v.origin,
      username: typeof v.username === 'string' ? v.username : '',
      password: v.password,
      createdAt: typeof v.createdAt === 'number' ? v.createdAt : 0,
      lastUsed: typeof v.lastUsed === 'number' ? v.lastUsed : null
    }
  } catch {
    return null
  }
}

const readItem = async (id: string): Promise<Credential | null> => parseCredential(await SecureStore.getItemAsync(ITEM + id, OPTIONS))
const writeItem = (c: Credential) => SecureStore.setItemAsync(ITEM + c.id, JSON.stringify(c), OPTIONS)

/** Is the Keychain usable here? (False on web, or if the native module is missing.) */
export async function vaultAvailable(): Promise<boolean> {
  try {
    return await SecureStore.isAvailableAsync()
  } catch {
    return false
  }
}

/** Loads the index once per session. Never rejects: an unreadable Keychain yields an empty vault. */
export function loadVault(): Promise<void> {
  loaded ??= (async () => {
    try {
      if (!(await vaultAvailable())) return
      const ids = await readList(IDS)
      const found: CredentialMeta[] = []
      for (const id of ids) {
        const c = await readItem(id)
        if (c) found.push(meta(c))
      }
      metas = found
      never = await readList(NEVER)
    } catch {
      /* empty vault */
    }
    emit()
  })()
  return loaded
}

export const getMetas = (): readonly CredentialMeta[] => metas
export const getNever = (): readonly string[] => never

const subscribe = (cb: () => void) => {
  listeners.add(cb)
  void loadVault()
  return () => void listeners.delete(cb)
}
/** Up-to-date index of logins (without passwords). */
export const useVaultMetas = (): readonly CredentialMeta[] => useSyncExternalStore(subscribe, getMetas, getMetas)
/** Sites for which saving is never offered. */
export const useNeverList = (): readonly string[] => useSyncExternalStore(subscribe, getNever, getNever)

/** Full login (with password). Call after an authentication. */
export async function getCredential(id: string): Promise<Credential | null> {
  await loadVault()
  try {
    return await readItem(id)
  } catch {
    return null
  }
}

/** Full logins that apply to `pageUrl`. */
export async function credentialsFor(pageUrl: string): Promise<Credential[]> {
  await loadVault()
  const out: Credential[] = []
  for (const m of credentialsForPage(metas, pageUrl)) {
    const c = await getCredential(m.id)
    if (c) out.push(c)
  }
  return out
}

/** Everything, passwords included (export, verification). Call after an authentication. */
export async function allCredentials(): Promise<Credential[]> {
  await loadVault()
  const out: Credential[] = []
  for (const m of metas) {
    const c = await getCredential(m.id)
    if (c) out.push(c)
  }
  return out
}

const idsOf = () => metas.map((m) => m.id)

export interface NewCredential {
  origin: string
  username: string
  password: string
  createdAt?: number
}

/** Saves a login; `null` if the origin or password is invalid. */
export function addCredential(input: NewCredential): Promise<CredentialMeta | null> {
  return serial(async () => {
    await loadVault()
    const origin = normalizeOrigin(input.origin)
    const username = input.username.trim().slice(0, MAX_FIELD)
    if (!origin || !input.password || input.password.length > MAX_FIELD) return null
    const now = Date.now()
    const c: Credential = { id: newCredentialId(now), origin, username, password: input.password, createdAt: input.createdAt ?? now, lastUsed: null }
    await writeItem(c)
    metas = [...metas, meta(c)]
    await writeList(IDS, idsOf())
    emit()
    return meta(c)
  })
}

export type CredentialPatch = Partial<Pick<Credential, 'origin' | 'username' | 'password' | 'lastUsed'>>

export function updateCredential(id: string, patch: CredentialPatch): Promise<boolean> {
  return serial(async () => {
    await loadVault()
    const current = await readItem(id)
    if (!current) return false
    const origin = patch.origin === undefined ? current.origin : normalizeOrigin(patch.origin)
    const password = patch.password ?? current.password
    if (!origin || !password || password.length > MAX_FIELD) return false
    const next: Credential = {
      ...current,
      origin,
      username: (patch.username ?? current.username).trim().slice(0, MAX_FIELD),
      password,
      lastUsed: patch.lastUsed === undefined ? current.lastUsed : patch.lastUsed
    }
    await writeItem(next)
    metas = metas.map((m) => (m.id === id ? meta(next) : m))
    emit()
    return true
  })
}

export function removeCredential(id: string): Promise<void> {
  return serial(async () => {
    await loadVault()
    metas = metas.filter((m) => m.id !== id)
    await writeList(IDS, idsOf())
    await SecureStore.deleteItemAsync(ITEM + id, OPTIONS).catch(() => undefined)
    emit()
  })
}

/** Imports rows (CSV): ones that already exist are kept as they are. */
export function importRows(rows: readonly ImportRow[]): Promise<{ added: number; existing: number }> {
  return serial(async () => {
    await loadVault()
    const plan = planImport(metas, rows)
    const now = Date.now()
    const added: Credential[] = plan.toAdd.map((r, i) => ({
      id: newCredentialId(now + i),
      origin: r.origin,
      username: r.username,
      password: r.password,
      createdAt: now,
      lastUsed: null
    }))
    for (const c of added) await writeItem(c)
    if (added.length) {
      metas = [...metas, ...added.map(meta)]
      await writeList(IDS, idsOf())
      emit()
    }
    return { added: added.length, existing: plan.existing }
  })
}

export function neverSaveFor(origin: string): Promise<void> {
  return serial(async () => {
    await loadVault()
    never = addNever(never, origin)
    await writeList(NEVER, never)
    emit()
  })
}

export function allowSavingFor(origin: string): Promise<void> {
  return serial(async () => {
    await loadVault()
    never = never.filter((o) => o !== origin)
    await writeList(NEVER, never)
    emit()
  })
}

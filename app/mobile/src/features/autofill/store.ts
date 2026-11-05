// Autofill storage: saved addresses and payment cards, in the iOS Keychain / Android Keystore (expo-secure-store),
// accessible when the device is unlocked and never backed up nor migrated to another device.
// One Keychain item per address, per card (metadata) and per card number, plus chunked id lists.
// Memory: addresses and card metadata (never a card number, never a security code: the code is not stored anywhere).
// A card number is only read on demand (`cardNumber`, `planCardSubmission`) and dropped as soon as the call ends.
import { useSyncExternalStore } from 'react'
import * as SecureStore from 'expo-secure-store'
import { cardDigits, detectNetwork, fullYear, isExpired, luhn } from '@signet/shared/autofill/card'
import {
  addressComplete,
  byUsage,
  canFill,
  cleanAddress,
  isEmptyAddress,
  mayOffer,
  mergeAddress,
  parseCardSubmission,
  planAddress,
  planCard,
  sameAddress,
  type AddressPlan,
  type AutofillKind,
  type CardCandidate,
  type CardPlan,
  type CardSubmission,
  type SavedCardRef
} from '@signet/shared/autofill/rules'
import { newId } from '@signet/shared/ids'
import { normalizeOrigin } from '@signet/shared/mobile/passwords'
import type { AddressInput, AutofillAddress, AutofillCard, CardInput, CardNetwork } from '@signet/shared/types'

const OPTIONS: SecureStore.SecureStoreOptions = { keychainAccessible: SecureStore.WHEN_UNLOCKED_THIS_DEVICE_ONLY }
const ADDRESS = 'signet.af.addr.'
const ADDRESS_IDS = 'signet.af.addr-ids'
const CARD = 'signet.af.card.'
const CARD_NUMBER = 'signet.af.cardnum.'
const CARD_IDS = 'signet.af.card-ids'
const NEVER = 'signet.af.never'
/** Items per list chunk (a Keychain item stays small). */
const CHUNK = 30
const MAX_NICKNAME = 40
const MAX_HOLDER = 200

export type { AddressPlan, AutofillKind, CardCandidate, CardPlan, CardSubmission, SavedCardRef }
export type { AddressInput, AutofillAddress, AutofillCard, CardInput, CardNetwork }

/** Why a save failed: the screens map each code to a message (`autofill.error.<code>`). */
export type AutofillErrorCode = 'unavailable' | 'empty' | 'missing' | 'number' | 'expiry' | 'expired' | 'duplicateAddress' | 'duplicateCard' | 'failed'

export class AutofillError extends Error {
  constructor(readonly code: AutofillErrorCode) {
    super(code)
    this.name = 'AutofillError'
  }
}

let loaded: Promise<void> | null = null
let available = false
let addresses: readonly AutofillAddress[] = []
let cards: readonly AutofillCard[] = []
let never: readonly string[] = []
let queue: Promise<unknown> = Promise.resolve()
const listeners = new Set<() => void>()
const emit = () => listeners.forEach((l) => l())

// One writer at a time: the id lists are re-read, modified and rewritten.
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
  // Extra chunks (shortened list), including an empty first one.
  for (let i = parts; i < 1000; i++) {
    if ((await SecureStore.getItemAsync(`${base}.${i}`, OPTIONS)) === null) break
    await SecureStore.deleteItemAsync(`${base}.${i}`, OPTIONS).catch(() => undefined)
  }
}

const str = (v: unknown, max = 1000): string => (typeof v === 'string' ? v.slice(0, max) : '')
const num = (v: unknown, fallback = 0): number => (typeof v === 'number' && Number.isFinite(v) ? v : fallback)

function parseAddress(raw: string | null): AutofillAddress | null {
  if (!raw) return null
  try {
    const v = JSON.parse(raw) as Partial<AutofillAddress>
    if (typeof v.id !== 'string' || !v.id) return null
    return {
      id: v.id,
      givenName: str(v.givenName),
      familyName: str(v.familyName),
      organization: str(v.organization),
      street: str(v.street, 600),
      city: str(v.city),
      postalCode: str(v.postalCode, 40),
      region: str(v.region),
      country: str(v.country, 2),
      phone: str(v.phone, 60),
      email: str(v.email),
      createdAt: num(v.createdAt),
      updatedAt: num(v.updatedAt),
      useCount: num(v.useCount),
      ...(typeof v.lastUsedAt === 'number' ? { lastUsedAt: v.lastUsedAt } : {})
    }
  } catch {
    return null
  }
}

const NETWORKS: readonly CardNetwork[] = ['visa', 'mastercard', 'amex', 'discover', 'diners', 'jcb', 'unionpay', 'maestro', 'unknown']

function parseCard(raw: string | null): AutofillCard | null {
  if (!raw) return null
  try {
    const v = JSON.parse(raw) as Partial<AutofillCard>
    if (typeof v.id !== 'string' || !v.id || typeof v.last4 !== 'string') return null
    return {
      id: v.id,
      holder: str(v.holder, MAX_HOLDER),
      last4: str(v.last4, 4),
      network: NETWORKS.includes(v.network as CardNetwork) ? (v.network as CardNetwork) : 'unknown',
      expMonth: num(v.expMonth),
      expYear: num(v.expYear),
      nickname: str(v.nickname, MAX_NICKNAME),
      createdAt: num(v.createdAt),
      updatedAt: num(v.updatedAt),
      useCount: num(v.useCount),
      ...(typeof v.lastUsedAt === 'number' ? { lastUsedAt: v.lastUsedAt } : {})
    }
  } catch {
    return null
  }
}

const writeAddress = (a: AutofillAddress) => SecureStore.setItemAsync(ADDRESS + a.id, JSON.stringify(a), OPTIONS)
const writeCard = (c: AutofillCard) => SecureStore.setItemAsync(CARD + c.id, JSON.stringify(c), OPTIONS)
const readNumber = (id: string) => SecureStore.getItemAsync(CARD_NUMBER + id, OPTIONS)

/** Is the Keychain usable here? (False on web, or if the native module is missing.) */
export async function autofillAvailable(): Promise<boolean> {
  try {
    return await SecureStore.isAvailableAsync()
  } catch {
    return false
  }
}

/** Loads addresses, card metadata and the never list once per session. Never rejects: an unreadable Keychain yields empty lists. */
export function loadAutofill(): Promise<void> {
  loaded ??= (async () => {
    try {
      available = await autofillAvailable()
      if (!available) return
      const foundAddresses: AutofillAddress[] = []
      for (const id of await readList(ADDRESS_IDS)) {
        const a = parseAddress(await SecureStore.getItemAsync(ADDRESS + id, OPTIONS))
        if (a) foundAddresses.push(a)
      }
      const foundCards: AutofillCard[] = []
      for (const id of await readList(CARD_IDS)) {
        const c = parseCard(await SecureStore.getItemAsync(CARD + id, OPTIONS))
        if (c) foundCards.push(c)
      }
      addresses = foundAddresses
      cards = foundCards
      never = await readList(NEVER)
    } catch {
      /* empty lists */
    }
    emit()
  })()
  return loaded
}

export const getAddresses = (): readonly AutofillAddress[] => addresses
export const getCards = (): readonly AutofillCard[] => cards
export const getAutofillNever = (): readonly string[] => never

const subscribe = (cb: () => void) => {
  listeners.add(cb)
  void loadAutofill()
  return () => void listeners.delete(cb)
}
/** Up-to-date saved addresses. */
export const useAddresses = (): readonly AutofillAddress[] => useSyncExternalStore(subscribe, getAddresses, getAddresses)
/** Up-to-date saved cards (metadata only: never the number). */
export const useCards = (): readonly AutofillCard[] => useSyncExternalStore(subscribe, getCards, getCards)
/** Sites (origins) where saving addresses and cards is never offered. */
export const useAutofillNever = (): readonly string[] => useSyncExternalStore(subscribe, getAutofillNever, getAutofillNever)

/** Saved items in the order a fill menu shows them: most used, then most recent. */
export const sortedByUsage = <T extends { useCount: number; lastUsedAt?: number; createdAt: number }>(items: readonly T[]): T[] => [...items].sort(byUsage)

/** Runs a write after loading, failing with `unavailable` if the Keychain is not usable. */
function write<T>(fn: () => Promise<T>): Promise<T> {
  return serial(async () => {
    await loadAutofill()
    if (!available) throw new AutofillError('unavailable')
    try {
      return await fn()
    } catch (err) {
      throw err instanceof AutofillError ? err : new AutofillError('failed')
    }
  })
}

// ───────────────────────── Addresses ─────────────────────────

export const getAddress = (id: string): AutofillAddress | undefined => addresses.find((a) => a.id === id)

/** Adds (no `id`) or edits an address. Throws `AutofillError`: empty, missing, duplicateAddress, unavailable, failed. */
export function upsertAddress(input: Partial<Record<keyof AddressInput | 'name', unknown>>, id?: string): Promise<AutofillAddress> {
  return write(async () => {
    const typed = cleanAddress(input)
    if (isEmptyAddress(typed)) throw new AutofillError('empty')
    const existing = id ? addresses.find((a) => a.id === id) : undefined
    if (id && !existing) throw new AutofillError('missing')
    if (addresses.some((a) => a.id !== existing?.id && sameAddress(a, typed))) throw new AutofillError('duplicateAddress')
    const now = Date.now()
    const next: AutofillAddress = existing ? { ...existing, ...typed, updatedAt: now } : { id: newId(), ...typed, createdAt: now, updatedAt: now, useCount: 0 }
    await writeAddress(next)
    if (existing) addresses = addresses.map((a) => (a.id === next.id ? next : a))
    else {
      addresses = [...addresses, next]
      await writeList(
        ADDRESS_IDS,
        addresses.map((a) => a.id)
      )
    }
    emit()
    return next
  })
}

export function removeAddress(id: string): Promise<void> {
  return write(async () => {
    if (!addresses.some((a) => a.id === id)) return
    addresses = addresses.filter((a) => a.id !== id)
    await writeList(
      ADDRESS_IDS,
      addresses.map((a) => a.id)
    )
    await SecureStore.deleteItemAsync(ADDRESS + id, OPTIONS).catch(() => undefined)
    emit()
  })
}

// ───────────────────────── Cards ─────────────────────────

export const getCard = (id: string): AutofillCard | undefined => cards.find((c) => c.id === id)

/** Number of a saved card, or null. CALLERS MUST AUTHENTICATE FIRST (Face ID, `passwords/auth.ts`); never keep the result. */
export async function cardNumber(id: string): Promise<string | null> {
  await loadAutofill()
  if (!available) return null
  try {
    return await readNumber(id)
  } catch {
    return null
  }
}

/** Is another saved card (not `exceptId`) already using this number? Numbers are read one at a time and dropped. */
async function numberTaken(digits: string, exceptId?: string): Promise<boolean> {
  for (const c of cards) {
    if (c.id === exceptId) continue
    if ((await readNumber(c.id)) === digits) return true
  }
  return false
}

/**
 * Adds (no `id`) or edits a card; `input.number` null = unchanged (required for a new card). The security code is not part
 * of the input and is never stored. Changing a number is sensitive: the screen asks for authentication before calling.
 * Throws `AutofillError`: unavailable, missing, number, expiry, expired, duplicateCard, failed.
 */
export function upsertCard(input: CardInput, id?: string): Promise<AutofillCard> {
  return write(async () => {
    const existing = id ? cards.find((c) => c.id === id) : undefined
    if (id && !existing) throw new AutofillError('missing')
    const expMonth = Number(input.expMonth)
    const expYear = fullYear(Number(input.expYear))
    if (!Number.isInteger(expMonth) || expMonth < 1 || expMonth > 12 || expYear === null) throw new AutofillError('expiry')
    const changedExpiry = !existing || existing.expMonth !== expMonth || existing.expYear !== expYear
    if (changedExpiry && isExpired(expMonth, expYear)) throw new AutofillError('expired')
    const typed = typeof input.number === 'string' && input.number.trim() ? input.number : null
    let digits: string | null = null
    if (typed !== null) {
      digits = cardDigits(typed)
      if (!digits || !luhn(digits)) throw new AutofillError('number')
      if (await numberTaken(digits, existing?.id)) throw new AutofillError('duplicateCard')
    } else if (!existing) throw new AutofillError('number')
    const now = Date.now()
    const base = {
      holder: input.holder.trim().slice(0, MAX_HOLDER),
      nickname: input.nickname.trim().slice(0, MAX_NICKNAME),
      expMonth,
      expYear,
      updatedAt: now
    }
    const number = digits ? { last4: digits.slice(-4), network: detectNetwork(digits) } : {}
    const next: AutofillCard = existing
      ? { ...existing, ...base, ...number }
      : { id: newId(), ...base, last4: '', network: 'unknown', ...number, createdAt: now, useCount: 0 }
    // The number first: a metadata item without a number would be a card that cannot be filled.
    if (digits) await SecureStore.setItemAsync(CARD_NUMBER + next.id, digits, OPTIONS)
    await writeCard(next)
    if (existing) cards = cards.map((c) => (c.id === next.id ? next : c))
    else {
      cards = [...cards, next]
      await writeList(
        CARD_IDS,
        cards.map((c) => c.id)
      )
    }
    emit()
    return next
  })
}

export function removeCard(id: string): Promise<void> {
  return write(async () => {
    if (!cards.some((c) => c.id === id)) return
    cards = cards.filter((c) => c.id !== id)
    await writeList(
      CARD_IDS,
      cards.map((c) => c.id)
    )
    await SecureStore.deleteItemAsync(CARD + id, OPTIONS).catch(() => undefined)
    await SecureStore.deleteItemAsync(CARD_NUMBER + id, OPTIONS).catch(() => undefined)
    emit()
  })
}

// ───────────────────────── Usage ─────────────────────────

/** Counts a fill: one more use, last used now. Never throws (statistics are not worth an error). */
export function markUsed(kind: AutofillKind, id: string): Promise<void> {
  return write(async () => {
    const now = Date.now()
    if (kind === 'address') {
      const a = addresses.find((x) => x.id === id)
      if (!a) return
      const next = { ...a, useCount: a.useCount + 1, lastUsedAt: now }
      await writeAddress(next)
      addresses = addresses.map((x) => (x.id === id ? next : x))
    } else {
      const c = cards.find((x) => x.id === id)
      if (!c) return
      const next = { ...c, useCount: c.useCount + 1, lastUsedAt: now }
      await writeCard(next)
      cards = cards.map((x) => (x.id === id ? next : x))
    }
    emit()
  }).catch(() => undefined)
}

// ───────────────────────── Never list ─────────────────────────

/** Never offer to save addresses or cards on this site (an origin such as `https://shop.example`). */
export function neverOfferFor(origin: string): Promise<void> {
  return write(async () => {
    const o = normalizeOrigin(origin)
    if (!o || never.includes(o)) return
    never = [...never, o]
    await writeList(NEVER, never)
    emit()
  })
}

export function allowOfferFor(origin: string): Promise<void> {
  return write(async () => {
    never = never.filter((o) => o !== origin)
    await writeList(NEVER, never)
    emit()
  })
}

export const isNeverOffer = (origin: string): boolean => never.includes(normalizeOrigin(origin) ?? origin)

// ───────────────────────── Planning (for the page bridge) ─────────────────────────

export { addressComplete, canFill, mergeAddress, parseCardSubmission }

/** May a fill be offered on this page? Settings switches are passed in (`settings.autofillAddresses` / `autofillCards`). */
export const mayFill = (kind: AutofillKind, origin: string | null | undefined, enabled: boolean): boolean => enabled && canFill(kind, origin)

/** May a save offer be shown? Applies the private tab, the switch, the never list and the page rules. */
export const mayOfferSave = (kind: AutofillKind, origin: string, opts: { isPrivate: boolean; enabled: boolean }): boolean =>
  mayOffer({ kind, origin, isPrivate: opts.isPrivate, enabled: opts.enabled, never, encryption: available })

/** Saved, complete addresses in menu order. */
export const addressesForFill = (): AutofillAddress[] => sortedByUsage(addresses)

/** What to do with a submitted address: nothing, save (new) or update (`id` with the merged address). */
export const planAddressSubmission = (typed: AddressInput): AddressPlan => planAddress(addresses, typed)

/** What to do with a submitted card: nothing, save (new) or update (`id`). Reads each saved number once, then drops it. */
export async function planCardSubmission(candidate: CardCandidate): Promise<CardPlan> {
  await loadAutofill()
  const refs: SavedCardRef[] = []
  for (const c of cards) {
    let number: string | null = null
    try {
      number = await readNumber(c.id)
    } catch {
      /* unreadable: cannot match */
    }
    refs.push({ id: c.id, number, holder: c.holder, expMonth: c.expMonth, expYear: c.expYear })
  }
  return planCard(refs, candidate)
}

/**
 * Saves a card read from a submission (after the user accepted the offer): a new card, or, with `id`, the new expiry
 * and holder of a known one. The number comes from the page and is not kept by this module.
 */
export function saveCandidate(candidate: CardCandidate, id?: string): Promise<AutofillCard> {
  const existing = id ? getCard(id) : undefined
  return upsertCard(
    {
      holder: candidate.holder || existing?.holder || '',
      number: id ? null : candidate.number,
      expMonth: candidate.expMonth,
      expYear: candidate.expYear,
      nickname: existing?.nickname ?? ''
    },
    id
  )
}

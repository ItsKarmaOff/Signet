// Autofill rules that need no Electron: where filling is allowed, what is worth offering to save, and when a
// submission matches something already saved. Kept apart so they can be tested without the app.
import type { AddressInput, AutofillAddress, CardNetwork } from '../types/autofill'
import { cardDigits, detectNetwork, fullYear, isExpired, luhn, parseExpiry } from './card'
import { countryCode, fullName, splitName } from './values'
import { normalizeText } from './classify'

export type AutofillKind = 'address' | 'card'

const MAX = 200
const MAX_STREET = 600

/** Hosts where http is as safe as https for a card: the developer's own machine. */
export function isLocalHost(hostname: string): boolean {
  const h = hostname.toLowerCase().replace(/^\[|\]$/g, '')
  return h === 'localhost' || h.endsWith('.localhost') || h === '127.0.0.1' || h === '::1'
}

/** Filling happens on http(s) pages only; a card also needs https (or localhost for development). */
export function canFill(kind: AutofillKind, origin: string | null | undefined): boolean {
  if (!origin) return false
  let u: URL
  try {
    u = new URL(origin)
  } catch {
    return false
  }
  if (u.protocol === 'https:') return true
  if (u.protocol !== 'http:') return false
  return kind === 'address' || isLocalHost(u.hostname)
}

export interface OfferContext {
  kind: AutofillKind
  origin: string
  isPrivate: boolean
  /** The "Save and fill" switch for this kind (missing = on). */
  enabled: boolean | undefined
  never: readonly string[] | undefined
  /** safeStorage can encrypt (cards only). */
  encryption: boolean
}

/** Whether a save offer may be shown at all. A save needs the same page rules as a fill. */
export function mayOffer(c: OfferContext): boolean {
  if (c.isPrivate || c.enabled === false) return false
  if ((c.never ?? []).includes(c.origin)) return false
  if (c.kind === 'card' && !c.encryption) return false
  return canFill(c.kind, c.origin)
}

const clip = (v: unknown, max = MAX): string => (typeof v === 'string' ? v.trim().slice(0, max) : '')

/** First and last name: as given, or, when only a full `name` is known, split from it. */
function names(raw: Partial<Record<'givenName' | 'familyName' | 'name', unknown>>): { givenName: string; familyName: string } {
  const givenName = clip(raw.givenName)
  const familyName = clip(raw.familyName)
  if (givenName || familyName) return { givenName, familyName }
  const { given, family } = splitName(clip(raw.name))
  return { givenName: given, familyName: family }
}

/** Address from untrusted input (a page, Settings): trimmed, clamped, country turned into its ISO code. */
export function cleanAddress(raw: Partial<Record<keyof AddressInput | 'name', unknown>>): AddressInput {
  const street = clip(raw.street, MAX_STREET)
    .split(/\r?\n/)
    .map((l) => l.trim())
    .filter(Boolean)
    .slice(0, 4)
    .join('\n')
  return {
    ...names(raw),
    organization: clip(raw.organization),
    street,
    city: clip(raw.city),
    postalCode: clip(raw.postalCode, 40),
    region: clip(raw.region),
    country: countryCode(clip(raw.country)),
    phone: clip(raw.phone, 60),
    email: clip(raw.email)
  }
}

/** Worth offering: a name, a street and where (city or postal code). */
export function addressComplete(a: AddressInput): boolean {
  return !!fullName(a) && !!a.street && !!(a.city || a.postalCode)
}

export const isEmptyAddress = (a: AddressInput): boolean => Object.values(a).every((v) => !v)

const norm = (s: string) => normalizeText(s).replace(/\s+/g, ' ')

/** Two addresses clearly describe the same place: same name and same street. */
export function sameAddress(a: AddressInput, b: AddressInput): boolean {
  return !!fullName(a) && norm(fullName(a)) === norm(fullName(b)) && !!a.street && norm(a.street) === norm(b.street)
}

const FIELDS: (keyof AddressInput)[] = ['givenName', 'familyName', 'organization', 'street', 'city', 'postalCode', 'region', 'country', 'phone', 'email']

/** True if every field is the same (case and accents aside). */
export function identicalAddress(a: AddressInput, b: AddressInput): boolean {
  return FIELDS.every((k) => norm(a[k]) === norm(b[k]))
}

/** Saved address updated with what was just typed: a non-empty value wins, an empty one keeps the saved value. */
export function mergeAddress(saved: AddressInput, typed: AddressInput): AddressInput {
  const out = { ...saved }
  for (const k of FIELDS) if (typed[k]) out[k] = typed[k]
  return out
}

export type AddressPlan = { action: 'none' } | { action: 'save' } | { action: 'update'; id: string; merged: AddressInput }

/** What to do with a submitted address: nothing (known), save (new), or update (same name and street, something changed). */
export function planAddress(saved: readonly AutofillAddress[], typed: AddressInput): AddressPlan {
  if (!addressComplete(typed)) return { action: 'none' }
  const known = saved.find((s) => sameAddress(s, typed))
  if (!known) return { action: 'save' }
  const merged = mergeAddress(known, typed)
  return identicalAddress(known, merged) ? { action: 'none' } : { action: 'update', id: known.id, merged }
}

/** Card read from a submission, already checked. */
export interface CardCandidate {
  holder: string
  number: string
  last4: string
  network: CardNetwork
  expMonth: number
  expYear: number
}

export interface CardSubmission {
  holder?: unknown
  number?: unknown
  /** Combined field (`08/27`). */
  exp?: unknown
  expMonth?: unknown
  expYear?: unknown
}

/** A card worth offering: number passes the Luhn check and the expiry is not in the past. */
export function parseCardSubmission(raw: CardSubmission, now = new Date()): CardCandidate | null {
  const digits = typeof raw.number === 'string' ? cardDigits(raw.number) : null
  if (!digits || !luhn(digits)) return null
  let month = 0
  let year = 0
  if (typeof raw.exp === 'string' && raw.exp.trim()) {
    const e = parseExpiry(raw.exp)
    if (e) ({ month, year } = e)
  } else {
    month = Number(clip(raw.expMonth, 4))
    year = fullYear(Number(clip(raw.expYear, 4))) ?? 0
  }
  if (!Number.isInteger(month) || month < 1 || month > 12 || !year || isExpired(month, year, now)) return null
  return { holder: clip(raw.holder), number: digits, last4: digits.slice(-4), network: detectNetwork(digits), expMonth: month, expYear: year }
}

export interface SavedCardRef {
  id: string
  /** Decrypted number, null if it cannot be read. */
  number: string | null
  holder: string
  expMonth: number
  expYear: number
}

export type CardPlan = { action: 'none' } | { action: 'save' } | { action: 'update'; id: string }

/** Same number = same card: nothing if the rest matches too, otherwise an update (new expiry after a renewal...). */
export function planCard(saved: readonly SavedCardRef[], typed: CardCandidate): CardPlan {
  const known = saved.find((s) => s.number === typed.number)
  if (!known) return { action: 'save' }
  const same = known.expMonth === typed.expMonth && known.expYear === typed.expYear && (!typed.holder || norm(known.holder) === norm(typed.holder))
  return same ? { action: 'none' } : { action: 'update', id: known.id }
}

/** Saved items in the order the menu shows them: most used, then most recent. */
export function byUsage<T extends { useCount: number; lastUsedAt?: number; createdAt: number }>(a: T, b: T): number {
  return b.useCount - a.useCount || (b.lastUsedAt ?? 0) - (a.lastUsedAt ?? 0) || b.createdAt - a.createdAt
}

// Values to put in fields and options to pick: name splitting, countries, select matching. Pure.
import type { AddressInput } from '../types/autofill'
import type { AddressKind } from './classify'
import { normalizeText } from './classify'
import { COUNTRY_CODES, COUNTRY_NAMES } from './countries'

/** Given and family names from a full name (`Last, First` and particles such as `van der` are understood). */
export function splitName(full: string): { given: string; family: string } {
  const name = full.trim().replace(/\s+/g, ' ')
  if (!name) return { given: '', family: '' }
  const comma = name.indexOf(',')
  if (comma > 0) return { given: name.slice(comma + 1).trim(), family: name.slice(0, comma).trim() }
  const words = name.split(' ')
  if (words.length === 1) return { given: name, family: '' }
  return { given: words[0], family: words.slice(1).join(' ') }
}

export const joinName = (given: string, family: string): string =>
  [given, family]
    .map((s) => s.trim())
    .filter(Boolean)
    .join(' ')

/** Full name of an address: first and last name joined, whichever exist. */
export const fullName = (a: Pick<AddressInput, 'givenName' | 'familyName'>): string => joinName(a.givenName, a.familyName)

const LOCALES = ['en', 'fr']

interface RegionNames {
  of(code: string): string | undefined
}

/** `Intl.DisplayNames` for a locale, or null where it is missing (Hermes) or the locale is unknown. */
function displayNames(locale: string): RegionNames | null {
  try {
    return new Intl.DisplayNames([locale], { type: 'region' })
  } catch {
    return null
  }
}

/** Name of a country in a locale: from Intl when it knows it, else from the static table (English and French only). */
function regionName(names: RegionNames | null, locale: string, code: string): string {
  let name: string | undefined
  try {
    name = names?.of(code)
  } catch {
    name = undefined
  }
  if (name && name !== code) return name
  const entry = COUNTRY_NAMES[code]
  const lang = locale.slice(0, 2).toLowerCase()
  return entry && (lang === 'fr' || lang === 'en') ? entry[lang] : ''
}

/** Display name of a country in a locale (the code itself if unknown). Works without Intl.DisplayNames. */
export function countryName(code: string, locale: string): string {
  if (!COUNTRY_NAMES[code]) return code
  return regionName(displayNames(locale), locale, code) || code
}

/** All countries as code and name in a locale, sorted by name (accents and case ignored). */
export function countryList(locale: string): { code: string; name: string }[] {
  const names = displayNames(locale)
  return COUNTRY_CODES.map((code) => ({ code, name: regionName(names, locale, code) || code })).sort((a, b) => {
    const x = normalizeText(a.name)
    const y = normalizeText(b.name)
    return x < y ? -1 : x > y ? 1 : 0
  })
}

let countryNames: Map<string, string> | null = null

/** ISO code of a country given as a code or a name (English, French or `extra` locales); empty if unknown. */
export function countryCode(input: string, extra: string[] = []): string {
  const text = input.trim()
  if (!text) return ''
  if (/^[A-Za-z]{2}$/.test(text)) return text.toUpperCase()
  if (!countryNames) {
    const map = new Map<string, string>()
    for (const locale of LOCALES) {
      const names = displayNames(locale)
      for (const code of COUNTRY_CODES) {
        const name = regionName(names, locale, code)
        if (name && !map.has(normalizeText(name))) map.set(normalizeText(name), code)
      }
    }
    countryNames = map
  }
  const key = normalizeText(text)
  const hit = countryNames.get(key)
  if (hit) return hit
  for (const locale of extra) {
    const names = displayNames(locale)
    if (!names) continue
    for (const code of COUNTRY_CODES) if (normalizeText(regionName(names, locale, code)) === key) return code
  }
  return ''
}

/** Names of a country in the languages a form may use. */
export function countryNamesOf(code: string, extra: string[] = []): string[] {
  if (!COUNTRY_NAMES[code]) return []
  const out: string[] = []
  for (const locale of [...extra, ...LOCALES]) {
    const name = regionName(displayNames(locale), locale, code)
    if (name && !out.includes(name)) out.push(name)
  }
  return out
}

/** Text value for an address field. `multiline`: a textarea can hold several lines. */
export function addressValue(a: AddressInput, kind: AddressKind, opts: { multiline?: boolean; locales?: string[] } = {}): string {
  const lines = a.street
    .split('\n')
    .map((l) => l.trim())
    .filter(Boolean)
  switch (kind) {
    case 'name':
      return fullName(a)
    case 'given-name':
      return a.givenName.trim()
    case 'family-name':
      return a.familyName.trim()
    case 'organization':
      return a.organization
    case 'street-address':
      return lines.join(opts.multiline ? '\n' : ', ')
    case 'address-line1':
      return lines[0] ?? ''
    case 'address-line2':
      return lines[1] ?? ''
    case 'address-line3':
      return lines.slice(2).join(', ')
    case 'city':
      return a.city
    case 'region':
      return a.region
    case 'postal-code':
      return a.postalCode
    case 'country':
      return a.country
    case 'country-name':
      return countryNamesOf(a.country, opts.locales)[0] ?? a.country
    case 'tel':
      return a.phone
    case 'email':
      return a.email
  }
}

export interface OptionLike {
  value: string
  text: string
}

/**
 * Picks the option that stands for one of the `candidates` (tried in order): the exact value or text first, then,
 * for candidates of 4+ letters, a text that starts with it. Returns the index, or -1.
 */
export function pickOption(options: OptionLike[], candidates: string[]): number {
  const norm = options.map((o) => ({ v: normalizeText(o.value), t: normalizeText(o.text) }))
  const wanted = candidates.map(normalizeText).filter(Boolean)
  for (const c of wanted) {
    const i = norm.findIndex((o) => o.v === c || o.t === c)
    if (i >= 0) return i
  }
  for (const c of wanted) {
    if (c.length < 4) continue
    const i = norm.findIndex((o) => o.t.startsWith(`${c} `) || o.t.startsWith(`${c}(`) || o.t.endsWith(` ${c}`))
    if (i >= 0) return i
  }
  return -1
}

/** Candidates for a country or region select: the code and the names it goes by. */
export function placeCandidates(kind: AddressKind, a: AddressInput, locales: string[] = []): string[] {
  if (kind === 'country' || kind === 'country-name') return [a.country, ...countryNamesOf(a.country, locales)].filter(Boolean)
  if (kind === 'region') return [a.region].filter(Boolean)
  return []
}

/** Month option: value or text `8`, `08`, or text that starts with it (`08 - August`). */
export function pickMonthOption(options: OptionLike[], month: number): number {
  const exact = pickOption(options, [String(month).padStart(2, '0'), String(month)])
  if (exact >= 0) return exact
  const re = new RegExp(`^0*${month}(\\D|$)`)
  return options.findIndex((o) => re.test(o.text.trim()) || re.test(o.value.trim()))
}

/** Candidates for a year select. */
export function yearCandidates(year: number): string[] {
  const full = String(year)
  return [full, full.slice(-2)]
}

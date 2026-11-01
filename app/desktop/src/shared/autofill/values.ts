// Values to put in fields and options to pick: name splitting, countries, select matching. Pure.
import type { AddressInput } from '../types/autofill'
import type { AddressKind } from './classify'
import { normalizeText } from './classify'

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

/** ISO 3166-1 alpha-2 codes (current countries and territories). */
const CODES =
  'AD AE AF AG AI AL AM AO AQ AR AS AT AU AW AX AZ BA BB BD BE BF BG BH BI BJ BL BM BN BO BQ BR BS BT BV BW BY BZ CA CC CD CF CG CH CI CK CL CM CN CO CR CU CV CW CX CY CZ DE DJ DK DM DO DZ EC EE EG EH ER ES ET FI FJ FK FM FO FR GA GB GD GE GF GG GH GI GL GM GN GP GQ GR GS GT GU GW GY HK HM HN HR HT HU ID IE IL IM IN IO IQ IR IS IT JE JM JO JP KE KG KH KI KM KN KP KR KW KY KZ LA LB LC LI LK LR LS LT LU LV LY MA MC MD ME MF MG MH MK ML MM MN MO MP MQ MR MS MT MU MV MW MX MY MZ NA NC NE NF NG NI NL NO NP NR NU NZ OM PA PE PF PG PH PK PL PM PN PR PS PT PW PY QA RE RO RS RU RW SA SB SC SD SE SG SH SI SJ SK SL SM SN SO SR SS ST SV SX SY SZ TC TD TF TG TH TJ TK TL TM TN TO TR TT TV TW TZ UA UG UM US UY UZ VA VC VE VG VI VN VU WF WS YE YT ZA ZM ZW'.split(
    ' '
  )

let countryNames: Map<string, string> | null = null

/** ISO code of a country given as a code or a name (English, French or `extra` locales); empty if unknown. */
export function countryCode(input: string, extra: string[] = []): string {
  const text = input.trim()
  if (!text) return ''
  if (/^[A-Za-z]{2}$/.test(text)) return text.toUpperCase()
  if (!countryNames) {
    countryNames = new Map()
    try {
      for (const locale of LOCALES) {
        const names = new Intl.DisplayNames([locale], { type: 'region' })
        for (const code of CODES) {
          const name = names.of(code)
          if (name && name !== code && !countryNames.has(normalizeText(name))) countryNames.set(normalizeText(name), code)
        }
      }
    } catch {
      // no ICU data: codes only
    }
  }
  const key = normalizeText(text)
  const hit = countryNames.get(key)
  if (hit) return hit
  for (const locale of extra) {
    try {
      const names = new Intl.DisplayNames([locale], { type: 'region' })
      for (const code of CODES) if (normalizeText(names.of(code)) === key) return code
    } catch {
      // unknown locale
    }
  }
  return ''
}

/** Names of a country in the languages a form may use. */
export function countryNamesOf(code: string, extra: string[] = []): string[] {
  if (!CODES.includes(code)) return []
  const out: string[] = []
  for (const locale of [...extra, ...LOCALES]) {
    try {
      const name = new Intl.DisplayNames([locale], { type: 'region' }).of(code)
      if (name && name !== code && !out.includes(name)) out.push(name)
    } catch {
      // unknown locale
    }
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

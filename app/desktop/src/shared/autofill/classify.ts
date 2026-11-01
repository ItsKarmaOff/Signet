// Field classifier for autofill: decides what a form field asks for (name, street, card number...). Pure: the page
// script describes each field, this module answers. English and French heuristics; `autocomplete` always wins.

export const ADDRESS_KINDS = [
  'name',
  'given-name',
  'family-name',
  'organization',
  'street-address',
  'address-line1',
  'address-line2',
  'address-line3',
  'city',
  'region',
  'postal-code',
  'country',
  'country-name',
  'tel',
  'email'
] as const

export const CARD_KINDS = ['cc-name', 'cc-number', 'cc-exp', 'cc-exp-month', 'cc-exp-year', 'cc-csc', 'cc-type'] as const

export type AddressKind = (typeof ADDRESS_KINDS)[number]
export type CardKind = (typeof CARD_KINDS)[number]
export type FieldKind = AddressKind | CardKind

export const isCardKind = (k: FieldKind): k is CardKind => (CARD_KINDS as readonly string[]).includes(k)

/** What the page script knows about a field. `type` is the input type, or `select` / `textarea`. */
export interface FieldDescriptor {
  type?: string
  autocomplete?: string
  name?: string
  id?: string
  label?: string
  placeholder?: string
  ariaLabel?: string
  hidden?: boolean
  disabled?: boolean
  readOnly?: boolean
}

export type Scope = '' | 'shipping' | 'billing'

export interface FieldClass {
  kind: FieldKind
  /** `section-xxx` token of the autocomplete attribute, or empty. */
  section: string
  /** `shipping` / `billing`, from the autocomplete attribute or the field name. */
  scope: Scope
  /** True when the `autocomplete` attribute decided. */
  explicit: boolean
}

const FILLABLE_TYPES = new Set(['', 'text', 'search', 'tel', 'email', 'number', 'select', 'textarea'])

const AUTOCOMPLETE: Record<string, FieldKind> = {
  name: 'name',
  'given-name': 'given-name',
  'additional-name': 'given-name',
  'family-name': 'family-name',
  organization: 'organization',
  'street-address': 'street-address',
  'address-line1': 'address-line1',
  'address-line2': 'address-line2',
  'address-line3': 'address-line3',
  'address-level1': 'region',
  'address-level2': 'city',
  'postal-code': 'postal-code',
  country: 'country',
  'country-name': 'country-name',
  tel: 'tel',
  'tel-national': 'tel',
  email: 'email',
  'cc-name': 'cc-name',
  'cc-given-name': 'cc-name',
  'cc-family-name': 'cc-name',
  'cc-number': 'cc-number',
  'cc-exp': 'cc-exp',
  'cc-exp-month': 'cc-exp-month',
  'cc-exp-year': 'cc-exp-year',
  'cc-csc': 'cc-csc',
  'cc-type': 'cc-type'
}

/** Tokens that settle a field is none of our business (`username`, `one-time-code`...). */
const FOREIGN = new Set([
  'username',
  'current-password',
  'new-password',
  'one-time-code',
  'bday',
  'bday-day',
  'bday-month',
  'bday-year',
  'url',
  'photo',
  'impp'
])

/** Reads the `autocomplete` attribute: [section, scope, kind]; kind is null when the attribute says nothing usable. */
export function parseAutocomplete(attr: string | undefined): { section: string; scope: Scope; kind: FieldKind | null; foreign: boolean } {
  const out = { section: '', scope: '' as Scope, kind: null as FieldKind | null, foreign: false }
  for (const token of (attr ?? '').toLowerCase().split(/\s+/).filter(Boolean)) {
    if (token.startsWith('section-')) out.section = token
    else if (token === 'shipping' || token === 'billing') out.scope = token
    else if (token in AUTOCOMPLETE) out.kind = AUTOCOMPLETE[token]
    else if (FOREIGN.has(token)) out.foreign = true
  }
  return out
}

/** Lowercase, no accents, separators turned into spaces, camelCase split (`postalCode` -> `postal code`). */
export function normalizeText(text: string | undefined): string {
  return (text ?? '')
    .replace(/([a-z])([A-Z])/g, '$1 $2')
    .normalize('NFD')
    .replace(/[̀-ͯ]/g, '')
    .toLowerCase()
    .replace(/[_.[\]:'’-]+/g, ' ')
    .replace(/\s+/g, ' ')
    .trim()
}

type Rule = [FieldKind, RegExp]

// Order matters: the first rule that matches wins, so specific kinds come before general ones.
const RULES: Rule[] = [
  ['cc-csc', /\b(cvc2?|cvv2?|csc|cid|cvn)\b|security code|card verification|code de (securite|verification)|cryptogramme|code de la carte/],
  [
    'cc-name',
    /name on (the )?card|\bcard ?name\b|\bcc ?name\b|card ?holder|cardholder|holder name|nom (sur|du titulaire|figurant)|titulaire|nom (de|sur) la carte/
  ],
  ['cc-type', /card ?type|cc ?type|card brand|type de carte|type of card/],
  ['cc-exp-month', /\bexp\w*\b.*\b(month|mm|mois)\b|\b(month|mois) ?(of |d |de )?exp|\bcc ?(exp ?)?(month|mm)\b|\bexpmonth\b|\bmois d ?expiration/],
  ['cc-exp-year', /\bexp\w*\b.*\b(year|yy|yyyy|annee|aa|aaaa)\b|\b(year|annee) ?(of |d |de )?exp|\bcc ?(exp ?)?(year|yy)\b|\bexpyear\b|\bannee d ?expiration/],
  ['cc-exp', /\bexp(iry|iration|ire|ires)?\b|valid (thru|until|through|to)|valable jusqu|date d ?exp|\bmm ?\/ ?(yy|yyyy|aa|aaaa)\b/],
  [
    'cc-number',
    /card ?(number|num|no)\b|cc ?(number|num|no)\b|ccnum|cardnum|credit ?card|debit ?card|numero ?(de ?)?(la ?)?carte|carte ?(bancaire|de credit|bleue)|number on (the )?card|\bpan\b/
  ],
  ['email', /e ?mail|courriel|\bmel\b/],
  ['tel', /phone|\btel\b|telephone|mobile|portable|\bgsm\b|cellulaire|\bcell\b/],
  ['postal-code', /post(al)? ?code|\bzip\b|zip ?code|code ?postal|\bcp\b|\bplz\b|postcode/],
  ['city', /\bcity\b|\btown\b|\bville\b|commune|locality|localite|suburb|municipality/],
  ['country', /\bcountry\b|\bpays\b|\bnation\b/],
  ['region', /\bstate\b|province|\bregion\b|\bcounty\b|departement|prefecture|state ?\/ ?province/],
  ['organization', /company|organi[sz]ation|business name|societe|entreprise|\bcorp\b|employer/],
  ['address-line3', /(address|adresse|street|line|ligne) ?3\b/],
  [
    'address-line2',
    /(address|adresse|street|line|ligne) ?(2|two|deux)\b|apartment|\bapt\b|\bsuite\b|\bunit\b|complement|batiment|\bfloor\b|\betage\b|\bresidence\b/
  ],
  ['address-line1', /(address|adresse|street|line|ligne|rue) ?(1|one|un)\b|address ?line|street ?address|ligne d ?adresse/],
  ['street-address', /\baddress\b|\badresse\b|\bstreet\b|\brue\b|\bvoie\b|domicile/],
  ['given-name', /first ?name|given ?name|\bfname\b|forename|\bprenom\b|firstname/],
  ['family-name', /last ?name|family ?name|surname|\blname\b|lastname|nom de famille/],
  ['name', /full ?name|your name|nom complet|nom et prenom|prenom et nom|\bname\b|\bnom\b/]
]

/** Words that turn a generic match into a different thing (`username` is not a person name). */
const REJECT = /user ?name|login|pseudo|identifiant|file ?name|domain|nickname|\burl\b|website|site web|\bip\b/

const SCOPE = /\b(shipping|delivery|livraison)\b/
const BILLING = /\b(billing|facturation|invoice)\b/

function scopeOf(...texts: (string | undefined)[]): Scope {
  const text = texts.map(normalizeText).join(' ')
  if (SCOPE.test(text)) return 'shipping'
  if (BILLING.test(text)) return 'billing'
  return ''
}

function byTypeOrText(d: FieldDescriptor): FieldKind | null {
  const type = d.type ?? ''
  if (type === 'email') return 'email'
  if (type === 'tel') return 'tel'
  // Most telling first: the field's own name and id, then what the user reads.
  for (const raw of [d.name, d.id, d.label, d.placeholder, d.ariaLabel]) {
    const text = normalizeText(raw)
    if (!text) continue
    const hit = RULES.find(([kind, re]) => re.test(text) && !(REJECT.test(text) && !isCardKind(kind) && kind !== 'email'))
    if (hit) return hit[0]
  }
  return null
}

/** Classifies a single field without looking at its neighbours; null if it is not an autofill field. */
export function classifyField(d: FieldDescriptor): FieldClass | null {
  if (d.hidden || d.disabled || d.readOnly) return null
  if (!FILLABLE_TYPES.has(d.type ?? '')) return null
  const auto = parseAutocomplete(d.autocomplete)
  if (auto.foreign) return null
  if (auto.kind) return { kind: auto.kind, section: auto.section, scope: auto.scope || scopeOf(d.name, d.id), explicit: true }
  // `type=number` alone says nothing: only a card number or a postal code is plausible there, which the rules find.
  const kind = byTypeOrText(d)
  return kind ? { kind, section: auto.section, scope: auto.scope || scopeOf(d.name, d.id), explicit: false } : null
}

/** Classifies all the fields of a form, then settles what needs the neighbours (`mois` next to a card number...). */
export function classifyFields(fields: FieldDescriptor[]): (FieldClass | null)[] {
  const out = fields.map(classifyField)
  const hasCard = out.some((c) => c?.kind === 'cc-number')
  if (hasCard) {
    // Loose month and year fields only count in a card form (a birth date has the same words).
    fields.forEach((d, i) => {
      if (out[i] || d.hidden || d.disabled || d.readOnly || !FILLABLE_TYPES.has(d.type ?? '')) return
      for (const raw of [d.name, d.id, d.label, d.placeholder, d.ariaLabel]) {
        const text = normalizeText(raw)
        if (!text || /birth|naissance|born/.test(text)) continue
        const kind: CardKind | null = /\b(month|mois|mm)\b/.test(text) ? 'cc-exp-month' : /\b(year|annee|yy|yyyy|aa|aaaa)\b/.test(text) ? 'cc-exp-year' : null
        if (kind) {
          out[i] = { kind, section: '', scope: '', explicit: false }
          return
        }
      }
    })
  }
  // "Nom" next to a "Prenom" is the family name; two "name" fields in a row do not make a full name each.
  const hasGiven = out.some((c) => c?.kind === 'given-name')
  const hasFamily = out.some((c) => c?.kind === 'family-name')
  fields.forEach((d, i) => {
    const c = out[i]
    if (!c || c.explicit || c.kind !== 'name') return
    const text = normalizeText([d.name, d.id, d.label, d.placeholder, d.ariaLabel].filter(Boolean).join(' '))
    if (/full|complet/.test(text)) return
    if (hasGiven && !hasFamily) out[i] = { ...c, kind: 'family-name' }
  })
  return out
}

/** Location kinds: an address form needs at least one of them. */
const LOCATION = new Set<FieldKind>(['street-address', 'address-line1', 'address-line2', 'city', 'region', 'postal-code', 'country', 'country-name'])

export function isAddressKind(k: FieldKind): k is AddressKind {
  return !isCardKind(k)
}

/** Address form: at least three address fields, one of which says where (name + email + phone alone is a sign-up). */
export function isAddressForm(classes: (FieldClass | null)[]): boolean {
  const list = classes.filter((c): c is FieldClass => !!c && isAddressKind(c.kind))
  return list.length >= 3 && list.some((c) => LOCATION.has(c.kind))
}

/** Card form: there is a card number field. */
export function isCardForm(classes: (FieldClass | null)[]): boolean {
  return classes.some((c) => c?.kind === 'cc-number')
}

/** Fields that fill together: same section and same shipping/billing scope as the target. */
export function sameGroup(a: FieldClass, b: FieldClass): boolean {
  // An email or phone without a shipping/billing mark is shared by both addresses.
  return a.section === b.section && (a.scope === b.scope || ((b.kind === 'email' || b.kind === 'tel') && !b.scope))
}

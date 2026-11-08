// Pure helpers of the autofill page script: field classifier, card helpers, values to fill. No DOM, no bridge.
// Port of app/desktop/src/shared/autofill/{classify,card,values}.ts (the parts the page script uses).
// Evaluated once and handed to autofill.js; also loaded as is by the Node tests.
(function autofillCore() {
  'use strict'

  // ───────────── classify.ts ─────────────

  const ADDRESS_KINDS = [
    'name', 'given-name', 'family-name', 'organization', 'street-address', 'address-line1', 'address-line2', 'address-line3',
    'city', 'region', 'postal-code', 'country', 'country-name', 'tel', 'email'
  ]
  const CARD_KINDS = ['cc-name', 'cc-number', 'cc-exp', 'cc-exp-month', 'cc-exp-year', 'cc-csc', 'cc-type']

  const isCardKind = (k) => CARD_KINDS.includes(k)
  const isAddressKind = (k) => !isCardKind(k)

  const FILLABLE_TYPES = new Set(['', 'text', 'search', 'tel', 'email', 'number', 'select', 'textarea'])

  const AUTOCOMPLETE = {
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
    'username', 'current-password', 'new-password', 'one-time-code', 'bday', 'bday-day', 'bday-month', 'bday-year', 'url', 'photo', 'impp'
  ])

  const own = (o, k) => Object.prototype.hasOwnProperty.call(o, k)

  /** Reads the `autocomplete` attribute; kind is null when the attribute says nothing usable. */
  function parseAutocomplete(attr) {
    const out = { section: '', scope: '', kind: null, foreign: false }
    for (const token of (attr || '').toLowerCase().split(/\s+/).filter(Boolean)) {
      if (token.startsWith('section-')) out.section = token
      else if (token === 'shipping' || token === 'billing') out.scope = token
      else if (own(AUTOCOMPLETE, token)) out.kind = AUTOCOMPLETE[token]
      else if (FOREIGN.has(token)) out.foreign = true
    }
    return out
  }

  /** Lowercase, no accents, separators turned into spaces, camelCase split (`postalCode` -> `postal code`). */
  function normalizeText(text) {
    return (text || '')
      .replace(/([a-z])([A-Z])/g, '$1 $2')
      .normalize('NFD')
      .replace(/[̀-ͯ]/g, '')
      .toLowerCase()
      .replace(/[_.[\]:'’-]+/g, ' ')
      .replace(/\s+/g, ' ')
      .trim()
  }

  // Order matters: the first rule that matches wins, so specific kinds come before general ones.
  const RULES = [
    ['cc-csc', /\b(cvc2?|cvv2?|csc|cid|cvn)\b|security code|card verification|code de (securite|verification)|cryptogramme|code de la carte/],
    ['cc-name', /name on (the )?card|\bcard ?name\b|\bcc ?name\b|card ?holder|cardholder|holder name|nom (sur|du titulaire|figurant)|titulaire|nom (de|sur) la carte/],
    ['cc-type', /card ?type|cc ?type|card brand|type de carte|type of card/],
    ['cc-exp-month', /\bexp\w*\b.*\b(month|mm|mois)\b|\b(month|mois) ?(of |d |de )?exp|\bcc ?(exp ?)?(month|mm)\b|\bexpmonth\b|\bmois d ?expiration/],
    ['cc-exp-year', /\bexp\w*\b.*\b(year|yy|yyyy|annee|aa|aaaa)\b|\b(year|annee) ?(of |d |de )?exp|\bcc ?(exp ?)?(year|yy)\b|\bexpyear\b|\bannee d ?expiration/],
    ['cc-exp', /\bexp(iry|iration|ire|ires)?\b|valid (thru|until|through|to)|valable jusqu|date d ?exp|\bmm ?\/ ?(yy|yyyy|aa|aaaa)\b/],
    ['cc-number', /card ?(number|num|no)\b|cc ?(number|num|no)\b|ccnum|cardnum|credit ?card|debit ?card|numero ?(de ?)?(la ?)?carte|carte ?(bancaire|de credit|bleue)|number on (the )?card|\bpan\b/],
    ['email', /e ?mail|courriel|\bmel\b/],
    ['tel', /phone|\btel\b|telephone|mobile|portable|\bgsm\b|cellulaire|\bcell\b/],
    ['postal-code', /post(al)? ?code|\bzip\b|zip ?code|code ?postal|\bcp\b|\bplz\b|postcode/],
    ['city', /\bcity\b|\btown\b|\bville\b|commune|locality|localite|suburb|municipality/],
    ['country', /\bcountry\b|\bpays\b|\bnation\b/],
    ['region', /\bstate\b|province|\bregion\b|\bcounty\b|departement|prefecture|state ?\/ ?province/],
    ['organization', /company|organi[sz]ation|business name|societe|entreprise|\bcorp\b|employer/],
    ['address-line3', /(address|adresse|street|line|ligne) ?3\b/],
    ['address-line2', /(address|adresse|street|line|ligne) ?(2|two|deux)\b|apartment|\bapt\b|\bsuite\b|\bunit\b|complement|batiment|\bfloor\b|\betage\b|\bresidence\b/],
    ['address-line1', /(address|adresse|street|line|ligne|rue) ?(1|one|un)\b|address ?line|street ?address|ligne d ?adresse/],
    ['street-address', /\baddress\b|\badresse\b|\bstreet\b|\brue\b|\bvoie\b|domicile/],
    ['given-name', /first ?name|given ?name|\bfname\b|forename|\bprenom\b|firstname/],
    ['family-name', /last ?name|family ?name|surname|\blname\b|lastname|nom de famille/],
    ['name', /full ?name|your name|nom complet|nom et prenom|prenom et nom|\bname\b|\bnom\b/]
  ]

  /** Words that turn a generic match into a different thing (`username` is not a person name). */
  const REJECT = /user ?name|login|pseudo|identifiant|file ?name|domain|nickname|\burl\b|website|site web|\bip\b/

  const SHIPPING_RE = /\b(shipping|delivery|livraison)\b/
  const BILLING_RE = /\b(billing|facturation|invoice)\b/

  function scopeOf(...texts) {
    const text = texts.map(normalizeText).join(' ')
    if (SHIPPING_RE.test(text)) return 'shipping'
    if (BILLING_RE.test(text)) return 'billing'
    return ''
  }

  function byTypeOrText(d) {
    const type = d.type || ''
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
  function classifyField(d) {
    if (d.hidden || d.disabled || d.readOnly) return null
    if (!FILLABLE_TYPES.has(d.type || '')) return null
    const auto = parseAutocomplete(d.autocomplete)
    if (auto.foreign) return null
    if (auto.kind) return { kind: auto.kind, section: auto.section, scope: auto.scope || scopeOf(d.name, d.id), explicit: true }
    const kind = byTypeOrText(d)
    return kind ? { kind, section: auto.section, scope: auto.scope || scopeOf(d.name, d.id), explicit: false } : null
  }

  /** Classifies all the fields of a form, then settles what needs the neighbours (`mois` next to a card number...). */
  function classifyFields(fields) {
    const out = fields.map(classifyField)
    const hasCard = out.some((c) => c && c.kind === 'cc-number')
    if (hasCard) {
      // Loose month and year fields only count in a card form (a birth date has the same words).
      fields.forEach((d, i) => {
        if (out[i] || d.hidden || d.disabled || d.readOnly || !FILLABLE_TYPES.has(d.type || '')) return
        for (const raw of [d.name, d.id, d.label, d.placeholder, d.ariaLabel]) {
          const text = normalizeText(raw)
          if (!text || /birth|naissance|born/.test(text)) continue
          const kind = /\b(month|mois|mm)\b/.test(text) ? 'cc-exp-month' : /\b(year|annee|yy|yyyy|aa|aaaa)\b/.test(text) ? 'cc-exp-year' : null
          if (kind) {
            out[i] = { kind, section: '', scope: '', explicit: false }
            return
          }
        }
      })
    }
    // "Nom" next to a "Prenom" is the family name; two "name" fields in a row do not make a full name each.
    const hasGiven = out.some((c) => c && c.kind === 'given-name')
    const hasFamily = out.some((c) => c && c.kind === 'family-name')
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
  const LOCATION = new Set(['street-address', 'address-line1', 'address-line2', 'city', 'region', 'postal-code', 'country', 'country-name'])

  /** Address form: at least three address fields, one of which says where (name + email + phone alone is a sign-up). */
  function isAddressForm(classes) {
    const list = classes.filter((c) => !!c && isAddressKind(c.kind))
    return list.length >= 3 && list.some((c) => LOCATION.has(c.kind))
  }

  /** Card form: there is a card number field. */
  function isCardForm(classes) {
    return classes.some((c) => c && c.kind === 'cc-number')
  }

  /** Fields that fill together: same section and same shipping/billing scope as the target. */
  function sameGroup(a, b) {
    // An email or phone without a shipping/billing mark is shared by both addresses.
    return a.section === b.section && (a.scope === b.scope || ((b.kind === 'email' || b.kind === 'tel') && !b.scope))
  }

  // ───────────── card.ts ─────────────

  const NETWORK_NAMES = {
    visa: 'Visa',
    mastercard: 'Mastercard',
    amex: 'American Express',
    discover: 'Discover',
    diners: 'Diners Club',
    jcb: 'JCB',
    unionpay: 'UnionPay',
    maestro: 'Maestro',
    unknown: ''
  }

  const inRange = (n, lo, hi) => n >= lo && n <= hi

  /** Network from the number prefix. */
  function detectNetwork(digits) {
    const p = (n) => Number(digits.slice(0, n))
    if (/^(34|37)/.test(digits)) return 'amex'
    if (/^4/.test(digits)) return 'visa'
    if (inRange(p(2), 51, 55) || inRange(p(4), 2221, 2720)) return 'mastercard'
    if (/^6011/.test(digits) || /^65/.test(digits) || inRange(p(3), 644, 649) || inRange(p(6), 622126, 622925)) return 'discover'
    if (/^62/.test(digits)) return 'unionpay'
    if (inRange(p(4), 3528, 3589)) return 'jcb'
    if (inRange(p(3), 300, 305) || /^(36|38|39)/.test(digits)) return 'diners'
    if (/^(5018|5020|5038|5893|6304|6759|676[1-3])/.test(digits)) return 'maestro'
    return 'unknown'
  }

  /** Number with spaces, the way it is printed on the card (4-6-5 for Amex). */
  function groupCardNumber(digits, network) {
    const net = network === undefined ? detectNetwork(digits) : network
    if (net === 'amex' && digits.length === 15) return digits.slice(0, 4) + ' ' + digits.slice(4, 10) + ' ' + digits.slice(10)
    return digits.replace(/(\d{4})(?=\d)/g, '$1 ')
  }

  /** Four-digit year from a two or four-digit one; null if it makes no sense. */
  function fullYear(year) {
    if (!Number.isInteger(year) || year < 0) return null
    const y = year < 100 ? 2000 + year : year
    return y >= 2000 && y <= 2100 ? y : null
  }

  /** Combined expiry field: `MM/YY` by default, `MM/YYYY`, `MMYY`, `MM-YY`... following the placeholder or the max length. */
  function formatExpiry(month, year, hint) {
    const h = hint || {}
    const mm = String(month).padStart(2, '0')
    const full = String(fullYear(year) === null ? year : fullYear(year))
    const ph = h.placeholder || ''
    const max = h.maxLength && h.maxLength > 0 ? h.maxLength : 0
    const longYear = /y{4}|a{4}|\d{2}\s*[/-]\s*\d{4}/i.test(ph) || max === 7 || max === 6
    const sep = max === 4 || max === 6 ? '' : /\s\/\s/.test(ph) ? ' / ' : /-/.test(ph) ? '-' : '/'
    return mm + sep + (longYear ? full : full.slice(-2))
  }

  /** Year for its own field: two digits when the field only holds two, four otherwise. */
  function formatYear(year, hint) {
    const h = hint || {}
    const full = String(fullYear(year) === null ? year : fullYear(year))
    const short = h.maxLength === 2 || /^(y{2}|a{2})$/i.test((h.placeholder || '').trim())
    return short ? full.slice(-2) : full
  }

  // ───────────── values.ts ─────────────

  const LOCALES = ['en', 'fr']

  /** ISO 3166-1 alpha-2 codes (current countries and territories). */
  const CODES =
    'AD AE AF AG AI AL AM AO AQ AR AS AT AU AW AX AZ BA BB BD BE BF BG BH BI BJ BL BM BN BO BQ BR BS BT BV BW BY BZ CA CC CD CF CG CH CI CK CL CM CN CO CR CU CV CW CX CY CZ DE DJ DK DM DO DZ EC EE EG EH ER ES ET FI FJ FK FM FO FR GA GB GD GE GF GG GH GI GL GM GN GP GQ GR GS GT GU GW GY HK HM HN HR HT HU ID IE IL IM IN IO IQ IR IS IT JE JM JO JP KE KG KH KI KM KN KP KR KW KY KZ LA LB LC LI LK LR LS LT LU LV LY MA MC MD ME MF MG MH MK ML MM MN MO MP MQ MR MS MT MU MV MW MX MY MZ NA NC NE NF NG NI NL NO NP NR NU NZ OM PA PE PF PG PH PK PL PM PN PR PS PT PW PY QA RE RO RS RU RW SA SB SC SD SE SG SH SI SJ SK SL SM SN SO SR SS ST SV SX SY SZ TC TD TF TG TH TJ TK TL TM TN TO TR TT TV TW TZ UA UG UM US UY UZ VA VC VE VG VI VN VU WF WS YE YT ZA ZM ZW'.split(' ')

  /** Names of a country in the languages a form may use. */
  function countryNamesOf(code, extra) {
    if (!CODES.includes(code)) return []
    const out = []
    for (const locale of [...(extra || []), ...LOCALES]) {
      try {
        const name = new Intl.DisplayNames([locale], { type: 'region' }).of(code)
        if (name && name !== code && !out.includes(name)) out.push(name)
      } catch {
        // unknown locale
      }
    }
    return out
  }

  const joinName = (given, family) =>
    [given, family]
      .map((s) => s.trim())
      .filter(Boolean)
      .join(' ')
  const fullName = (a) => joinName(a.givenName, a.familyName)

  /** Text value for an address field. `multiline`: a textarea can hold several lines. */
  function addressValue(a, kind, opts) {
    const o = opts || {}
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
        return lines.join(o.multiline ? '\n' : ', ')
      case 'address-line1':
        return lines[0] || ''
      case 'address-line2':
        return lines[1] || ''
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
        return countryNamesOf(a.country, o.locales)[0] || a.country
      case 'tel':
        return a.phone
      case 'email':
        return a.email
      default:
        return ''
    }
  }

  /**
   * Picks the option that stands for one of the `candidates` (tried in order): the exact value or text first, then,
   * for candidates of 4+ letters, a text that starts with it. Returns the index, or -1.
   */
  function pickOption(options, candidates) {
    const norm = options.map((o) => ({ v: normalizeText(o.value), t: normalizeText(o.text) }))
    const wanted = candidates.map(normalizeText).filter(Boolean)
    for (const c of wanted) {
      const i = norm.findIndex((o) => o.v === c || o.t === c)
      if (i >= 0) return i
    }
    for (const c of wanted) {
      if (c.length < 4) continue
      const i = norm.findIndex((o) => o.t.startsWith(c + ' ') || o.t.startsWith(c + '(') || o.t.endsWith(' ' + c))
      if (i >= 0) return i
    }
    return -1
  }

  /** Candidates for a country or region select: the code and the names it goes by. */
  function placeCandidates(kind, a, locales) {
    if (kind === 'country' || kind === 'country-name') return [a.country, ...countryNamesOf(a.country, locales || [])].filter(Boolean)
    if (kind === 'region') return [a.region].filter(Boolean)
    return []
  }

  /** Month option: value or text `8`, `08`, or text that starts with it (`08 - August`). */
  function pickMonthOption(options, month) {
    const exact = pickOption(options, [String(month).padStart(2, '0'), String(month)])
    if (exact >= 0) return exact
    const re = new RegExp('^0*' + month + '(\\D|$)')
    return options.findIndex((o) => re.test(o.text.trim()) || re.test(o.value.trim()))
  }

  /** Candidates for a year select. */
  function yearCandidates(year) {
    const full = String(year)
    return [full, full.slice(-2)]
  }

  return Object.freeze({
    ADDRESS_KINDS, CARD_KINDS, NETWORK_NAMES,
    isCardKind, isAddressKind, parseAutocomplete, normalizeText, classifyField, classifyFields,
    isAddressForm, isCardForm, sameGroup,
    detectNetwork, groupCardNumber, fullYear, formatExpiry, formatYear,
    countryNamesOf, joinName, fullName, addressValue, pickOption, placeCandidates, pickMonthOption, yearCandidates
  })
})

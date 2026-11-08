'use strict'
// Port of shared/autofill/{classify,card,values}.test.ts for the parts the page script uses.
const test = require('node:test')
const assert = require('node:assert/strict')
const { load } = require('./harness')

const core = load('autofill-core.js')()
const { classifyField, classifyFields, isAddressForm, isCardForm, parseAutocomplete, sameGroup } = core
const { detectNetwork, formatExpiry, formatYear, fullYear, groupCardNumber } = core
const { addressValue, countryNamesOf, fullName, joinName, pickMonthOption, pickOption, placeCandidates } = core

const kind = (d) => {
  const c = classifyField({ type: 'text', ...d })
  return c ? c.kind : null
}

test('autocomplete tokens', () => {
  const same = ['name', 'given-name', 'family-name', 'organization', 'street-address', 'address-line1', 'address-line2', 'address-line3',
    'postal-code', 'country', 'country-name', 'tel', 'email', 'cc-name', 'cc-number', 'cc-exp', 'cc-exp-month', 'cc-exp-year', 'cc-csc', 'cc-type']
  for (const t of same) assert.equal(kind({ autocomplete: t }), t, t)
  assert.equal(kind({ autocomplete: 'address-level1' }), 'region')
  assert.equal(kind({ autocomplete: 'address-level2' }), 'city')
})

test('autocomplete wins over the name and label', () => {
  assert.equal(kind({ autocomplete: 'postal-code', name: 'city', label: 'Ville' }), 'postal-code')
})

test('section and shipping/billing prefixes, contact hints', () => {
  assert.deepEqual(parseAutocomplete('section-blue shipping street-address'), {
    section: 'section-blue', scope: 'shipping', kind: 'street-address', foreign: false
  })
  const b = parseAutocomplete('billing work tel')
  assert.equal(b.scope, 'billing')
  assert.equal(b.kind, 'tel')
  assert.deepEqual(classifyField({ type: 'text', autocomplete: 'section-a billing cc-number' }), {
    kind: 'cc-number', section: 'section-a', scope: 'billing', explicit: true
  })
})

test('inherited property names are not autocomplete tokens', () => {
  assert.equal(parseAutocomplete('constructor toString').kind, null)
  assert.equal(kind({ autocomplete: '__proto__', name: 'city' }), 'city')
})

test('other tokens leave the field alone', () => {
  assert.equal(kind({ autocomplete: 'username', name: 'email' }), null)
  assert.equal(kind({ autocomplete: 'one-time-code', name: 'code' }), null)
})

test('autocomplete off falls back to the heuristics', () => {
  assert.equal(kind({ autocomplete: 'off', name: 'city' }), 'city')
})

const HEURISTICS = [
  [{ name: 'firstName' }, 'given-name'], [{ name: 'last_name' }, 'family-name'], [{ name: 'fullname' }, 'name'],
  [{ label: 'Company' }, 'organization'], [{ name: 'address1' }, 'address-line1'], [{ name: 'address_line_2' }, 'address-line2'],
  [{ label: 'Apartment, suite' }, 'address-line2'], [{ label: 'Street address' }, 'address-line1'], [{ label: 'Address' }, 'street-address'],
  [{ name: 'billing_city' }, 'city'], [{ name: 'zip' }, 'postal-code'], [{ placeholder: 'ZIP code' }, 'postal-code'],
  [{ label: 'State / Province' }, 'region'], [{ label: 'Country' }, 'country'], [{ name: 'phone_number' }, 'tel'],
  [{ name: 'card_number' }, 'cc-number'], [{ label: 'Name on card' }, 'cc-name'], [{ name: 'cardholder' }, 'cc-name'],
  [{ label: 'Expiration date' }, 'cc-exp'], [{ placeholder: 'MM / YY' }, 'cc-exp'], [{ name: 'exp_month' }, 'cc-exp-month'],
  [{ name: 'expYear' }, 'cc-exp-year'], [{ name: 'cvv' }, 'cc-csc'], [{ label: 'Security code' }, 'cc-csc'],
  // French
  [{ label: 'Prénom' }, 'given-name'], [{ label: 'Nom de famille' }, 'family-name'], [{ label: 'Nom complet' }, 'name'],
  [{ label: 'Société' }, 'organization'], [{ label: 'Adresse' }, 'street-address'], [{ label: 'Adresse ligne 2' }, 'address-line2'],
  [{ label: 'Complément d’adresse' }, 'address-line2'], [{ label: 'Code postal' }, 'postal-code'], [{ name: 'codePostal' }, 'postal-code'],
  [{ label: 'Ville' }, 'city'], [{ label: 'Pays' }, 'country'], [{ label: 'Département' }, 'region'],
  [{ label: 'Téléphone' }, 'tel'], [{ label: 'Courriel' }, 'email'], [{ label: 'Numéro de carte' }, 'cc-number'],
  [{ label: 'Titulaire de la carte' }, 'cc-name'], [{ label: 'Date d’expiration' }, 'cc-exp'],
  [{ label: 'Mois d’expiration' }, 'cc-exp-month'], [{ label: 'Cryptogramme' }, 'cc-csc'], [{ label: 'Code de sécurité' }, 'cc-csc']
]
for (const [d, expected] of HEURISTICS) {
  test(`heuristic ${JSON.stringify(d)} is ${expected}`, () => assert.equal(kind(d), expected))
}

test('email and tel by input type', () => {
  assert.equal(classifyField({ type: 'email', name: 'x' }).kind, 'email')
  assert.equal(classifyField({ type: 'tel', name: 'x' }).kind, 'tel')
})

test('does not take a username or a file name for a person name', () => {
  assert.equal(kind({ name: 'username' }), null)
  assert.equal(kind({ name: 'filename' }), null)
  assert.equal(kind({ label: 'Company name' }), 'organization')
})

test('hidden, disabled and read-only fields are ignored', () => {
  for (const flag of ['hidden', 'disabled', 'readOnly']) assert.equal(classifyField({ type: 'text', name: 'city', [flag]: true }), null)
})

test('types that hold no address or card', () => {
  for (const type of ['password', 'checkbox', 'radio', 'hidden', 'file', 'submit', 'date']) assert.equal(classifyField({ type, name: 'city' }), null)
})

test('selects and textareas are fillable', () => {
  assert.equal(classifyField({ type: 'select', name: 'country' }).kind, 'country')
  assert.equal(classifyField({ type: 'textarea', name: 'address' }).kind, 'street-address')
})

const f = (name, extra = {}) => ({ type: 'text', name, ...extra })

test('an address form has at least three address fields, one of them a place', () => {
  assert.equal(isAddressForm(classifyFields([f('name'), f('street'), f('city'), f('zip')])), true)
  assert.equal(isAddressForm(classifyFields([f('name'), f('city')])), false)
  assert.equal(isAddressForm(classifyFields([f('name'), f('email'), f('phone')])), false)
})

test('hidden fields do not count', () => {
  assert.equal(isAddressForm(classifyFields([f('name'), f('street'), f('city', { hidden: true })])), false)
})

test('a card form has a card number', () => {
  assert.equal(isCardForm(classifyFields([f('card_number'), f('cvv')])), true)
  assert.equal(isCardForm(classifyFields([f('cvv'), f('exp_month')])), false)
})

test('loose month and year fields count in a card form only', () => {
  const card = classifyFields([f('card_number'), f('month'), f('year')])
  assert.deepEqual(card.map((c) => c && c.kind), ['cc-number', 'cc-exp-month', 'cc-exp-year'])
  assert.deepEqual(classifyFields([f('month'), f('year')]), [null, null])
  assert.equal(classifyFields([f('card_number'), f('birth_month')])[1], null)
})

test('"Nom" next to "Prénom" is the family name', () => {
  const [given, family] = classifyFields([f('x', { label: 'Prénom' }), f('y', { label: 'Nom' })])
  assert.equal(given.kind, 'given-name')
  assert.equal(family.kind, 'family-name')
  assert.equal(classifyFields([f('x', { label: 'Nom' })])[0].kind, 'name')
})

test('sections and shipping/billing groups fill separately', () => {
  const [a, b, c] = classifyFields([
    f('a', { autocomplete: 'shipping address-line1' }),
    f('b', { autocomplete: 'billing address-line1' }),
    f('c', { autocomplete: 'email' })
  ])
  assert.equal(sameGroup(a, b), false)
  assert.equal(sameGroup(a, c), true)
  const s1 = classifyField({ type: 'text', autocomplete: 'section-a address-level2' })
  const s2 = classifyField({ type: 'text', autocomplete: 'section-b address-level2' })
  assert.equal(sameGroup(s1, s2), false)
})

test('scope is guessed from the field name', () => {
  assert.equal(classifyField({ type: 'text', name: 'shipping_city' }).scope, 'shipping')
  assert.equal(classifyField({ type: 'text', name: 'billing_city' }).scope, 'billing')
})

// ───────────── card ─────────────

test('network from the prefix', () => {
  const cases = [
    ['4242424242424242', 'visa'], ['5555555555554444', 'mastercard'], ['2223003122003222', 'mastercard'], ['378282246310005', 'amex'],
    ['6011111111111117', 'discover'], ['6221260000000000', 'discover'], ['6200000000000005', 'unionpay'], ['3530111333300000', 'jcb'],
    ['30569309025904', 'diners'], ['6759649826438453', 'maestro'], ['9999999999999995', 'unknown']
  ]
  for (const [n, network] of cases) assert.equal(detectNetwork(n), network, n)
})

test('groups digits like the card (4-6-5 for Amex)', () => {
  assert.equal(groupCardNumber('4242424242424242'), '4242 4242 4242 4242')
  assert.equal(groupCardNumber('378282246310005'), '3782 822463 10005')
})

test('two-digit years are 20xx', () => {
  assert.equal(fullYear(27), 2027)
  assert.equal(fullYear(2027), 2027)
  assert.equal(fullYear(1999), null)
})

test('formats a combined expiry field after its hints', () => {
  assert.equal(formatExpiry(8, 2027), '08/27')
  assert.equal(formatExpiry(8, 2027, { placeholder: 'MM/YYYY' }), '08/2027')
  assert.equal(formatExpiry(8, 2027, { placeholder: 'MM / YY' }), '08 / 27')
  assert.equal(formatExpiry(8, 2027, { placeholder: 'MM-YY' }), '08-27')
  assert.equal(formatExpiry(8, 2027, { maxLength: 4 }), '0827')
  assert.equal(formatExpiry(8, 2027, { maxLength: 7 }), '08/2027')
  assert.equal(formatExpiry(8, 2027, { maxLength: 6 }), '082027')
})

test('formats a year field after its hints', () => {
  assert.equal(formatYear(2027), '2027')
  assert.equal(formatYear(2027, { maxLength: 2 }), '27')
  assert.equal(formatYear(2027, { placeholder: 'YY' }), '27')
  assert.equal(formatYear(2027, { maxLength: 4 }), '2027')
})

// ───────────── values ─────────────

const address = {
  givenName: 'Jeanne', familyName: 'Dupont', organization: 'Acme', street: '12 rue de la Paix\nBâtiment B\nEscalier 2',
  city: 'Paris', postalCode: '75002', region: 'Île-de-France', country: 'FR', phone: '+33 1 23 45 67 89', email: 'jeanne@example.com'
}

test('joinName skips empty parts', () => {
  assert.equal(joinName('Jeanne', 'Dupont'), 'Jeanne Dupont')
  assert.equal(joinName('Jeanne', ''), 'Jeanne')
})

test('address values map each kind', () => {
  assert.equal(addressValue(address, 'given-name'), 'Jeanne')
  assert.equal(addressValue(address, 'family-name'), 'Dupont')
  assert.equal(addressValue(address, 'address-line1'), '12 rue de la Paix')
  assert.equal(addressValue(address, 'address-line2'), 'Bâtiment B')
  assert.equal(addressValue(address, 'address-line3'), 'Escalier 2')
  assert.equal(addressValue(address, 'postal-code'), '75002')
  assert.equal(addressValue(address, 'country'), 'FR')
  assert.equal(addressValue(address, 'name'), 'Jeanne Dupont')
  assert.equal(fullName({ givenName: '', familyName: 'Dupont' }), 'Dupont')
})

test('a single name part fills only its own field', () => {
  const given = { ...address, familyName: '' }
  assert.equal(addressValue(given, 'given-name'), 'Jeanne')
  assert.equal(addressValue(given, 'family-name'), '')
  assert.equal(addressValue(given, 'name'), 'Jeanne')
  const family = { ...address, givenName: '' }
  assert.equal(addressValue(family, 'given-name'), '')
  assert.equal(addressValue(family, 'family-name'), 'Dupont')
  assert.equal(addressValue(family, 'name'), 'Dupont')
})

test('street in one field: lines joined, or kept in a textarea', () => {
  assert.equal(addressValue(address, 'street-address'), '12 rue de la Paix, Bâtiment B, Escalier 2')
  assert.equal(addressValue(address, 'street-address', { multiline: true }), '12 rue de la Paix\nBâtiment B\nEscalier 2')
})

test('country name in the form language', () => {
  assert.equal(addressValue(address, 'country-name', { locales: ['fr'] }), 'France')
})

test('names of a code', () => {
  assert.deepEqual(countryNamesOf('DE', ['fr']), ['Allemagne', 'Germany'])
  assert.deepEqual(countryNamesOf('zz'), [])
})

test('select option matching: countries', () => {
  const countries = [
    { value: '', text: 'Choose...' }, { value: 'US', text: 'United States' }, { value: 'FR', text: 'France' }, { value: 'DE', text: 'Germany' }
  ]
  assert.equal(pickOption(countries, placeCandidates('country', address)), 2)
  assert.equal(pickOption([{ value: '75', text: 'France' }], placeCandidates('country', address)), 0)
  assert.equal(pickOption([{ value: 'x', text: 'Allemagne' }], placeCandidates('country', { ...address, country: 'DE' }, ['fr'])), 0)
})

test('select option matching: regions, accents ignored', () => {
  const regions = [{ value: 'IDF', text: 'Ile-de-France' }, { value: 'BRE', text: 'Bretagne' }]
  assert.equal(pickOption(regions, placeCandidates('region', address)), 0)
  assert.equal(pickOption(regions, ['bre']), 1)
  assert.equal(pickOption(regions, ['Normandie']), -1)
})

test('a longer text starting with the candidate matches, a short code does not', () => {
  assert.equal(pickOption([{ value: '1', text: 'France (métropolitaine)' }], ['France']), 0)
  assert.equal(pickOption([{ value: '1', text: 'Frankreich' }], ['FR']), -1)
})

test('months and years', () => {
  const months = ['01', '02', '03', '04', '05', '06', '07', '08', '09', '10', '11', '12'].map((v) => ({ value: v, text: v }))
  assert.equal(pickMonthOption(months, 8), 7)
  assert.equal(pickMonthOption([{ value: '8', text: 'August' }], 8), 0)
  assert.equal(pickMonthOption([{ value: 'a', text: '08 - August' }], 8), 0)
  assert.equal(pickMonthOption([{ value: 'a', text: '10 - October' }], 1), -1)
  const years = [{ value: '2026', text: '2026' }, { value: '2027', text: '2027' }]
  assert.equal(pickOption(years, ['2027', '27']), 1)
  assert.equal(pickOption([{ value: '27', text: '27' }], ['2027', '27']), 0)
})

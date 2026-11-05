import { describe, expect, it } from 'vitest'
import { classifyField, classifyFields, isAddressForm, isCardForm, parseAutocomplete, sameGroup, type FieldDescriptor } from './classify'

const kind = (d: FieldDescriptor) => classifyField({ type: 'text', ...d })?.kind ?? null

describe('autocomplete tokens', () => {
  it.each([
    ['name', 'name'],
    ['given-name', 'given-name'],
    ['family-name', 'family-name'],
    ['organization', 'organization'],
    ['street-address', 'street-address'],
    ['address-line1', 'address-line1'],
    ['address-line2', 'address-line2'],
    ['address-line3', 'address-line3'],
    ['address-level1', 'region'],
    ['address-level2', 'city'],
    ['postal-code', 'postal-code'],
    ['country', 'country'],
    ['country-name', 'country-name'],
    ['tel', 'tel'],
    ['email', 'email'],
    ['cc-name', 'cc-name'],
    ['cc-number', 'cc-number'],
    ['cc-exp', 'cc-exp'],
    ['cc-exp-month', 'cc-exp-month'],
    ['cc-exp-year', 'cc-exp-year'],
    ['cc-csc', 'cc-csc'],
    ['cc-type', 'cc-type']
  ])('%s', (token, expected) => {
    expect(kind({ autocomplete: token })).toBe(expected)
  })

  it('autocomplete wins over the name and label', () => {
    expect(kind({ autocomplete: 'postal-code', name: 'city', label: 'Ville' })).toBe('postal-code')
  })

  it('section and shipping/billing prefixes, contact hints', () => {
    expect(parseAutocomplete('section-blue shipping street-address')).toEqual({
      section: 'section-blue',
      scope: 'shipping',
      kind: 'street-address',
      foreign: false
    })
    expect(parseAutocomplete('billing work tel')).toMatchObject({ scope: 'billing', kind: 'tel' })
    expect(classifyField({ type: 'text', autocomplete: 'section-a billing cc-number' })).toMatchObject({
      kind: 'cc-number',
      section: 'section-a',
      scope: 'billing',
      explicit: true
    })
  })

  it('other tokens leave the field alone (username, one-time-code)', () => {
    expect(kind({ autocomplete: 'username', name: 'email' })).toBeNull()
    expect(kind({ autocomplete: 'one-time-code', name: 'code' })).toBeNull()
  })

  it('autocomplete off falls back to the heuristics', () => {
    expect(kind({ autocomplete: 'off', name: 'city' })).toBe('city')
  })
})

describe('heuristics in English', () => {
  it.each([
    [{ name: 'firstName' }, 'given-name'],
    [{ name: 'last_name' }, 'family-name'],
    [{ name: 'fullname' }, 'name'],
    [{ label: 'Company' }, 'organization'],
    [{ name: 'address1' }, 'address-line1'],
    [{ name: 'address_line_2' }, 'address-line2'],
    [{ label: 'Apartment, suite' }, 'address-line2'],
    [{ label: 'Street address' }, 'address-line1'],
    [{ label: 'Address' }, 'street-address'],
    [{ name: 'billing_city' }, 'city'],
    [{ name: 'zip' }, 'postal-code'],
    [{ placeholder: 'ZIP code' }, 'postal-code'],
    [{ label: 'State / Province' }, 'region'],
    [{ label: 'Country' }, 'country'],
    [{ name: 'phone_number' }, 'tel'],
    [{ name: 'card_number' }, 'cc-number'],
    [{ label: 'Name on card' }, 'cc-name'],
    [{ name: 'cardholder' }, 'cc-name'],
    [{ label: 'Expiration date' }, 'cc-exp'],
    [{ placeholder: 'MM / YY' }, 'cc-exp'],
    [{ name: 'exp_month' }, 'cc-exp-month'],
    [{ name: 'expYear' }, 'cc-exp-year'],
    [{ name: 'cvv' }, 'cc-csc'],
    [{ label: 'Security code' }, 'cc-csc']
  ] as [FieldDescriptor, string][])('%j -> %s', (d, expected) => {
    expect(kind(d)).toBe(expected)
  })

  it('email and tel by input type', () => {
    expect(classifyField({ type: 'email', name: 'x' })?.kind).toBe('email')
    expect(classifyField({ type: 'tel', name: 'x' })?.kind).toBe('tel')
  })

  it('does not take a username or a file name for a person name', () => {
    expect(kind({ name: 'username' })).toBeNull()
    expect(kind({ name: 'filename' })).toBeNull()
    expect(kind({ label: 'Company name' })).toBe('organization')
  })
})

describe('heuristics in French', () => {
  it.each([
    [{ label: 'Prénom' }, 'given-name'],
    [{ label: 'Nom de famille' }, 'family-name'],
    [{ label: 'Nom complet' }, 'name'],
    [{ label: 'Société' }, 'organization'],
    [{ label: 'Adresse' }, 'street-address'],
    [{ label: 'Adresse ligne 2' }, 'address-line2'],
    [{ label: 'Complément d’adresse' }, 'address-line2'],
    [{ label: 'Code postal' }, 'postal-code'],
    [{ name: 'codePostal' }, 'postal-code'],
    [{ label: 'Ville' }, 'city'],
    [{ label: 'Pays' }, 'country'],
    [{ label: 'Département' }, 'region'],
    [{ label: 'Téléphone' }, 'tel'],
    [{ label: 'Courriel' }, 'email'],
    [{ label: 'Numéro de carte' }, 'cc-number'],
    [{ label: 'Titulaire de la carte' }, 'cc-name'],
    [{ label: 'Date d’expiration' }, 'cc-exp'],
    [{ label: 'Mois d’expiration' }, 'cc-exp-month'],
    [{ label: 'Cryptogramme' }, 'cc-csc'],
    [{ label: 'Code de sécurité' }, 'cc-csc']
  ] as [FieldDescriptor, string][])('%j -> %s', (d, expected) => {
    expect(kind(d)).toBe(expected)
  })
})

describe('ignored fields', () => {
  it('hidden, disabled and read-only fields', () => {
    expect(classifyField({ type: 'text', name: 'city', hidden: true })).toBeNull()
    expect(classifyField({ type: 'text', name: 'city', disabled: true })).toBeNull()
    expect(classifyField({ type: 'text', name: 'city', readOnly: true })).toBeNull()
  })

  it('types that hold no address or card', () => {
    for (const type of ['password', 'checkbox', 'radio', 'hidden', 'file', 'submit', 'date']) expect(classifyField({ type, name: 'city' })).toBeNull()
  })

  it('selects and textareas are fillable', () => {
    expect(classifyField({ type: 'select', name: 'country' })?.kind).toBe('country')
    expect(classifyField({ type: 'textarea', name: 'address' })?.kind).toBe('street-address')
  })
})

describe('form level', () => {
  const f = (name: string, extra: Partial<FieldDescriptor> = {}): FieldDescriptor => ({ type: 'text', name, ...extra })

  it('an address form has at least three address fields, one of them a place', () => {
    const address = classifyFields([f('name'), f('street'), f('city'), f('zip')])
    expect(isAddressForm(address)).toBe(true)
    expect(isAddressForm(classifyFields([f('name'), f('city')]))).toBe(false)
    // A sign-up form: name, email, phone, nothing about where.
    expect(isAddressForm(classifyFields([f('name'), f('email'), f('phone')]))).toBe(false)
  })

  it('hidden fields do not count', () => {
    expect(isAddressForm(classifyFields([f('name'), f('street'), f('city', { hidden: true })]))).toBe(false)
  })

  it('a card form has a card number', () => {
    expect(isCardForm(classifyFields([f('card_number'), f('cvv')]))).toBe(true)
    expect(isCardForm(classifyFields([f('cvv'), f('exp_month')]))).toBe(false)
  })

  it('loose month and year fields count in a card form only', () => {
    const card = classifyFields([f('card_number'), f('month'), f('year')])
    expect(card.map((c) => c?.kind)).toEqual(['cc-number', 'cc-exp-month', 'cc-exp-year'])
    const other = classifyFields([f('month'), f('year')])
    expect(other).toEqual([null, null])
    expect(classifyFields([f('card_number'), f('birth_month')])[1]).toBeNull()
  })

  it('"Nom" next to "Prénom" is the family name', () => {
    const [given, family] = classifyFields([f('x', { label: 'Prénom' }), f('y', { label: 'Nom' })])
    expect(given?.kind).toBe('given-name')
    expect(family?.kind).toBe('family-name')
    expect(classifyFields([f('x', { label: 'Nom' })])[0]?.kind).toBe('name')
  })

  it('sections and shipping/billing groups fill separately', () => {
    const [a, b, c] = classifyFields([
      f('a', { autocomplete: 'shipping address-line1' }),
      f('b', { autocomplete: 'billing address-line1' }),
      f('c', { autocomplete: 'email' })
    ])
    expect(sameGroup(a!, b!)).toBe(false)
    expect(sameGroup(a!, c!)).toBe(true)
    const s1 = classifyField({ type: 'text', autocomplete: 'section-a address-level2' })!
    const s2 = classifyField({ type: 'text', autocomplete: 'section-b address-level2' })!
    expect(sameGroup(s1, s2)).toBe(false)
  })

  it('scope is guessed from the field name', () => {
    expect(classifyField({ type: 'text', name: 'shipping_city' })?.scope).toBe('shipping')
    expect(classifyField({ type: 'text', name: 'billing_city' })?.scope).toBe('billing')
  })
})

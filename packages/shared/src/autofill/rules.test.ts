import { describe, expect, it } from 'vitest'
import type { AutofillAddress } from '../types/autofill'
import {
  canFill,
  cleanAddress,
  identicalAddress,
  mayOffer,
  mergeAddress,
  parseCardSubmission,
  planAddress,
  planCard,
  sameAddress,
  type OfferContext
} from './rules'

const typed = cleanAddress({
  givenName: 'Jeanne',
  familyName: 'Dupont',
  street: '12 rue de la Paix',
  city: 'Paris',
  postalCode: '75002',
  country: 'France',
  email: 'j@example.com'
})

const saved = (over: Partial<AutofillAddress> = {}): AutofillAddress => ({
  id: 'a1',
  ...typed,
  createdAt: 1,
  updatedAt: 1,
  useCount: 0,
  ...over
})

describe('canFill (https rule)', () => {
  it('addresses on http and https, cards on https or localhost only', () => {
    expect(canFill('address', 'https://shop.example')).toBe(true)
    expect(canFill('address', 'http://shop.example')).toBe(true)
    expect(canFill('card', 'https://shop.example')).toBe(true)
    expect(canFill('card', 'http://shop.example')).toBe(false)
    expect(canFill('card', 'http://localhost:3000')).toBe(true)
    expect(canFill('card', 'http://127.0.0.1:8080')).toBe(true)
    expect(canFill('card', 'http://[::1]:8080')).toBe(true)
    expect(canFill('card', 'http://app.localhost')).toBe(true)
    expect(canFill('card', 'http://localhost.evil.example')).toBe(false)
  })
  it('only http(s) pages', () => {
    for (const origin of ['file:///a', 'signetui://app', 'chrome-extension://abc', 'null', '', null, undefined]) {
      expect(canFill('address', origin)).toBe(false)
      expect(canFill('card', origin)).toBe(false)
    }
  })
})

describe('mayOffer (private windows, switches, never list)', () => {
  const base: OfferContext = { kind: 'address', origin: 'https://shop.example', isPrivate: false, enabled: undefined, never: undefined, encryption: true }
  it('offers by default', () => {
    expect(mayOffer(base)).toBe(true)
  })
  it('never in a private window', () => {
    expect(mayOffer({ ...base, isPrivate: true })).toBe(false)
    expect(mayOffer({ ...base, kind: 'card', isPrivate: true })).toBe(false)
  })
  it('respects the switches', () => {
    expect(mayOffer({ ...base, enabled: false })).toBe(false)
    expect(mayOffer({ ...base, enabled: true })).toBe(true)
  })
  it('respects the never list, for this origin only', () => {
    expect(mayOffer({ ...base, never: ['https://shop.example'] })).toBe(false)
    expect(mayOffer({ ...base, never: ['https://other.example'] })).toBe(true)
  })
  it('a card needs encryption and https', () => {
    expect(mayOffer({ ...base, kind: 'card', encryption: false })).toBe(false)
    expect(mayOffer({ ...base, kind: 'card', origin: 'http://shop.example' })).toBe(false)
    expect(mayOffer({ ...base, kind: 'card' })).toBe(true)
  })
})

describe('address offers', () => {
  it('clean: trims, clamps lines, turns the country into its code', () => {
    expect(typed.country).toBe('FR')
    expect(cleanAddress({ street: ' a \n\n b \nc\nd\ne' }).street).toBe('a\nb\nc\nd')
    expect(cleanAddress({ name: 5, city: null })).toMatchObject({ givenName: '', familyName: '' })
  })
  it('clean: separate names are kept as typed, a lone full name is split', () => {
    expect(cleanAddress({ givenName: ' Jeanne ', familyName: '' })).toMatchObject({ givenName: 'Jeanne', familyName: '' })
    expect(cleanAddress({ givenName: '', familyName: 'Dupont', name: 'Autre Nom' })).toMatchObject({ givenName: '', familyName: 'Dupont' })
    expect(cleanAddress({ name: 'Jeanne Marie Dupont' })).toMatchObject({ givenName: 'Jeanne', familyName: 'Marie Dupont' })
    expect(cleanAddress({ name: 'Dupont, Jeanne' })).toMatchObject({ givenName: 'Jeanne', familyName: 'Dupont' })
    expect(cleanAddress({ name: 'Jeanne' })).toMatchObject({ givenName: 'Jeanne', familyName: '' })
  })
  it('worth saving with name + street + city or postal code', () => {
    expect(planAddress([], typed)).toEqual({ action: 'save' })
    expect(planAddress([], { ...typed, city: '' })).toEqual({ action: 'save' })
    expect(planAddress([], { ...typed, city: '', postalCode: '' })).toEqual({ action: 'none' })
    expect(planAddress([], { ...typed, givenName: '', familyName: '' })).toEqual({ action: 'none' })
    expect(planAddress([], { ...typed, familyName: '' })).toEqual({ action: 'save' })
    expect(planAddress([], { ...typed, street: '' })).toEqual({ action: 'none' })
  })
  it('a known address (same name and street) is not offered again', () => {
    expect(planAddress([saved()], typed)).toEqual({ action: 'none' })
    expect(planAddress([saved()], { ...typed, givenName: '  jeanne ', familyName: ' DUPONT ', street: '12 Rue de la Paix' })).toEqual({ action: 'none' })
    // Same full name whether it came from two fields or one.
    expect(planAddress([saved()], cleanAddress({ name: 'jeanne  dupont', street: typed.street, city: 'Paris' }))).toEqual({ action: 'none' })
  })
  it('a changed field offers an update that keeps what was saved', () => {
    const plan = planAddress([saved({ phone: '0102030405' })], { ...typed, phone: '', postalCode: '75008' })
    expect(plan).toMatchObject({ action: 'update', id: 'a1' })
    if (plan.action === 'update') {
      expect(plan.merged.postalCode).toBe('75008')
      expect(plan.merged.phone).toBe('0102030405')
    }
  })
  it('another street is a new address', () => {
    expect(planAddress([saved()], { ...typed, street: '1 avenue Foch' })).toEqual({ action: 'save' })
    expect(sameAddress(typed, { ...typed, givenName: 'Paul' })).toBe(false)
    expect(identicalAddress(typed, mergeAddress(typed, { ...typed, city: 'Lyon' }))).toBe(false)
  })
})

describe('card offers', () => {
  const now = new Date(2026, 10, 1)
  const sub = { holder: ' J DUPONT ', number: '4242 4242 4242 4242', exp: '08/27' }
  it('reads number, network, holder and expiry', () => {
    expect(parseCardSubmission(sub, now)).toEqual({
      holder: 'J DUPONT',
      number: '4242424242424242',
      last4: '4242',
      network: 'visa',
      expMonth: 8,
      expYear: 2027
    })
  })
  it('separate month and year fields', () => {
    expect(parseCardSubmission({ number: '5555555555554444', expMonth: '12', expYear: '28' }, now)).toMatchObject({
      expMonth: 12,
      expYear: 2028,
      network: 'mastercard'
    })
  })
  it('refuses a bad Luhn number, a past expiry and a missing expiry', () => {
    expect(parseCardSubmission({ ...sub, number: '4242 4242 4242 4241' }, now)).toBeNull()
    expect(parseCardSubmission({ ...sub, exp: '10/26' }, now)).toBeNull()
    expect(parseCardSubmission({ ...sub, exp: '' }, now)).toBeNull()
    expect(parseCardSubmission({ number: 42 }, now)).toBeNull()
  })
  const known = { id: 'c1', number: '4242424242424242', holder: 'J Dupont', expMonth: 8, expYear: 2027 }
  const cand = parseCardSubmission(sub, now)!
  it('new number: save; same number and details: nothing; renewed expiry: update', () => {
    expect(planCard([], cand)).toEqual({ action: 'save' })
    expect(planCard([{ ...known, number: '5555555555554444' }], cand)).toEqual({ action: 'save' })
    expect(planCard([known], cand)).toEqual({ action: 'none' })
    expect(planCard([{ ...known, expYear: 2025 }], cand)).toEqual({ action: 'update', id: 'c1' })
    expect(planCard([{ ...known, number: null }], cand)).toEqual({ action: 'save' })
  })
})

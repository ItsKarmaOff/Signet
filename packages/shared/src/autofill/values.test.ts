import { afterEach, describe, expect, it, vi } from 'vitest'
import type { AddressInput } from '../types/autofill'
import {
  addressValue,
  countryCode,
  countryList,
  countryName,
  fullName,
  countryNamesOf,
  joinName,
  pickMonthOption,
  pickOption,
  placeCandidates,
  splitName
} from './values'

const address: AddressInput = {
  givenName: 'Jeanne',
  familyName: 'Dupont',
  organization: 'Acme',
  street: '12 rue de la Paix\nBâtiment B\nEscalier 2',
  city: 'Paris',
  postalCode: '75002',
  region: 'Île-de-France',
  country: 'FR',
  phone: '+33 1 23 45 67 89',
  email: 'jeanne@example.com'
}

describe('splitName', () => {
  it('splits on the first word', () => {
    expect(splitName('Jeanne Dupont')).toEqual({ given: 'Jeanne', family: 'Dupont' })
    expect(splitName('  Jean   Pierre  Martin ')).toEqual({ given: 'Jean', family: 'Pierre Martin' })
  })
  it('understands "Last, First" and single words', () => {
    expect(splitName('Dupont, Jeanne')).toEqual({ given: 'Jeanne', family: 'Dupont' })
    expect(splitName('Madonna')).toEqual({ given: 'Madonna', family: '' })
    expect(splitName('')).toEqual({ given: '', family: '' })
  })
  it('joinName skips empty parts', () => {
    expect(joinName('Jeanne', 'Dupont')).toBe('Jeanne Dupont')
    expect(joinName('Jeanne', '')).toBe('Jeanne')
  })
})

describe('address values', () => {
  it('maps each kind', () => {
    expect(addressValue(address, 'given-name')).toBe('Jeanne')
    expect(addressValue(address, 'family-name')).toBe('Dupont')
    expect(addressValue(address, 'address-line1')).toBe('12 rue de la Paix')
    expect(addressValue(address, 'address-line2')).toBe('Bâtiment B')
    expect(addressValue(address, 'address-line3')).toBe('Escalier 2')
    expect(addressValue(address, 'postal-code')).toBe('75002')
    expect(addressValue(address, 'country')).toBe('FR')
  })
  it('full name is the joined parts', () => {
    expect(addressValue(address, 'name')).toBe('Jeanne Dupont')
    expect(fullName({ givenName: '', familyName: 'Dupont' })).toBe('Dupont')
  })
  it('a single part fills only its own field, never the other', () => {
    const given = { ...address, familyName: '' }
    expect(addressValue(given, 'given-name')).toBe('Jeanne')
    expect(addressValue(given, 'family-name')).toBe('')
    expect(addressValue(given, 'name')).toBe('Jeanne')
    const family = { ...address, givenName: '' }
    expect(addressValue(family, 'given-name')).toBe('')
    expect(addressValue(family, 'family-name')).toBe('Dupont')
    expect(addressValue(family, 'name')).toBe('Dupont')
  })
  it('street in one field: lines joined, or kept in a textarea', () => {
    expect(addressValue(address, 'street-address')).toBe('12 rue de la Paix, Bâtiment B, Escalier 2')
    expect(addressValue(address, 'street-address', { multiline: true })).toBe('12 rue de la Paix\nBâtiment B\nEscalier 2')
  })
  it('country name in the form language', () => {
    expect(addressValue(address, 'country-name', { locales: ['fr'] })).toBe('France')
    expect(addressValue(address, 'country-name', { locales: ['en'] })).toBe('France')
  })
})

describe('countries', () => {
  it('codes and names, in English and French', () => {
    expect(countryCode('fr')).toBe('FR')
    expect(countryCode('France')).toBe('FR')
    expect(countryCode('Allemagne')).toBe('DE')
    expect(countryCode('United States')).toBe('US')
    expect(countryCode('États-Unis')).toBe('US')
    expect(countryCode('Nowhereland')).toBe('')
    expect(countryCode('')).toBe('')
  })
  it('names of a code', () => {
    expect(countryNamesOf('DE', ['fr'])).toEqual(['Allemagne', 'Germany'])
    expect(countryNamesOf('zz')).toEqual([])
  })
})

describe('countries without Intl.DisplayNames (Hermes)', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
    vi.resetModules()
  })

  async function withoutIntl() {
    vi.resetModules()
    vi.stubGlobal('Intl', { ...Intl, DisplayNames: undefined })
    return import('./values')
  }

  it('finds codes from English and French names', async () => {
    const v = await withoutIntl()
    expect(v.countryCode('France')).toBe('FR')
    expect(v.countryCode('Allemagne')).toBe('DE')
    expect(v.countryCode('etats-unis')).toBe('US')
    expect(v.countryCode('Nowhereland')).toBe('')
  })

  it('lists names and display names from the static table', async () => {
    const v = await withoutIntl()
    expect(v.countryNamesOf('DE')).toEqual(['Germany', 'Allemagne'])
    expect(v.countryName('DE', 'fr')).toBe('Allemagne')
    expect(v.countryName('DE', 'en-US')).toBe('Germany')
    expect(v.countryList('fr').find((c) => c.code === 'ES')?.name).toBe('Espagne')
  })
})

describe('country list', () => {
  it('has a sorted, complete list', () => {
    const list = countryList('en')
    expect(list.length).toBeGreaterThan(240)
    expect(list.find((c) => c.code === 'FR')?.name).toBe('France')
    expect(countryName('FR', 'fr')).toBe('France')
    expect(countryName('ZZ', 'en')).toBe('ZZ')
    expect(list[0].name.localeCompare(list[10].name)).toBeLessThan(0)
  })
})

describe('select option matching', () => {
  const countries = [
    { value: '', text: 'Choose...' },
    { value: 'US', text: 'United States' },
    { value: 'FR', text: 'France' },
    { value: 'DE', text: 'Germany' }
  ]
  it('country by code or by localized name', () => {
    expect(pickOption(countries, placeCandidates('country', address))).toBe(2)
    expect(pickOption([{ value: '75', text: 'France' }], placeCandidates('country', address))).toBe(0)
    expect(pickOption([{ value: 'x', text: 'Allemagne' }], placeCandidates('country', { ...address, country: 'DE' }, ['fr']))).toBe(0)
  })
  it('region by code or by name, accents ignored', () => {
    const regions = [
      { value: 'IDF', text: 'Ile-de-France' },
      { value: 'BRE', text: 'Bretagne' }
    ]
    expect(pickOption(regions, placeCandidates('region', address))).toBe(0)
    expect(pickOption(regions, ['bre'])).toBe(1)
    expect(pickOption(regions, ['Normandie'])).toBe(-1)
  })
  it('a longer text starting with the candidate matches, a short code does not', () => {
    expect(pickOption([{ value: '1', text: 'France (métropolitaine)' }], ['France'])).toBe(0)
    expect(pickOption([{ value: '1', text: 'Frankreich' }], ['FR'])).toBe(-1)
  })
  it('months and years', () => {
    const months = ['01', '02', '03', '04', '05', '06', '07', '08', '09', '10', '11', '12'].map((v) => ({ value: v, text: v }))
    expect(pickMonthOption(months, 8)).toBe(7)
    expect(pickMonthOption([{ value: '8', text: 'August' }], 8)).toBe(0)
    expect(pickMonthOption([{ value: 'a', text: '08 - August' }], 8)).toBe(0)
    expect(pickMonthOption([{ value: 'a', text: '10 - October' }], 1)).toBe(-1)
    const years = [
      { value: '2026', text: '2026' },
      { value: '2027', text: '2027' }
    ]
    expect(pickOption(years, ['2027', '27'])).toBe(1)
    expect(pickOption([{ value: '27', text: '27' }], ['2027', '27'])).toBe(0)
  })
})

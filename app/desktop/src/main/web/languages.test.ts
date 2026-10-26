import { describe, expect, it, vi } from 'vitest'

const system = vi.hoisted(() => ({ langs: ['fr-FR', 'en-US'] as string[] }))
vi.mock('electron', () => ({ app: { getPreferredSystemLanguages: () => system.langs, getPath: () => '' } }))

import { acceptLanguage, defaultLanguages, expandLanguages, normalizeLanguages } from './languages'

describe('expandLanguages', () => {
  it('adds the base language after the regional one, without duplicates', () => {
    expect(expandLanguages(['en-US', 'fr-FR'])).toEqual(['en-US', 'en', 'fr-FR', 'fr'])
    expect(expandLanguages(['en-US', 'en-GB'])).toEqual(['en-US', 'en', 'en-GB'])
  })
  it('normalizes _ to - and ignores invalid codes', () => {
    expect(expandLanguages(['pt_BR', 'x', '12', ''])).toEqual(['pt-BR', 'pt'])
  })
})

describe('defaultLanguages / normalizeLanguages', () => {
  it('follows the system', () => {
    system.langs = ['fr-FR', 'en-US']
    expect(defaultLanguages()).toEqual(['fr-FR', 'fr', 'en-US', 'en'])
  })
  it('English if the system says nothing', () => {
    system.langs = []
    expect(defaultLanguages()).toEqual(['en-US', 'en'])
  })
  it('normalizeLanguages keeps valid codes without expanding them, deduplicated', () => {
    expect(normalizeLanguages(['fr-FR', 'fr', 'fr', 'zz zz', 3])).toEqual(['fr-FR', 'fr'])
  })
  it('falls back to the system if empty or not a list', () => {
    system.langs = ['de-DE']
    expect(normalizeLanguages([])).toEqual(['de-DE', 'de'])
    expect(normalizeLanguages('fr')).toEqual(['de-DE', 'de'])
    expect(normalizeLanguages(undefined)).toEqual(['de-DE', 'de'])
  })
})

describe('acceptLanguage', () => {
  it('simple comma-separated list', () => {
    expect(acceptLanguage(['fr-FR', 'fr', 'en'])).toBe('fr-FR,fr,en')
  })
})

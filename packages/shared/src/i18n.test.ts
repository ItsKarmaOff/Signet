import { describe, expect, it } from 'vitest'
import { LOCALES, createI18n, platformKeys, resolveLocale } from './i18n'

describe('resolveLocale', () => {
  it('follows the explicit choice', () => {
    expect(resolveLocale('fr', ['en-US'])).toBe('fr')
    expect(resolveLocale('en', ['fr-FR'])).toBe('en')
  })

  it('takes the first supported system language', () => {
    expect(resolveLocale('system', ['de-DE', 'fr-CA', 'en'])).toBe('fr')
    expect(resolveLocale(undefined, ['FR_fr'])).toBe('fr')
  })

  it('falls back to English', () => {
    expect(resolveLocale('system', ['de', 'ja'])).toBe('en')
    expect(resolveLocale('system', [])).toBe('en')
  })

  it('LOCALES expose en et fr', () => {
    expect(LOCALES.map((l) => l.id)).toEqual(['en', 'fr'])
  })
})

describe('platformKeys', () => {
  it('changes nothing on Mac', () => {
    expect(platformKeys('⇧⌘T', true)).toBe('⇧⌘T')
  })

  it('changes nothing without symbols', () => {
    expect(platformKeys('Ctrl+T', false)).toBe('Ctrl+T')
  })

  it('translates Mac symbols elsewhere', () => {
    expect(platformKeys('⇧⌘T', false)).toBe('Ctrl+Shift+T')
    expect(platformKeys('⌥⌘B', false)).toBe('Ctrl+Alt+B')
    expect(platformKeys('⌘,', false)).toBe('Ctrl+,')
    expect(platformKeys('Ouvrir (⌘L)', false)).toBe('Ouvrir (Ctrl+L)')
  })

  it('ordonne Ctrl, Alt, Shift', () => {
    expect(platformKeys('⇧⌥⌃K', false)).toBe('Ctrl+Alt+Shift+K')
  })

  it('merges two Mac spellings that became identical', () => {
    expect(platformKeys('⌃⇧S ou ⌘⇧S', false)).toBe('Ctrl+Shift+S')
    expect(platformKeys('⌃⇧S or ⌘⇧S', false)).toBe('Ctrl+Shift+S')
  })

  it('remplace ⇥ par Tab', () => {
    expect(platformKeys('⌃⇥', false)).toBe('Ctrl+Tab')
  })
})

// `t` is typed on the real catalogs (i18next.d.ts): we loosen it to test with small resources.
type T = { t: (key: string, opts?: Record<string, unknown>) => string }

describe('createI18n', () => {
  const resources = {
    en: { ui: { hello: 'Hello {{name}}', only: 'Only EN', count_one: '{{count}} tab', count_other: '{{count}} tabs' } },
    fr: { ui: { hello: 'Bonjour {{name}}' } }
  }

  it('interpolates without escaping', () => {
    const i = createI18n('en', 'ui', resources) as unknown as T
    expect(i.t('hello', { name: '<b>' })).toBe('Hello <b>')
  })

  it('falls back to English for a missing key', () => {
    const i = createI18n('fr', 'ui', resources) as unknown as T
    expect(i.t('hello', { name: 'A' })).toBe('Bonjour A')
    expect(i.t('only')).toBe('Only EN')
  })

  it('handles plurals', () => {
    const i = createI18n('en', 'ui', resources) as unknown as T
    expect(i.t('count', { count: 1 })).toBe('1 tab')
    expect(i.t('count', { count: 3 })).toBe('3 tabs')
  })
})

import { describe, expect, it } from 'vitest'
import { APP_ICON_IDS, DEFAULT_MOBILE_SETTINGS, newTabUrl, patchSettings, sanitizeSettings } from './settings'

describe('settings', () => {
  it('missing or invalid values -> defaults', () => {
    expect(sanitizeSettings(undefined)).toEqual(DEFAULT_MOBILE_SETTINGS)
    expect(sanitizeSettings('x')).toEqual(DEFAULT_MOBILE_SETTINGS)
    const s = sanitizeSettings({ searchEngine: 'nope', theme: 3, language: 'de', contentBlocking: 'yes', haptics: false })
    expect(s.searchEngine).toBe('google')
    expect(s.theme).toBe('system')
    expect(s.language).toBe('system')
    expect(s.contentBlocking).toBe(true)
    expect(s.haptics).toBe(false)
  })
  it('keeps valid values and ignores unknown fields', () => {
    const s = sanitizeSettings({ searchEngine: 'qwant', theme: 'dark', language: 'fr', extra: 1 })
    expect(s).toMatchObject({ searchEngine: 'qwant', theme: 'dark', language: 'fr' })
    expect('extra' in s).toBe(false)
  })
  it('chosen home page: http(s) address required', () => {
    expect(sanitizeSettings({ homepage: 'custom', homepageUrl: 'javascript:1' }).homepage).toBe('start')
    const ok = sanitizeSettings({ homepage: 'custom', homepageUrl: ' https://a.com ' })
    expect(ok.homepage).toBe('custom')
    expect(newTabUrl(ok)).toBe('https://a.com')
    expect(newTabUrl(DEFAULT_MOBILE_SETTINGS)).toBe('')
  })
  it('app icon: known identifier only, otherwise aurore', () => {
    expect(DEFAULT_MOBILE_SETTINGS.appIcon).toBe('aurore')
    expect(sanitizeSettings({ appIcon: 'ocean' }).appIcon).toBe('ocean')
    expect(sanitizeSettings({ appIcon: 'inconnue' }).appIcon).toBe('aurore')
    expect(sanitizeSettings({ appIcon: 3 }).appIcon).toBe('aurore')
    expect(sanitizeSettings({ appIcon: 'nuit' }).appIcon).toBe('nuit')
    expect(patchSettings(sanitizeSettings({ appIcon: 'or' }), { appIcon: 'x' as never }).appIcon).toBe('or')
    expect(APP_ICON_IDS).toHaveLength(8)
  })
  it('patchSettings applies a partial update without touching the rest', () => {
    const s = patchSettings(DEFAULT_MOBILE_SETTINGS, { theme: 'light', searchEngine: 'brave' })
    expect(s).toMatchObject({ theme: 'light', searchEngine: 'brave', privateLock: true })
    expect(patchSettings(s, { theme: 'bad' as never }).theme).toBe('light')
  })
  it('search suggestions and password saving: on by default, booleans only', () => {
    expect(DEFAULT_MOBILE_SETTINGS.searchSuggestions).toBe(true)
    expect(DEFAULT_MOBILE_SETTINGS.offerToSavePasswords).toBe(true)
    expect(sanitizeSettings({ searchSuggestions: false, offerToSavePasswords: false })).toMatchObject({ searchSuggestions: false, offerToSavePasswords: false })
    expect(sanitizeSettings({ searchSuggestions: 'no', offerToSavePasswords: 0 })).toMatchObject({ searchSuggestions: true, offerToSavePasswords: true })
    expect(patchSettings(DEFAULT_MOBILE_SETTINGS, { searchSuggestions: false }).searchSuggestions).toBe(false)
  })
  it('master link switch: on by default, boolean only', () => {
    expect(DEFAULT_MOBILE_SETTINGS.linkEnabled).toBe(true)
    expect(sanitizeSettings({ linkEnabled: false }).linkEnabled).toBe(false)
    expect(sanitizeSettings({ linkEnabled: 'non' }).linkEnabled).toBe(true)
    expect(patchSettings(DEFAULT_MOBILE_SETTINGS, { linkEnabled: false }).linkEnabled).toBe(false)
    expect(patchSettings(sanitizeSettings({ linkEnabled: false }), { linkEnabled: 1 as never }).linkEnabled).toBe(false)
  })
})

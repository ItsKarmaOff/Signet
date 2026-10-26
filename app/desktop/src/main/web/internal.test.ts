import { beforeEach, describe, expect, it, vi } from 'vitest'

vi.mock('electron', () => ({
  app: { getPreferredSystemLanguages: () => ['en-US'], getPath: () => '', getName: () => 'Signet', getVersion: () => '1.0.0' },
  protocol: { registerSchemesAsPrivileged: () => {} },
  webContents: { getAllWebContents: () => [] }
}))

import { displayUrl, ENGINE_PAGES, guardNavigation, internalSuggestions, isEngineErrorPage, isInternalUrl, openShortcut, resolveInternal } from './internal'
import type { Browser } from '../browser/browser'

const ID = '123e4567-e89b-12d3-a456-426614174000'

describe("conversion d'adresses", () => {
  it('displayUrl: engine page -> signet://, trailing slash removed', () => {
    expect(displayUrl('chrome://gpu')).toBe('signet://gpu')
    expect(displayUrl('chrome://GPU/')).toBe('signet://gpu')
    expect(displayUrl('chrome://settings')).toBe('chrome://settings') // not in the list
    expect(displayUrl('signet://urls/')).toBe('signet://urls')
    expect(displayUrl('https://example.com/')).toBe('https://example.com/')
  })

  it('resolveInternal : inverse de displayUrl', () => {
    for (const name of ENGINE_PAGES) expect(resolveInternal(`signet://${name}`)).toBe(`chrome://${name}`)
    expect(resolveInternal('signet:gpu')).toBe('chrome://gpu')
    expect(resolveInternal('signet://GPU')).toBe('chrome://gpu')
    expect(resolveInternal('signet://urls')).toBe('signet://urls')
    expect(resolveInternal('https://example.com')).toBe('https://example.com')
  })

  it("resolveInternal : notes -> interface de l'app", () => {
    expect(resolveInternal(`signet://note/${ID}`)).toBe(`signetui://app/index.html#note/${ID}`)
    expect(resolveInternal(`signet://easel/${ID}`)).toBe(`signetui://app/index.html#easel/${ID}`)
  })

  it("displayUrl: a note's real address -> signet://", () => {
    expect(displayUrl(`signetui://app/index.html#note/${ID}`)).toBe(`signet://note/${ID}`)
  })

  it('isInternalUrl / isEngineErrorPage', () => {
    expect(isInternalUrl('signet://settings')).toBe(true)
    expect(isInternalUrl('signetu://x')).toBe(false)
    expect(isInternalUrl('signetui://app/index.html')).toBe(true)
    expect(isInternalUrl('chrome://gpu')).toBe(true)
    expect(isInternalUrl('chrome-untrusted://x')).toBe(true)
    expect(isInternalUrl('https://signet.com')).toBe(false)
    expect(isEngineErrorPage('chrome-error://chromewebdata/')).toBe(true)
    expect(isEngineErrorPage('chrome://gpu')).toBe(false)
  })
})

describe('internalSuggestions', () => {
  it('only for a signet: input', () => {
    expect(internalSuggestions('github')).toEqual([])
    expect(internalSuggestions('')).toEqual([])
  })
  it('filters by prefix, with or without //', () => {
    const a = internalSuggestions('signet:settings')
    expect(a.length).toBeGreaterThan(1)
    expect(a.every((r) => r.id.startsWith('url:signet://settings'))).toBe(true)
    expect(a[0]).toMatchObject({ kind: 'url', id: 'url:signet://settings', url: 'signet://settings' })
    expect(internalSuggestions('SIGNET://Settings/pri').map((r) => r.id)).toEqual(['url:signet://settings/privacy'])
  })
  it('signet:// seul liste tout', () => {
    const all = internalSuggestions('signet://')
    expect(all.map((r) => r.id)).toContain('url:signet://urls')
    expect(all.map((r) => r.id)).toContain('url:signet://gpu')
  })
})

describe('openShortcut / guardNavigation', () => {
  const settings = { open: vi.fn() }
  const library = { open: vi.fn() }
  const b = { settings, library } as unknown as Browser
  beforeEach(() => {
    settings.open.mockClear()
    library.open.mockClear()
  })

  it('Settings / Library shortcuts', () => {
    expect(openShortcut(b, 'signet://settings')).toBe(true)
    expect(settings.open).toHaveBeenLastCalledWith(undefined)
    expect(openShortcut(b, 'signet://settings/privacy')).toBe(true)
    expect(settings.open).toHaveBeenLastCalledWith('privacy')
    expect(openShortcut(b, 'signet://about')).toBe(true)
    expect(settings.open).toHaveBeenLastCalledWith('about')
    expect(openShortcut(b, 'signet://library/archive')).toBe(true)
    expect(library.open).toHaveBeenLastCalledWith('archive')
    expect(openShortcut(b, 'signet://library/inconnu')).toBe(true)
    expect(library.open).toHaveBeenLastCalledWith(undefined)
    expect(openShortcut(b, 'signet://history')).toBe(true)
    expect(library.open).toHaveBeenLastCalledWith('history')
  })

  it('not a shortcut', () => {
    expect(openShortcut(b, 'signet://urls')).toBe(false)
    expect(openShortcut(b, 'https://example.com')).toBe(false)
  })

  it('an ordinary web page cannot navigate to an internal address of the engine or the app', () => {
    const navigate = vi.fn()
    expect(guardNavigation(b, 'https://evil.com', 'chrome://gpu', navigate)).toBe(true)
    expect(guardNavigation(b, 'https://evil.com', 'signetui://app/index.html', navigate)).toBe(true)
    expect(navigate).not.toHaveBeenCalled()
  })

  it('an ordinary web page cannot navigate to signet://', () => {
    const navigate = vi.fn()
    expect(guardNavigation(b, 'https://evil.com', 'signet://settings', navigate)).toBe(true)
    expect(guardNavigation(b, 'https://evil.com', 'signet://version', navigate)).toBe(true)
    expect(settings.open).not.toHaveBeenCalled()
    expect(navigate).not.toHaveBeenCalled()
  })

  it('web address: not handled', () => {
    expect(guardNavigation(b, 'https://a.com', 'https://b.com', vi.fn())).toBe(false)
  })

  it('BUG? from an internal page, a chrome:// target is swallowed without navigation (parseSignet does not read it)', () => {
    const navigate = vi.fn()
    expect(guardNavigation(b, 'signet://urls', 'chrome://gpu', navigate)).toBe(true)
    expect(navigate).not.toHaveBeenCalled()
  })

  it('from an internal page: shortcut to settings, unknown page to not-found', () => {
    const navigate = vi.fn()
    expect(guardNavigation(b, 'signet://urls', 'signet://settings/privacy', navigate)).toBe(true)
    expect(settings.open).toHaveBeenCalled()
    expect(navigate).not.toHaveBeenCalled()
    expect(guardNavigation(b, 'signet://urls', 'signet://nimporte', navigate)).toBe(true)
    expect(navigate).toHaveBeenCalledWith('signet://not-found')
  })
})

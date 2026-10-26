import { describe, expect, it, vi } from 'vitest'

vi.mock('electron', () => ({
  app: { getPreferredSystemLanguages: () => ['en-US'], getPath: () => '', isPackaged: false },
  net: {},
  nativeTheme: {},
  webContents: { getAllWebContents: () => [] },
  protocol: {}
}))
// Depends on the machine's disk (installed browsers): neutralized to stay deterministic.
vi.mock('../features/importers', () => ({ availableSources: () => [] }))
vi.mock('../app/appicon', () => ({ iconPreview: () => 'data:icon', setAppIcon: () => {} }))
vi.mock('../features/boost/boost', () => ({ boostActions: () => [] }))

import type { Item, PaletteResult, Settings, TabInfo } from '@shared/types'
import type { Browser } from './browser'
import { paletteQuery } from './palette'
import { setSearchLanguage } from '../core/urls'

interface Fixture {
  tabs?: Record<string, Partial<TabInfo>>
  favorites?: string[]
  pinned?: Item[]
  today?: Item[]
  active?: string | null
  archive?: object[]
  history?: { url: string; title: string; favicon?: string }[]
  settings?: Partial<Settings>
}

function fake(f: Fixture = {}): Browser {
  const tabs = Object.fromEntries(Object.entries(f.tabs ?? {}).map(([id, t]) => [id, { id, url: `https://${id}.com`, title: id, lastActive: 0, ...t }]))
  const space = { id: 's1', name: 'Perso', icon: 'x', hue: 1, pinned: f.pinned ?? [], today: f.today ?? [], activeTabId: f.active ?? null }
  const b = {
    store: { state: { tabs, favorites: f.favorites ?? [], spaces: [space], archive: f.archive ?? [], boosts: {}, settings: { adblock: true, ...f.settings } } },
    space,
    layout: { horizontal: false },
    model: { activeTab: () => (f.active ? tabs[f.active] : undefined), locate: () => null },
    webApps: { names: () => [], appFor: () => null },
    link: { canOpenOnPhone: () => false },
    pageTools: { canSave: () => false },
    reader: { canRead: () => false },
    history: { search: (_q: string, exclude: Set<string>) => (f.history ?? []).filter((e) => !exclude.has(e.url)) }
  }
  return b as unknown as Browser
}

const kinds = (r: PaletteResult[]) => r.map((x) => x.kind)

describe('paletteQuery : saisie vide', () => {
  const tabs = { a: { lastActive: 10 }, b: { lastActive: 30 }, c: { lastActive: 20 }, d: { lastActive: 5 } }
  const items = (...ids: string[]): Item[] => ids.map((tabId) => ({ kind: 'tab', tabId }))

  it('edit-url : rien', () => {
    expect(paletteQuery(fake({ tabs }), '', 'edit-url')).toEqual([])
  })

  it('recent tabs, excluding the active tab, from most to least recent', () => {
    const b = fake({ tabs, today: items('a', 'b', 'c', 'd'), active: 'b' })
    const r = paletteQuery(b, '   ', 'default' as never)
    expect(r.map((x) => (x.kind === 'tab' ? x.tabId : ''))).toEqual(['c', 'a', 'd'])
    expect(r[0].subtitle).toContain('Perso')
  })

  it('new-tab: keeps the active tab and adds a few actions', () => {
    const b = fake({ tabs, today: items('a', 'b'), active: 'b' })
    const r = paletteQuery(b, '', 'new-tab')
    expect(r.filter((x) => x.kind === 'tab')).toHaveLength(2)
    expect(r.filter((x) => x.kind === 'action').map((x) => (x.kind === 'action' ? x.action : ''))).toEqual(['split', 'toggle-sidebar', 'little', 'reopen']) // order of buildActions
  })

  it('at most 6 recent tabs', () => {
    const many = Object.fromEntries(Array.from({ length: 10 }, (_, i) => [`t${i}`, { lastActive: i }]))
    const b = fake({ tabs: many, today: items(...Object.keys(many)) })
    expect(paletteQuery(b, '', 'default' as never)).toHaveLength(6)
  })
})

describe('paletteQuery : saisie', () => {
  it('address: offer to open, no search', () => {
    setSearchLanguage('')
    const r = paletteQuery(fake(), 'example.com', 'default' as never)
    expect(r[0]).toMatchObject({ kind: 'url', url: 'https://example.com', title: 'https://example.com' })
    expect(kinds(r)).not.toContain('search')
  })

  it('texte libre : recherche Google', () => {
    setSearchLanguage('')
    const r = paletteQuery(fake(), 'chats mignons', 'default' as never)
    expect(r[0]).toMatchObject({ kind: 'search', id: 'search:chats mignons', url: 'https://www.google.com/search?q=chats%20mignons' })
  })

  it('signet: internal addresses first', () => {
    const r = paletteQuery(fake(), 'signet://sett', 'default' as never)
    expect(r[0]).toMatchObject({ kind: 'url', url: 'signet://settings' })
    expect(r.some((x) => x.kind === 'url' && x.url === 'signet://settings/privacy')).toBe(true)
  })

  it('local names: direct target, no search', () => {
    const b = fake({ settings: { devAliases: [{ name: 'app', target: 'http://localhost:3000' }] } as Partial<Settings> })
    const r = paletteQuery(b, 'app', 'default' as never)
    expect(r[0]).toMatchObject({ kind: 'url', url: 'http://localhost:3000' })
    expect(kinds(r)).not.toContain('search')
    // prefix of a name: offered in addition to the search
    const r2 = paletteQuery(b, 'ap', 'default' as never)
    expect(r2.some((x) => x.id === 'alias:app')).toBe(true)
    expect(kinds(r2)).toContain('search')
  })

  it('open tabs found by title, custom title or address, accent-insensitive', () => {
    const b = fake({
      tabs: {
        a: { title: 'Le Café', url: 'https://x.com' },
        b: { title: 'rien', customTitle: 'Mon CAFÉ', url: 'https://y.com' },
        c: { title: 'z', url: 'https://cafe.org' },
        d: { title: 'autre' }
      },
      favorites: ['d'],
      today: ['a', 'b', 'c'].map((tabId) => ({ kind: 'tab', tabId }) as Item)
    })
    const r = paletteQuery(b, 'cafe', 'default' as never).filter((x) => x.kind === 'tab')
    expect(r.map((x) => (x.kind === 'tab' ? x.tabId : ''))).toEqual(['a', 'b', 'c'])
    expect(r[1].title).toBe('Mon CAFÉ')
  })

  it('space tabs and favorites named in the subtitle', () => {
    const b = fake({ tabs: { a: { title: 'foo' }, f: { title: 'foo fav' } }, favorites: ['f'], today: [{ kind: 'tab', tabId: 'a' }] })
    const r = paletteQuery(b, 'foo', 'default' as never).filter((x) => x.kind === 'tab')
    expect(r[0].subtitle).toContain('Favorites')
    expect(r[1].subtitle).toContain('Perso')
  })

  it('actions filtered by title', () => {
    const r = paletteQuery(fake(), 'settings', 'default' as never)
    expect(r.some((x) => x.kind === 'action' && x.action === 'settings')).toBe(true)
  })

  it('history: excludes already-open pages, no duplicates with tabs', () => {
    const b = fake({
      tabs: { a: { title: 'wiki', url: 'https://wiki.org' } },
      today: [{ kind: 'tab', tabId: 'a' }],
      history: [
        { url: 'https://wiki.org', title: 'wiki' },
        { url: 'https://wiki.org/2', title: 'wiki 2' }
      ]
    })
    const r = paletteQuery(b, 'wiki', 'default' as never)
    expect(r.filter((x) => x.kind === 'history').map((x) => (x.kind === 'history' ? x.url : ''))).toEqual(['https://wiki.org/2'])
  })

  it('archive: found, no duplicates with history', () => {
    const b = fake({
      history: [{ url: 'https://old.com', title: 'Old' }],
      archive: [
        { id: 'z', url: 'https://old.com', title: 'Old', archivedAt: 1 },
        { id: 'y', url: 'https://gone.com', title: 'Disparu old', archivedAt: 2 }
      ]
    })
    const r = paletteQuery(b, 'old', 'default' as never)
    expect(r.filter((x) => x.kind === 'archived').map((x) => x.id)).toEqual(['archived:y'])
    expect(r.filter((x) => x.kind === 'history')).toHaveLength(1)
  })

  it('at most 12 results', () => {
    const history = Array.from({ length: 30 }, (_, i) => ({ url: `https://h.com/${i}`, title: `h${i}` }))
    expect(paletteQuery(fake({ history }), 'h', 'default' as never)).toHaveLength(12)
  })

  it('icon actions only if the input is about icons', () => {
    expect(paletteQuery(fake(), 'icon', 'default' as never).some((x) => x.id.startsWith('action:icon:'))).toBe(true)
    expect(paletteQuery(fake(), 'xyz', 'default' as never).some((x) => x.id.startsWith('action:icon:'))).toBe(false)
  })

  it('go to a space', () => {
    const r = paletteQuery(fake(), 'perso', 'default' as never)
    expect(r.some((x) => x.kind === 'action' && x.action === 'space:s1')).toBe(true)
  })
})

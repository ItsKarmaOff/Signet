import { existsSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const env = vi.hoisted(() => ({ dir: '' }))
vi.mock('electron', () => ({
  app: { getPath: () => env.dir, getPreferredSystemLanguages: () => ['en-US'], userAgentFallback: '' },
  webContents: { getAllWebContents: () => [] }
}))

import type { Settings } from '@shared/types'
import {
  applyCleaned,
  ARCHIVE_MAX,
  cleanSettings,
  cleanSpaceDraft,
  DOWNLOADS_MAX,
  HISTORY_MAX_ENTRIES,
  newTab,
  SIDEBAR_MAX,
  SIDEBAR_MIN,
  Store,
  trimHistory,
  type HistoryEntry
} from './store'

let dir: string
beforeEach(() => {
  dir = mkdtempSync(join(tmpdir(), 'signet-store-'))
  env.dir = dir
})
afterEach(() => {
  vi.useRealTimers()
  rmSync(dir, { recursive: true, force: true })
})

const file = (name: string) => join(dir, name)
const write = (name: string, data: unknown) => writeFileSync(file(name), typeof data === 'string' ? data : JSON.stringify(data))
const read = (name: string) => JSON.parse(readFileSync(file(name), 'utf8'))
const open = () => new Store()
const tabRec = (id: string, url = 'https://example.com') => ({ id, url, title: 'T ' + id })
const files = (prefix: string) => readdirSync(dir).filter((f) => f.startsWith(prefix))

describe('Store: introduction', () => {
  it('fresh state: introduction to show', () => {
    expect(open().state.settings.onboarded).toBe(false)
  })
  it('existing state without the field: already seen', () => {
    write('state.json', { version: 1, settings: { adblock: true } })
    expect(open().state.settings.onboarded).toBe(true)
  })
  it('existing state where it is pending: kept', () => {
    write('state.json', { version: 1, settings: { onboarded: false } })
    expect(open().state.settings.onboarded).toBe(false)
  })
  it('corrupt file: no new introduction', () => {
    write('state.json', '{pas du json')
    expect(open().state.settings.onboarded).toBe(true)
  })
})

describe('Store: startup', () => {
  it('without a file: initial state', () => {
    const s = open()
    expect(s.state.version).toBe(1)
    expect(s.state.spaces).toHaveLength(1)
    expect(s.state.activeSpaceId).toBe(s.state.spaces[0].id)
    expect(s.state.spaces[0]).toMatchObject({ name: 'Personal', icon: '🏠', pinned: [], today: [] })
    expect(s.state.sidebarWidth).toBe(240)
    expect(s.state.settings).toMatchObject({ archiveAfterHours: 12, adblock: true, appIcon: 'aurore', routes: [] })
    expect(s.state.settings.languages).toEqual(['en-US', 'en'])
    expect(files('state.json.corrupt')).toEqual([])
  })

  it('corrupt JSON: copy aside then fresh state', () => {
    write('state.json', '{pas du json')
    const s = open()
    expect(s.state.spaces).toHaveLength(1)
    const copies = files('state.json.corrupt-')
    expect(copies).toHaveLength(1)
    expect(readFileSync(file(copies[0]), 'utf8')).toBe('{pas du json')
  })

  it('valid JSON but not an object: treated as corrupt', () => {
    write('state.json', 'null')
    const s = open()
    expect(s.state.spaces).toHaveLength(1)
  })

  it('loads a valid state', () => {
    write('state.json', {
      version: 1,
      tabs: { a: tabRec('a'), b: tabRec('b', 'https://b.org') },
      favorites: ['a'],
      spaces: [{ id: 's1', name: 'Work', icon: '💼', hue: 10, pinned: [{ kind: 'tab', tabId: 'a' }], today: [{ kind: 'tab', tabId: 'b' }], activeTabId: 'b' }],
      activeSpaceId: 's1',
      settings: { archiveAfterHours: 3, languages: ['fr-FR'] }
    })
    const s = open()
    expect(s.state.activeSpaceId).toBe('s1')
    expect(Object.keys(s.state.tabs).sort()).toEqual(['a', 'b'])
    expect(s.state.tabs.b).toMatchObject({ id: 'b', url: 'https://b.org', title: 'T b', loading: false, loaded: false })
    expect(s.state.favorites).toEqual(['a'])
    expect(s.state.settings.archiveAfterHours).toBe(3)
    expect(s.state.settings.languages).toEqual(['fr-FR'])
    expect(s.state.settings.adblock).toBe(true) // filled in by the defaults
    expect(s.state.spaces[0].activeTabId).toBe('b')
  })
})

describe('Store: normalization', () => {
  it('references to unknown tabs removed', () => {
    write('state.json', {
      tabs: { a: tabRec('a') },
      favorites: ['a', 'ghost'],
      spaces: [
        {
          id: 's1',
          name: 'S',
          icon: 'x',
          hue: 1,
          pinned: [
            { kind: 'tab', tabId: 'ghost' },
            { kind: 'split', id: 'sp', tabIds: ['a', 'ghost'] }
          ],
          today: [{ kind: 'tab', tabId: 'ghost' }],
          activeTabId: 'ghost'
        }
      ],
      activeSpaceId: 's1'
    })
    const s = open()
    const sp = s.state.spaces[0]
    expect(s.state.favorites).toEqual(['a'])
    expect(sp.pinned).toEqual([{ kind: 'tab', tabId: 'a' }]) // split view with 1 tab -> tab
    expect(sp.today).toEqual([])
    expect(sp.activeTabId).toBeNull()
  })

  it('today tabs: folders flattened; pinned: folders kept', () => {
    const folder = (id: string) => ({ kind: 'folder', id, name: 'F', open: true, items: [{ kind: 'tab', tabId: 'a' }] })
    write('state.json', {
      tabs: { a: tabRec('a') },
      spaces: [{ id: 's1', name: 'S', icon: 'x', hue: 1, pinned: [folder('p')], today: [folder('t')] }],
      activeSpaceId: 's1'
    })
    const s = open()
    expect(s.state.spaces[0].pinned[0]).toMatchObject({ kind: 'folder', id: 'p' })
    expect(s.state.spaces[0].today).toEqual([{ kind: 'tab', tabId: 'a' }])
  })

  it('groups: pinned without label, today synchronized', () => {
    write('state.json', {
      tabs: { a: tabRec('a'), b: tabRec('b'), c: tabRec('c') },
      spaces: [
        {
          id: 's1',
          name: 'S',
          icon: 'x',
          hue: 1,
          pinned: [{ kind: 'tab', tabId: 'a', group: 'g' }],
          today: [
            { kind: 'tab', tabId: 'b', group: 'h' },
            { kind: 'tab', tabId: 'c', group: 'h' }
          ]
        }
      ],
      activeSpaceId: 's1'
    })
    const sp = open().state.spaces[0]
    expect(sp.pinned).toEqual([{ kind: 'tab', tabId: 'a' }])
    expect(Object.keys(sp.groups!)).toEqual(['h'])
  })

  it('missing/empty/invalid spaces: default space; activeSpaceId fixed', () => {
    write('state.json', { tabs: {}, spaces: [], activeSpaceId: 'x' })
    const a = open()
    expect(a.state.spaces).toHaveLength(1)
    expect(a.state.activeSpaceId).toBe(a.state.spaces[0].id)
    write('state.json', { tabs: {}, spaces: [null, 3, { id: 's2', name: 'N', icon: 'i', hue: 2 }], activeSpaceId: 'zzz' })
    const b = open()
    expect(b.state.spaces.map((s) => s.id)).toEqual(['s2'])
    expect(b.state.activeSpaceId).toBe('s2')
    expect(b.state.spaces[0]).toMatchObject({ pinned: [], today: [] }) // missing lists filled in
  })

  it('sidebar width bounded', () => {
    write('state.json', { sidebarWidth: 5000 })
    expect(open().state.sidebarWidth).toBe(SIDEBAR_MAX)
    write('state.json', { sidebarWidth: 10 })
    expect(open().state.sidebarWidth).toBe(SIDEBAR_MIN)
    write('state.json', { sidebarWidth: 'abc' })
    expect(open().state.sidebarWidth).toBe(240)
  })

  it('downloads in progress -> interrupted; archive and downloads capped', () => {
    const downloads = Array.from({ length: DOWNLOADS_MAX + 5 }, (_, i) => ({
      id: String(i),
      status: i === 0 ? 'progressing' : i === 1 ? 'paused' : 'completed'
    }))
    write('state.json', { downloads: [...downloads, 'x'], archive: Array.from({ length: ARCHIVE_MAX + 10 }, (_, i) => ({ i })) })
    const s = open()
    expect(s.state.downloads).toHaveLength(DOWNLOADS_MAX)
    expect(s.state.downloads.slice(0, 3).map((d) => d.status)).toEqual(['interrupted', 'interrupted', 'completed'])
    expect(s.state.archive).toHaveLength(ARCHIVE_MAX)
    write('state.json', { downloads: 'oops', archive: 'oops' })
    const t = open()
    expect(t.state.downloads).toEqual([])
    expect(t.state.archive).toEqual([])
  })

  it('settings: unknown icon, invalid routes, inconsistent values', () => {
    write('state.json', {
      settings: { appIcon: 'inconnue', routes: 'x', archiveAfterHours: -5, tabSleepMinutes: 1.5, languages: [] },
      pipBounds: { x: 1, y: 2, width: NaN, height: 4 }
    })
    const s = open()
    expect(s.state.settings.appIcon).toBe('aurore')
    expect(s.state.settings.routes).toEqual([])
    expect(s.state.settings.archiveAfterHours).toBe(12)
    expect(s.state.settings.tabSleepMinutes).toBeUndefined()
    expect(s.state.settings.languages).toEqual(['en-US', 'en'])
    expect(s.state.pipBounds).toBeNull()
    write('state.json', { pipBounds: { x: 1, y: 2, width: 3, height: 4 } })
    expect(open().state.pipBounds).toEqual({ x: 1, y: 2, width: 3, height: 4 })
  })

  it('tabs: only known fields are kept', () => {
    write('state.json', {
      tabs: {
        a: {
          ...tabRec('a'),
          loading: true,
          audible: true,
          muted: 1,
          customTitle: 'C',
          customIcon: '🔥',
          pinnedUrl: 'https://p.com',
          lastActive: 42,
          inconnu: 1
        },
        bad: 'x'
      }
    })
    const t = open().state.tabs
    expect(Object.keys(t)).toEqual(['a'])
    expect(t.a).toMatchObject({ loading: false, audible: false, muted: true, customTitle: 'C', customIcon: '🔥', pinnedUrl: 'https://p.com', lastActive: 42 })
    expect('inconnu' in t.a).toBe(false)
  })

  it('tabHistory is removed from the state', () => {
    write('state.json', { tabs: { a: tabRec('a') }, tabHistory: { a: { entries: [{ url: 'https://x.com', title: '' }], index: 0 } } })
    expect('tabHistory' in open().state).toBe(false)
  })
})

describe('Store: bookmarks', () => {
  const link = (id: string, url = 'https://x.com') => ({ kind: 'link', id, url, title: id })
  const withSpace = (extra: object = {}) => ({
    tabs: {},
    spaces: [{ id: 's1', name: 'S', icon: 'x', hue: 1, pinned: [], today: [], ...extra }],
    activeSpaceId: 's1'
  })

  it('migration: bookmarks from state.json copied into bookmarks.json', () => {
    write('state.json', withSpace({ bookmarks: [link('b1')] }))
    const s = open()
    expect(s.state.spaces[0].bookmarks).toHaveLength(1)
    expect(read('bookmarks.json')).toEqual({ s1: [link('b1')] })
  })

  it('bookmarks.json is authoritative for the spaces it mentions', () => {
    write('state.json', withSpace({ bookmarks: [link('old')] }))
    write('bookmarks.json', { s1: [link('new')] })
    expect(open().state.spaces[0].bookmarks).toEqual([link('new')])
  })

  it('invalid bookmarks cleaned', () => {
    write('state.json', withSpace())
    write('bookmarks.json', { s1: [link('ok'), { kind: 'link', id: 'x' }, 5, { kind: 'folder', name: '', items: [link('in')] }] })
    const b = open().state.spaces[0].bookmarks!
    expect(b).toHaveLength(2)
    expect(b[1]).toMatchObject({ kind: 'folder', name: 'Folder', items: [link('in')] })
  })

  it('corrupt bookmarks.json: copied aside, state.json bookmarks kept and rewritten', () => {
    write('state.json', withSpace({ bookmarks: [link('b1')] }))
    write('bookmarks.json', '{{')
    const s = open()
    expect(files('bookmarks.json.corrupt-')).toHaveLength(1)
    expect(s.state.spaces[0].bookmarks).toHaveLength(1)
    expect(read('bookmarks.json')).toEqual({ s1: [link('b1')] })
  })

  it('unknown spaces: bookmarks kept (orphans) and rewritten at shutdown', () => {
    write('state.json', withSpace())
    write('bookmarks.json', { s1: [], lost: [link('keep')] })
    const s = open()
    expect(read('bookmarks.orphans.json')).toEqual({ lost: [link('keep')] })
    s.flush()
    expect(read('bookmarks.json').lost).toEqual([link('keep')])
  })
})

describe('Store: writing', () => {
  it('flush writes the state without bookmarks, detached tabs or private downloads', () => {
    write('state.json', {
      tabs: { a: tabRec('a'), b: tabRec('b') },
      spaces: [
        {
          id: 's1',
          name: 'S',
          icon: 'x',
          hue: 1,
          pinned: [],
          today: [{ kind: 'tab', tabId: 'a' }],
          bookmarks: [{ kind: 'link', id: 'l', url: 'https://x.com', title: 't' }]
        }
      ],
      activeSpaceId: 's1'
    })
    const s = open()
    s.detached.add('b')
    s.state.downloads.push({ id: 'd1', private: true } as never, { id: 'd2' } as never)
    s.flush()
    const saved = read('state.json')
    expect(Object.keys(saved.tabs)).toEqual(['a'])
    expect(saved.tabs.a).not.toHaveProperty('loading')
    expect(saved.spaces[0]).not.toHaveProperty('bookmarks')
    expect(saved.downloads.map((d: { id: string }) => d.id)).toEqual(['d2'])
    expect(read('bookmarks.json').s1).toHaveLength(1)
  })

  it('round trip: reloading the written state gives the same state', () => {
    const s = open()
    const tab = newTab('https://rt.com', 'RT')
    s.state.tabs[tab.id] = tab
    s.state.spaces[0].today.push({ kind: 'tab', tabId: tab.id })
    s.state.spaces[0].activeTabId = tab.id
    s.flush()
    const again = open()
    expect(again.state.spaces[0].today).toEqual([{ kind: 'tab', tabId: tab.id }])
    expect(again.state.tabs[tab.id]).toMatchObject({ url: 'https://rt.com', title: 'RT' })
    expect(again.state.spaces[0].activeTabId).toBe(tab.id)
  })

  it('schedule writes after the delay', async () => {
    vi.useFakeTimers()
    const s = open()
    s.schedule(100)
    vi.advanceTimersByTime(99)
    expect(existsSync(file('state.json'))).toBe(false)
    vi.advanceTimersByTime(2)
    vi.useRealTimers()
    await new Promise((r) => setTimeout(r, 80))
    expect(existsSync(file('state.json'))).toBe(true)
  })
})

describe('Store: per-tab histories', () => {
  const hist = {
    entries: [
      { url: 'https://a.com', title: 'A' },
      { url: 'https://b.com', title: 'B', pageState: 'xyz' }
    ],
    index: 1
  }

  it('migration of the old tabHistory field to tab-history.json', () => {
    write('state.json', { tabs: { a: tabRec('a') }, tabHistory: { a: hist, ghost: hist } })
    const s = open()
    expect(s.histories.get('a')).toEqual({
      entries: [
        { url: 'https://a.com', title: 'A', pageState: undefined },
        { url: 'https://b.com', title: 'B', pageState: 'xyz' }
      ],
      index: 1
    })
    expect(s.histories.has('ghost')).toBe(false)
    expect(Object.keys(read('tab-history.json'))).toEqual(['a'])
  })

  it('tab-history.json takes precedence; invalid entries ignored', () => {
    write('state.json', { tabs: { a: tabRec('a'), b: tabRec('b') } })
    write('tab-history.json', { a: hist, b: { entries: [{ url: '' }], index: 0 } })
    const s = open()
    expect(s.histories.has('a')).toBe(true)
    expect(s.histories.has('b')).toBe(false)
  })
})

describe('trimHistory', () => {
  const mk = (n: number, state = ''): HistoryEntry[] =>
    Array.from({ length: n }, (_, i) => ({ url: `https://x.com/${i}`, title: String(i), pageState: state || undefined }))

  it('empty or index out of bounds: null', () => {
    expect(trimHistory([], 0)).toBeNull()
    expect(trimHistory(mk(3), 3)).toBeNull()
    expect(trimHistory(mk(3), -1)).toBeNull()
  })

  it('keeps 50 entries around the active one', () => {
    const t = trimHistory(mk(200), 100)!
    expect(t.entries).toHaveLength(HISTORY_MAX_ENTRIES)
    expect(t.entries[t.index].url).toBe('https://x.com/100')
    expect(t.entries[0].url).toBe('https://x.com/75')
  })

  it('at the ends, the window stays full', () => {
    expect(trimHistory(mk(200), 0)!.index).toBe(0)
    const end = trimHistory(mk(200), 199)!
    expect(end.entries).toHaveLength(50)
    expect(end.index).toBe(49)
  })

  it('the pageState budget goes from the active entry outward', () => {
    const t = trimHistory(mk(5, 'x'.repeat(100)), 2, 250)!
    // active (100) + one neighbor (100) = 200; the next would exceed 250
    const kept = t.entries.map((e) => !!e.pageState)
    expect(kept.filter(Boolean)).toHaveLength(2)
    expect(t.entries[2].pageState).toBeDefined()
  })
})

describe('cleanSpaceDraft (translated default name)', () => {
  it('a space without a name takes the translated name', () => {
    expect(cleanSpaceDraft({} as never)).toEqual({ name: 'Personal', icon: '✨', hue: 250 })
  })
})

describe('cleanSettings / applyCleaned', () => {
  const base = { archiveAfterHours: 12, routes: [], languages: ['en'] } as unknown as Settings

  it('invalid value: the base one is kept', () => {
    expect(cleanSettings({ archiveAfterHours: -1 }, base).archiveAfterHours).toBe(12)
    expect(cleanSettings({ archiveAfterHours: NaN }, base).archiveAfterHours).toBe(12)
    expect(cleanSettings({ archiveAfterHours: 0 }, base).archiveAfterHours).toBe(0)
    expect(cleanSettings({ archiveAfterHours: 24 }, base).archiveAfterHours).toBe(24)
    expect(cleanSettings({ tabSleepMinutes: 2.5 }, base).tabSleepMinutes).toBeUndefined()
    expect(cleanSettings({ tabSleepMinutes: 30 }, base).tabSleepMinutes).toBe(30)
  })

  it('searchEngine: unknown identifier discarded, known one kept', () => {
    expect(cleanSettings({ searchEngine: 'bing' }, base).searchEngine).toBe('bing')
    expect(cleanSettings({ searchEngine: 'nope' as never }, base).searchEngine).toBeUndefined()
    const target = { ...base, searchEngine: 'nope' } as unknown as Settings
    applyCleaned(target, cleanSettings(target, base))
    expect(target.searchEngine).toBeUndefined()
  })

  it('downloadDir: absolute path kept, relative discarded, empty = system folder', () => {
    const abs = process.platform === 'win32' ? 'D:\\Fichiers' : '/Volumes/Disque/Fichiers'
    expect(cleanSettings({ downloadDir: abs }, base).downloadDir).toBe(abs)
    expect(cleanSettings({ downloadDir: 'relatif/dossier' }, base).downloadDir).toBeUndefined()
    expect(cleanSettings({ downloadDir: '' }, { ...base, downloadDir: abs }).downloadDir).toBeUndefined()
    // Absent from the patch: unchanged.
    expect(cleanSettings({}, { ...base, downloadDir: abs }).downloadDir).toBe(abs)
  })

  it('does not touch the other fields', () => {
    expect(cleanSettings({ adblock: false } as Partial<Settings>, base)).toEqual(base)
  })

  it('devAliases: valid name, normalized target, first one wins', () => {
    const out = cleanSettings(
      {
        devAliases: [
          { name: ' App ', target: 'localhost:3000' },
          { name: 'app', target: 'localhost:4000' },
          { name: 'bad name', target: 'localhost:1' },
          { name: 'x', target: 'https://evil.com' }
        ]
      },
      base
    ).devAliases
    expect(out).toHaveLength(1)
    expect(out![0].name).toBe('app')
    expect(cleanSettings({ devAliases: [] }, { ...base, devAliases: [{ name: 'a', target: 'localhost:1' }] } as Settings)).not.toHaveProperty('devAliases')
  })

  it('extensions: 32-letter a-p identifiers, no duplicates', () => {
    const id = 'a'.repeat(32)
    const out = cleanSettings({ extensionOrder: [id, id, 'court', 5 as never, 'q'.repeat(32)], pinnedExtensions: [id] }, base)
    expect(out.extensionOrder).toEqual([id])
    expect(out.pinnedExtensions).toEqual([id])
  })

  it('applyCleaned copies the validated fields and removes the absent ones', () => {
    const target = { archiveAfterHours: 1, tabSleepMinutes: 5, adblock: true } as unknown as Settings
    applyCleaned(target, { archiveAfterHours: 7 } as Settings)
    expect(target).toEqual({ archiveAfterHours: 7, adblock: true })
  })
})

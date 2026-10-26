import { mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { gzipSync } from 'node:zlib'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

vi.mock('electron', () => ({
  app: { getPath: () => '', getPreferredSystemLanguages: () => ['en-US'], getVersion: () => '1.0.0' },
  webContents: { getAllWebContents: () => [] },
  BrowserWindow: class {},
  dialog: {},
  safeStorage: {},
  shell: {}
}))

import type { Item, TabInfo } from '@shared/types'
import type { Browser } from '../browser/browser'
import { Backup, buildItems, cleanTab, itemsData, seal, tabData, unseal, urlsOf } from './backup'

const tabInfo = (id: string, extra: Partial<TabInfo> = {}): TabInfo => ({ id, url: `https://${id}.com`, title: id.toUpperCase(), ...extra }) as TabInfo

describe('seal / unseal', () => {
  it('round trip with the right password', () => {
    const value = { logins: [{ origin: 'https://a.com', username: 'u', password: 'é€' }] }
    const sealed = seal(value, 'correct horse')
    expect(sealed.kdf).toBe('scrypt')
    expect(JSON.stringify(sealed)).not.toContain('a.com')
    expect(unseal(sealed, 'correct horse')).toEqual(value)
  })

  it('wrong password or damaged block: null', () => {
    const sealed = seal({ a: 1 }, 'pw')
    expect(unseal(sealed, 'autre')).toBeNull()
    expect(unseal({ ...sealed, data: Buffer.from('x').toString('base64') }, 'pw')).toBeNull()
    expect(unseal({ ...sealed, tag: Buffer.alloc(16).toString('base64') }, 'pw')).toBeNull()
    expect(unseal({} as never, 'pw')).toBeNull()
  })

  it('different salt and IV at each encryption', () => {
    const a = seal(1, 'pw')
    const b = seal(1, 'pw')
    expect(a.salt).not.toBe(b.salt)
    expect(a.data).not.toBe(b.data)
  })
})

describe('tabData / itemsData', () => {
  it('tabData: optional fields only if they exist', () => {
    expect(tabData(undefined)).toBeNull()
    expect(tabData(tabInfo('a', { url: '' }))).toBeNull()
    expect(tabData(tabInfo('a'))).toEqual({ url: 'https://a.com', title: 'A' })
    expect(tabData(tabInfo('a', { favicon: 'f', pinnedUrl: 'p', customTitle: 'c', customIcon: 'i', muted: true }))).toEqual({
      url: 'https://a.com',
      title: 'A',
      favicon: 'f',
      pinnedUrl: 'p',
      customTitle: 'c',
      customIcon: 'i',
      muted: true
    })
  })

  it('itemsData: tabs in clear, split views of at least 2, recursive folders', () => {
    const tabs = { a: tabInfo('a'), b: tabInfo('b'), c: tabInfo('c') }
    const items: Item[] = [
      { kind: 'tab', tabId: 'a', group: 'g' },
      { kind: 'tab', tabId: 'ghost' },
      { kind: 'split', id: 's', tabIds: ['b', 'ghost'], layout: 'rows' },
      { kind: 'split', id: 's2', tabIds: ['b', 'c'], layout: 'rows', sizes: [1, 2] },
      { kind: 'folder', id: 'f', name: 'F', open: false, items: [{ kind: 'tab', tabId: 'c' }] }
    ]
    const out = itemsData(items, tabs)
    expect(out).toHaveLength(3)
    expect(out[0]).toEqual({ kind: 'tab', tab: { url: 'https://a.com', title: 'A' }, group: 'g' })
    expect(out[1]).toMatchObject({ kind: 'split', layout: 'rows', sizes: [1, 2] })
    expect(out[2]).toMatchObject({ kind: 'folder', name: 'F', open: false, items: [{ kind: 'tab' }] })
    expect(JSON.stringify(out)).not.toContain('tabId')
  })
})

describe('cleanTab', () => {
  it('rejects missing addresses or unexpected schemes', () => {
    expect(cleanTab(null)).toBeNull()
    expect(cleanTab({})).toBeNull()
    expect(cleanTab({ url: 'javascript:alert(1)' })).toBeNull()
    expect(cleanTab({ url: 'data:text/html,x' })).toBeNull()
    expect(cleanTab({ url: 'chrome://gpu' })).toBeNull()
  })

  it('accepts http(s), file and signet; bounds the fields', () => {
    expect(cleanTab({ url: 'signet://settings' })).toMatchObject({ url: 'signet://settings', title: '' })
    expect(cleanTab({ url: 'file:///x' })).not.toBeNull()
    const t = cleanTab({
      url: 'https://a.com',
      title: 't'.repeat(2000),
      customTitle: 'c'.repeat(300),
      customIcon: 'i'.repeat(40),
      muted: 'yes',
      pinnedUrl: 'javascript:1'
    })!
    expect(t.title).toHaveLength(1000)
    expect(t.customTitle).toHaveLength(200)
    expect(t.customIcon).toHaveLength(16)
    expect(t.muted).toBeUndefined() // only `true`
    expect(t.pinnedUrl).toBeUndefined()
    expect(cleanTab({ url: 'https://a.com', pinnedUrl: 'https://p.com', muted: true })).toMatchObject({ pinnedUrl: 'https://p.com', muted: true })
  })
})

describe('buildItems', () => {
  const data = (url: string) => ({ url, title: url })

  it('creates new tabs with new ids', () => {
    const tabs: Record<string, TabInfo> = {}
    const items = buildItems([{ kind: 'tab', tab: data('https://a.com'), group: 'g' }], tabs, false)
    expect(items).toHaveLength(1)
    const it0 = items[0] as Extract<Item, { kind: 'tab' }>
    expect(it0.group).toBe('g')
    expect(tabs[it0.tabId]).toMatchObject({ url: 'https://a.com', muted: false })
  })

  it('pinned: home address, no group, folders rebuilt', () => {
    const tabs: Record<string, TabInfo> = {}
    const items = buildItems(
      [
        { kind: 'tab', tab: data('https://a.com'), group: 'g' },
        {
          kind: 'folder',
          name: '',
          icon: '😀',
          open: false,
          hue: 400,
          live: { source: { kind: 'github', query: 'mine' }, junk: 1 },
          items: [{ kind: 'tab', tab: data('https://b.com') }]
        }
      ],
      tabs,
      true
    )
    expect(items[0]).not.toHaveProperty('group')
    const first = items[0] as Extract<Item, { kind: 'tab' }>
    expect(tabs[first.tabId].pinnedUrl).toBe('https://a.com')
    const folder = items[1] as Extract<Item, { kind: 'folder' }>
    expect(folder).toMatchObject({ name: 'New folder', icon: '😀', open: false, hue: 40, live: { source: { kind: 'github', query: 'mine' } } })
    expect(folder.live).not.toHaveProperty('junk')
    expect(folder.items).toHaveLength(1)
  })

  it('today: folders are ignored', () => {
    const tabs: Record<string, TabInfo> = {}
    expect(buildItems([{ kind: 'folder', name: 'F', items: [] }], tabs, false)).toEqual([])
  })

  it('split views: 2 to 4 valid tabs, known layout', () => {
    const tabs: Record<string, TabInfo> = {}
    const split = (n: number, layout?: string) => ({ kind: 'split', tabs: Array.from({ length: n }, (_, i) => data(`https://${i}.com`)), layout })
    expect(buildItems([split(1)], tabs, false)).toEqual([])
    const [s] = buildItems([split(6, 'grid')], tabs, false) as Extract<Item, { kind: 'split' }>[]
    expect(s.tabIds).toHaveLength(4)
    expect(s.layout).toBe('grid')
    const [u] = buildItems([split(2, 'bizarre')], tabs, false) as Extract<Item, { kind: 'split' }>[]
    expect(u.layout).toBeUndefined()
    expect(buildItems([{ kind: 'split', tabs: [data('https://a.com'), data('javascript:1')] }], tabs, false)).toEqual([])
  })

  it('invalid entries, missing list, depth and size bounded', () => {
    const tabs: Record<string, TabInfo> = {}
    expect(buildItems('x', tabs, false)).toEqual([])
    expect(buildItems([null, 3, { kind: 'tab' }, { kind: 'tab', tab: { url: 'javascript:1' } }, { kind: 'inconnu' }], tabs, false)).toEqual([])
    let deep: unknown = [{ kind: 'tab', tab: data('https://deep.com') }]
    for (let i = 0; i < 12; i++) deep = [{ kind: 'folder', name: 'd', items: deep }]
    const out = buildItems(deep, tabs, true)
    let depth = 0
    let cur: Item[] = out
    while (cur[0]?.kind === 'folder') {
      depth++
      cur = (cur[0] as Extract<Item, { kind: 'folder' }>).items
    }
    expect(depth).toBeLessThanOrEqual(9)
    expect(cur).toEqual([])
    const big = Array.from({ length: 2500 }, (_, i) => ({ kind: 'tab', tab: data(`https://x.com/${i}`) }))
    expect(buildItems(big, {}, false)).toHaveLength(2000)
  })
})

describe('urlsOf', () => {
  it('collects the addresses of all tabs, folders included', () => {
    const tabs = { a: tabInfo('a'), b: tabInfo('b'), c: tabInfo('c') }
    const items: Item[] = [
      { kind: 'tab', tabId: 'a' },
      { kind: 'split', id: 's', tabIds: ['b', 'ghost'] },
      { kind: 'folder', id: 'f', name: 'F', open: true, items: [{ kind: 'tab', tabId: 'c' }] }
    ]
    expect([...urlsOf(items, tabs)].sort()).toEqual(['', 'https://a.com', 'https://b.com', 'https://c.com'])
  })
})

describe('Backup.parse (reading a .signetbackup file)', () => {
  let dir: string
  beforeEach(() => {
    dir = mkdtempSync(join(tmpdir(), 'signet-backup-'))
  })
  afterEach(() => rmSync(dir, { recursive: true, force: true }))

  const parse = (path: string) => (new Backup({} as Browser) as unknown as { parse(p: string): unknown }).parse(path)
  const valid = { format: 'signetbackup', version: 1, createdAt: 1, appVersion: '1', sections: { favorites: [] } }
  const put = (name: string, data: string | Buffer) => {
    const p = join(dir, name)
    writeFileSync(p, data)
    return p
  }

  it('JSON compressed (gzip) or in clear', () => {
    expect(parse(put('a.signetbackup', gzipSync(JSON.stringify(valid))))).toEqual(valid)
    expect(parse(put('b.signetbackup', JSON.stringify(valid)))).toEqual(valid)
  })

  it('rejects another format, a future version, missing sections', () => {
    expect(parse(put('c', JSON.stringify({ ...valid, format: 'autre' })))).toBeNull()
    expect(parse(put('d', JSON.stringify({ ...valid, version: 2 })))).toBeNull()
    expect(parse(put('e', JSON.stringify({ ...valid, version: '1' })))).toBeNull()
    expect(parse(put('f', JSON.stringify({ ...valid, sections: [] })))).toBeNull()
    expect(parse(put('g', 'null'))).toBeNull()
  })

  it('unreadable, corrupt or missing file: null', () => {
    expect(parse(put('h', '{oops'))).toBeNull()
    expect(parse(put('i', Buffer.from([0x1f, 0x8b, 1, 2, 3])))).toBeNull()
    expect(parse(join(dir, 'absent'))).toBeNull()
  })
})

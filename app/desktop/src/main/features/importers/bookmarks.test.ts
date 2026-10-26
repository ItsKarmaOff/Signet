import { describe, expect, it, vi } from 'vitest'

vi.mock('electron', () => ({
  app: { getPreferredSystemLanguages: () => ['en-US'], getPath: () => '' },
  webContents: { getAllWebContents: () => [] }
}))

import { parseChromeBookmarks, parseSafariBookmarks, parseXmlPlist } from './bookmarks'

const url = (name: string, u: string) => ({ type: 'url', name, url: u })

describe('parseChromeBookmarks', () => {
  const json = {
    roots: {
      bookmark_bar: {
        children: [
          url('A', 'https://a.com'),
          { type: 'folder', name: 'Folder', children: [url('B', 'https://b.com'), url('JS', 'javascript:alert(1)')] },
          { type: 'folder', name: 'Vide', children: [] },
          url('', 'https://noname.com')
        ]
      },
      other: { children: [url('O', 'https://o.com')] },
      synced: { children: [] }
    }
  }

  it('root folder named after the browser, other bookmarks in a subfolder', () => {
    const out = parseChromeBookmarks(json, 'Brave')!
    expect(out).toMatchObject({ kind: 'folder', name: 'Brave' })
    if (out.kind !== 'folder') throw new Error()
    expect(out.items.map((n) => n.kind)).toEqual(['tab', 'folder', 'tab', 'folder'])
    expect(out.items[1]).toMatchObject({ name: 'Folder', items: [{ kind: 'tab', url: 'https://b.com' }] }) // javascript: filtered
    expect(out.items[2]).toMatchObject({ title: 'https://noname.com' }) // empty name -> URL
    expect(out.items[3]).toMatchObject({ kind: 'folder', name: 'Other Bookmarks' })
  })

  it('nothing to import: null; unknown format: error', () => {
    expect(parseChromeBookmarks({ roots: { bookmark_bar: { children: [] } } }, 'Chrome')).toBeNull()
    expect(() => parseChromeBookmarks({}, 'Chrome')).toThrow()
    expect(() => parseChromeBookmarks(null, 'Chrome')).toThrow()
  })

  it('limited depth (no stack overflow)', () => {
    let node: unknown = url('deep', 'https://deep.com')
    for (let i = 0; i < 100; i++) node = { type: 'folder', name: 'd', children: [node] }
    expect(parseChromeBookmarks({ roots: { bookmark_bar: { children: [node] } } }, 'C')).toBeNull()
  })
})

describe('parseSafariBookmarks', () => {
  const leaf = (title: string, u: string) => ({ WebBookmarkType: 'WebBookmarkTypeLeaf', URLString: u, URIDictionary: { title } })
  const list = (Title: string, Children: unknown[]) => ({ WebBookmarkType: 'WebBookmarkTypeList', Title, Children })

  it('favorites bar + menu, reading list ignored', () => {
    const out = parseSafariBookmarks({
      Children: [
        list('BookmarksBar', [
          leaf('A', 'https://a.com'),
          list('Sous', [leaf('B', 'https://b.com')]),
          list('com.apple.ReadingList', [leaf('R', 'https://r.com')])
        ]),
        list('BookmarksMenu', [leaf('M', 'https://m.com')])
      ]
    })!
    expect(out).toMatchObject({ kind: 'folder', name: 'Safari' })
    if (out.kind !== 'folder') throw new Error()
    expect(out.items).toHaveLength(3)
    expect(out.items[1]).toMatchObject({ kind: 'folder', name: 'Sous' })
    expect(out.items[2]).toMatchObject({ kind: 'folder', name: 'Bookmarks Menu', items: [{ url: 'https://m.com' }] })
  })

  it('unknown format: error; empty: null', () => {
    expect(() => parseSafariBookmarks({})).toThrow()
    expect(parseSafariBookmarks({ Children: [] })).toBeNull()
  })
})

describe('parseXmlPlist', () => {
  const plist = `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>Title</key><string>A &amp; B</string>
  <key>Count</key><integer>3</integer>
  <key>Ratio</key><real>1.5</real>
  <key>On</key><true/>
  <key>Off</key><false/>
  <key>Empty</key><string/>
  <key>Children</key><array><dict><key>URLString</key><string>https://a.com</string></dict><array/></array>
  <key>Nested</key><dict/>
</dict></plist>`

  it('dictionaries, arrays, numbers, booleans, entities', () => {
    expect(parseXmlPlist(plist)).toEqual({
      Title: 'A & B',
      Count: 3,
      Ratio: 1.5,
      On: true,
      Off: false,
      Empty: '',
      Children: [{ URLString: 'https://a.com' }, []],
      Nested: {}
    })
  })

  it('empty entry: null', () => {
    expect(parseXmlPlist('')).toBeNull()
  })
})

import { describe, expect, it } from 'vitest'
import { exportBookmarksHtml, parseBookmarksHtml } from './bookmarks-html'
import type { Bookmark } from '../types'

const tree: Bookmark[] = [
  { kind: 'link', id: 'a', title: 'Tom & "Jerry" <b>', url: 'https://example.com/?a=1&b=2' },
  {
    kind: 'folder',
    id: 'f',
    name: 'Work',
    items: [
      { kind: 'link', id: 'b', title: 'Docs', url: 'https://docs.example.com' },
      { kind: 'folder', id: 'g', name: 'Deep', items: [] }
    ]
  },
  { kind: 'link', id: 'c', title: 'Bad', url: 'javascript:alert(1)' }
]

const strip = (nodes: Bookmark[]): unknown[] => nodes.map((n) => (n.kind === 'link' ? { title: n.title, url: n.url } : { name: n.name, items: strip(n.items) }))

describe('bookmarks html', () => {
  it('exports the Netscape format and skips non-web links', () => {
    const html = exportBookmarksHtml(tree, 1_700_000_000_000)
    expect(html.startsWith('<!DOCTYPE NETSCAPE-Bookmark-file-1>')).toBe(true)
    expect(html).toContain('<H3 ADD_DATE="1700000000">Work</H3>')
    expect(html).toContain('HREF="https://example.com/?a=1&amp;b=2"')
    expect(html).toContain('Tom &amp; &quot;Jerry&quot; &lt;b&gt;')
    expect(html).not.toContain('javascript:')
  })

  it('round-trips folders, links and escaped text', () => {
    const back = parseBookmarksHtml(exportBookmarksHtml(tree))
    expect(strip(back)).toEqual([
      { title: 'Tom & "Jerry" <b>', url: 'https://example.com/?a=1&b=2' },
      {
        name: 'Work',
        items: [
          { title: 'Docs', url: 'https://docs.example.com' },
          { name: 'Deep', items: [] }
        ]
      }
    ])
  })

  it('reads a browser-style file with lowercase tags and single quotes', () => {
    const html = `<dl><p><dt><h3>News</h3><dl><p><dt><a href='https://a.test/x'>A &#38; B</a><dt><a href="ftp://nope">ftp</a></dl><p></dl>`
    expect(strip(parseBookmarksHtml(html))).toEqual([{ name: 'News', items: [{ title: 'A & B', url: 'https://a.test/x' }] }])
  })

  it('gives fresh unique ids and returns nothing for garbage', () => {
    const back = parseBookmarksHtml(exportBookmarksHtml(tree))
    expect(back[0].id).not.toBe('a')
    expect(parseBookmarksHtml('not html at all')).toEqual([])
  })

  it('flattens folders nested deeper than the limit', () => {
    let html = ''
    for (let i = 0; i < 10; i++) html += `<DT><H3>L${i}</H3><DL><p>`
    html += '<DT><A HREF="https://deep.test">deep</A>' + '</DL><p>'.repeat(10)
    let depth = 0
    let node: Bookmark | undefined = parseBookmarksHtml(html)[0]
    while (node?.kind === 'folder') {
      depth++
      node = node.items.find((n) => n.kind === 'folder') ?? node.items[0]
    }
    expect(depth).toBeLessThanOrEqual(6)
  })
})

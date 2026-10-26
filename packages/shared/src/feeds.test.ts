import { describe, expect, it } from 'vitest'
import { attributes, cleanLiveFolder, cleanLiveSource, decodeEntities, discoverFeed, looksLikeHtml, parseFeed } from './feeds'

describe('decodeEntities', () => {
  it('decodes named, decimal and hexadecimal', () => {
    expect(decodeEntities('a &amp; b &lt;c&gt; &quot;d&quot; &#39;e&#39; &#x2019;')).toBe('a & b <c> "d" \'e\' ’')
  })
  it('leaves unknown or invalid entities alone', () => {
    expect(decodeEntities('&foo; &#0; &#x110000;')).toBe('&foo; &#0; &#x110000;')
  })
})

describe('attributes', () => {
  it('reads double, single and no quotes', () => {
    expect(attributes(' rel="alternate" type=\'a/b\' href=/x?a=1&amp;b=2 ')).toEqual({ rel: 'alternate', type: 'a/b', href: '/x?a=1&b=2' })
  })
})

const RSS = `<?xml version="1.0"?>
<rss version="2.0"><channel>
  <title>Mon &amp; Blog</title>
  <item><title><![CDATA[Premier <b>billet</b>]]></title><link>/p/1</link><pubDate>Mon, 01 Jan 2024 10:00:00 GMT</pubDate></item>
  <item><title>Sans lien</title><guid>https://example.com/guid</guid></item>
  <item><title>Rien</title></item>
  <item><link>javascript:alert(1)</link><title>Mauvais</title></item>
  <item><link>https://ex.com/3</link></item>
</channel></rss>`

const ATOM = `<feed xmlns="http://www.w3.org/2005/Atom"><title>Atom</title>
  <entry><title>Un</title><link rel="self" href="/self"/><link rel="alternate" href="/un"/><updated>2024-02-03T04:05:06Z</updated></entry>
  <entry><title>Deux</title><link href="https://ex.com/deux"/></entry>
  <entry><title>Sans lien</title></entry>
</feed>`

describe('parseFeed', () => {
  it('RSS: title, relative links resolved, fallback guid, dates', () => {
    const f = parseFeed(RSS, 'https://example.com/feed')!
    expect(f.title).toBe('Mon & Blog')
    expect(f.entries).toEqual([
      { url: 'https://example.com/p/1', title: 'Premier billet', date: Date.parse('2024-01-01T10:00:00Z') },
      { url: 'https://example.com/guid', title: 'Sans lien', date: 0 },
      { url: 'https://ex.com/3', title: 'https://ex.com/3', date: 0 }
    ])
  })

  it('Atom: prefers rel=alternate, ignores entries without a link', () => {
    const f = parseFeed(ATOM, 'https://example.com/')!
    expect(f.title).toBe('Atom')
    expect(f.entries).toEqual([
      { url: 'https://example.com/un', title: 'Un', date: Date.parse('2024-02-03T04:05:06Z') },
      { url: 'https://ex.com/deux', title: 'Deux', date: 0 }
    ])
  })

  it('returns null if it is not a feed', () => {
    expect(parseFeed('<html><body>hi</body></html>', 'https://e.com')).toBeNull()
    expect(parseFeed('', 'https://e.com')).toBeNull()
  })

  it('empty but valid feed', () => {
    expect(parseFeed('<rss><channel><title>T</title></channel></rss>', 'https://e.com')).toEqual({ title: 'T', entries: [] })
  })
})

describe('discoverFeed / looksLikeHtml', () => {
  it('finds the announced feed', () => {
    const html = '<head><link rel="stylesheet" href="/a.css"><link rel="alternate" type="application/atom+xml" href="/feed.xml"></head>'
    expect(discoverFeed(html, 'https://e.com/blog/')).toBe('https://e.com/feed.xml')
    expect(discoverFeed('<link rel="alternate" type="text/html" href="/x">', 'https://e.com')).toBeNull()
    expect(discoverFeed('<link rel="alternate" type="application/rss+xml" href="javascript:1">', 'https://e.com')).toBeNull()
  })

  it('looksLikeHtml', () => {
    expect(looksLikeHtml('  <!DOCTYPE html><html>')).toBe(true)
    expect(looksLikeHtml('<HTML lang="fr">')).toBe(true)
    expect(looksLikeHtml('<?xml version="1.0"?><rss>')).toBe(false)
  })
})

describe('cleanLiveSource / cleanLiveFolder', () => {
  it('only valid feed or github source', () => {
    expect(cleanLiveSource({ kind: 'feed', url: ' https://e.com/rss ' })).toEqual({ kind: 'feed', url: 'https://e.com/rss' })
    expect(cleanLiveSource({ kind: 'feed', url: 'ftp://e.com' })).toBeNull()
    expect(cleanLiveSource({ kind: 'feed', url: 'https://e.com a' })).toBeNull()
    expect(cleanLiveSource({ kind: 'github', query: 'mine' })).toEqual({ kind: 'github', query: 'mine' })
    expect(cleanLiveSource({ kind: 'github', query: 'other' })).toBeNull()
    expect(cleanLiveSource(null)).toBeNull()
    expect(cleanLiveSource('x')).toBeNull()
  })

  it('dossier : retire refreshing, borne error et seen', () => {
    const seen = Array.from({ length: 250 }, (_, i) => `https://e.com/${i}`)
    const out = cleanLiveFolder({ source: { kind: 'github', query: 'mine' }, refreshing: true, refreshedAt: 9, error: 'x'.repeat(500), seen: [...seen, 4] })!
    expect(out.refreshing).toBeUndefined()
    expect(out.refreshedAt).toBe(9)
    expect(out.error).toHaveLength(300)
    expect(out.seen).toHaveLength(200)
    expect(out.seen![199]).toBe('https://e.com/249')
    expect(cleanLiveFolder({ source: { kind: 'nope' } })).toBeUndefined()
    expect(cleanLiveFolder(undefined)).toBeUndefined()
  })
})

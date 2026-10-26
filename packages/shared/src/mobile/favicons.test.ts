import { describe, expect, it } from 'vitest'
import { iconChain, parseIconSize, parseIconsMessage, pickBestIcon } from './favicons'

describe('favicons', () => {
  it('parses sizes', () => {
    expect(parseIconSize('32x32')).toBe(32)
    expect(parseIconSize('16x16 48x48')).toBe(48)
    expect(parseIconSize('any')).toBe(512)
    expect(parseIconSize(undefined)).toBe(0)
  })

  it('prefers a big apple-touch-icon and resolves it', () => {
    const best = pickBestIcon(
      [
        { href: '/favicon.ico', rel: 'shortcut icon' },
        { href: '/icon-32.png', rel: 'icon', sizes: '32x32' },
        { href: 'touch.png', rel: 'apple-touch-icon', sizes: '180x180' }
      ],
      'https://a.test/dir/page'
    )
    expect(best).toBe('https://a.test/dir/touch.png')
  })

  it('without apple icon picks the icon closest to 128, above small ones', () => {
    const list = [
      { href: '/16.png', rel: 'icon', sizes: '16x16' },
      { href: '/192.png', rel: 'icon', sizes: '192x192' },
      { href: '/128.png', rel: 'icon', sizes: '128x128' }
    ]
    expect(pickBestIcon(list, 'https://a.test')).toBe('https://a.test/128.png')
    expect(pickBestIcon(list.slice(0, 1), 'https://a.test')).toBe('https://a.test/16.png')
  })

  it('ignores data urls, other rels and bad input', () => {
    expect(pickBestIcon([{ href: 'data:image/png;base64,AAA', rel: 'icon' }], 'https://a.test')).toBeNull()
    expect(pickBestIcon([{ href: '/a.css', rel: 'stylesheet' }], 'https://a.test')).toBeNull()
    expect(pickBestIcon([], 'https://a.test')).toBeNull()
  })

  it('validates messages', () => {
    const m = parseIconsMessage(
      JSON.stringify({ type: 'icons', base: 'https://a.test', icons: [{ href: '/i.png', rel: 'icon', sizes: '' }, { href: 3 }, null] })
    )
    expect(m?.icons).toEqual([{ href: '/i.png', rel: 'icon' }])
    expect(parseIconsMessage('{"type":"meta"}')).toBeNull()
    expect(parseIconsMessage('x')).toBeNull()
  })

  it('builds the fallback chain', () => {
    expect(iconChain('https://a.test/i.png', 'https://a.test/p')).toEqual([
      'https://a.test/i.png',
      'https://a.test/apple-touch-icon.png',
      'https://icons.duckduckgo.com/ip3/a.test.ico'
    ])
    expect(iconChain(undefined, 'https://a.test/p')).toHaveLength(2)
    expect(iconChain('https://x.test/i.png', 'about:blank')).toEqual(['https://x.test/i.png'])
    expect(iconChain(undefined, undefined)).toEqual([])
  })
})

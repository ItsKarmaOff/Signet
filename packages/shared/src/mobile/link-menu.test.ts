import { describe, expect, it } from 'vitest'
import { isMenuUrl, linkMenuTitle, parseLinkMenuMessage, resolveHref, truncateMiddle } from './link-menu'

describe('link menu', () => {
  it('accepts web links only', () => {
    expect(isMenuUrl('https://a.test/x')).toBe(true)
    expect(isMenuUrl('javascript:alert(1)')).toBe(false)
    expect(isMenuUrl('mailto:a@b.test')).toBe(false)
  })

  it('resolves relative links against the page', () => {
    expect(resolveHref('/a?b=1', 'https://x.test/p/q')).toBe('https://x.test/a?b=1')
    expect(resolveHref('  JavaScript:void(0)', 'https://x.test')).toBeNull()
    expect(resolveHref('', 'https://x.test')).toBeNull()
    expect(resolveHref('/a')).toBeNull()
  })

  it('parses and validates messages', () => {
    const ok = parseLinkMenuMessage(JSON.stringify({ type: 'linkMenu', href: 'https://a.test/', text: '  Hello \n world ', x: 10, y: 20 }))
    expect(ok).toEqual({ type: 'linkMenu', href: 'https://a.test/', text: 'Hello world', x: 10, y: 20 })
    expect(parseLinkMenuMessage(JSON.stringify({ type: 'linkMenu', href: 'javascript:x()', text: '' }))).toBeNull()
    expect(parseLinkMenuMessage(JSON.stringify({ type: 'find', href: 'https://a.test' }))).toBeNull()
    expect(parseLinkMenuMessage('nope')).toBeNull()
    expect(parseLinkMenuMessage(JSON.stringify({ type: 'linkMenu', href: 'https://a.test', text: 5, x: 'a' }))).toMatchObject({ text: '', x: 0, y: 0 })
  })

  it('titles with the text, else the site', () => {
    expect(linkMenuTitle({ href: 'https://www.a.test/p', text: 'Hi' })).toBe('Hi')
    expect(linkMenuTitle({ href: 'https://www.a.test/p', text: '' })).toContain('a.test')
  })

  it('truncates in the middle', () => {
    expect(truncateMiddle('short', 20)).toBe('short')
    const long = 'https://example.com/' + 'a'.repeat(100) + '/end'
    const out = truncateMiddle(long, 40)
    expect(out).toHaveLength(40)
    expect(out).toContain('…')
    expect(out.endsWith('/end')).toBe(true)
  })
})

import { describe, expect, it } from 'vitest'
import type { Session } from 'electron'
import { CONTINUE_TITLE, HttpsOnly } from './https-only'

const make = (httpsOnly = true, devAliases: { name: string; target: string }[] = []) =>
  new HttpsOnly({ store: { state: { settings: { httpsOnly, devAliases } } } as never })
const ses = () => ({}) as Session

describe('HttpsOnly.resolve', () => {
  it('does nothing when the mode is off', () => {
    expect(make(false).resolve('t', ses(), 'http://example.com/')).toBe('http://example.com/')
  })
  it('upgrades http and leaves exempt hosts and local names', () => {
    const h = make(true, [{ name: 'site', target: 'http://site.test:7000' }])
    const s = ses()
    expect(h.resolve('t', s, 'http://example.com/a?b#c')).toBe('https://example.com/a?b#c')
    expect(h.resolve('t', s, 'http://localhost:3000/')).toBe('http://localhost:3000/')
    expect(h.resolve('t', s, 'http://site.test:7000/x')).toBe('http://site.test:7000/x')
    expect(h.resolve('t', s, 'https://example.com/')).toBe('https://example.com/')
  })
  it('stops upgrading a site that keeps redirecting back to http', () => {
    const h = make()
    const s = ses()
    const results = [1, 2, 3, 4].map(() => h.resolve('t', s, 'http://loop.example.com/'))
    expect(results.slice(0, 3)).toEqual(Array(3).fill('https://loop.example.com/'))
    expect(results[3]).toBe('http://loop.example.com/')
  })
})

describe('HttpsOnly warning', () => {
  it('a failed upgraded load shows the warning; a DNS error or another host does not', () => {
    const h = make()
    h.resolve('t', ses(), 'http://example.com/a')
    expect(h.insecureFailure('t', 'https://example.com/a', -102)).toBe('http://example.com/a')
    expect(h.insecureFailure('t', 'https://example.com/a', -200)).toBe('http://example.com/a')
    expect(h.insecureFailure('t', 'https://example.com/a', -105)).toBeNull()
    expect(h.insecureFailure('t', 'https://other.com/a', -102)).toBeNull()
    expect(h.insecureFailure('other-tab', 'https://example.com/a', -102)).toBeNull()
  })
  it('a typed address promoted to https counts as an upgrade', () => {
    const h = make()
    h.noteTyped('t', 'https://example.com/')
    expect(h.insecureFailure('t', 'https://example.com/', -107)).toBe('http://example.com/')
  })
  it('no warning when the mode is off', () => {
    const h = make(false)
    h.noteTyped('t', 'https://example.com/')
    expect(h.insecureFailure('t', 'https://example.com/', -102)).toBeNull()
  })
  it('continue needs the right token, allows the host for that session only, once', () => {
    const h = make()
    const a = ses()
    const b = ses()
    h.resolve('t', a, 'http://example.com/a')
    const token = h.warn('t', 'http://example.com/a')
    expect(h.continueFrom('t', a, CONTINUE_TITLE + 'forged')).toBeNull()
    expect(h.continueFrom('other', a, CONTINUE_TITLE + token)).toBeNull()
    expect(h.continueFrom('t', a, CONTINUE_TITLE + token)).toBe('http://example.com/a')
    expect(h.continueFrom('t', a, CONTINUE_TITLE + token)).toBeNull()
    expect(h.resolve('t', a, 'http://example.com/a')).toBe('http://example.com/a')
    expect(h.resolve('t', a, 'http://example.com/other')).toBe('http://example.com/other')
    expect(h.resolve('t', b, 'http://example.com/a')).toBe('https://example.com/a') // another session (private window)
  })
})

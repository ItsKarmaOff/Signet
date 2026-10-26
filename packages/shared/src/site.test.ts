import { describe, expect, it } from 'vitest'
import { siteOf } from './site'

describe('siteOf', () => {
  it('gives the registrable domain', () => {
    expect(siteOf('https://www.example.com/a?b=1')).toBe('example.com')
    expect(siteOf('https://sub.deep.example.com')).toBe('example.com')
    expect(siteOf('https://www.bbc.co.uk/news')).toBe('bbc.co.uk')
  })

  it('lowercases and ignores the port of public domains', () => {
    expect(siteOf('https://WWW.Example.COM:8443/')).toBe('example.com')
  })

  it('counts private suffixes (github.io)', () => {
    expect(siteOf('https://alice.github.io/repo')).toBe('alice.github.io')
    expect(siteOf('https://bob.github.io/')).toBe('bob.github.io')
  })

  it('keeps the port for localhost and IP addresses', () => {
    expect(siteOf('http://localhost:3000/x')).toBe('localhost:3000')
    expect(siteOf('http://localhost:5173')).toBe('localhost:5173')
    expect(siteOf('http://localhost/')).toBe('localhost')
    expect(siteOf('http://192.168.1.5:8080/')).toBe('192.168.1.5:8080')
    expect(siteOf('http://[::1]:80/')).toBe('[::1]')
  })

  it('keeps the host as is when it has no public domain', () => {
    expect(siteOf('http://intranet/')).toBe('intranet')
    expect(siteOf('http://nas.local/')).toBe('nas.local')
  })

  it('returns an empty string for an unreadable address', () => {
    expect(siteOf('pas une url')).toBe('')
    expect(siteOf('')).toBe('')
  })
})

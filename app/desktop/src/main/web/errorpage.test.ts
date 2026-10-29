import { describe, expect, it, vi } from 'vitest'

vi.mock('electron', () => ({
  app: { getPreferredSystemLanguages: () => ['en-US'], getPath: () => '' },
  webContents: { getAllWebContents: () => [] }
}))

import { crashDocument, ERROR_CSS, injectionScript, insecureSiteScript, insecureSiteText, loadErrorText } from './errorpage'

describe('loadErrorText', () => {
  it('known cause: no raw code', () => {
    const e = loadErrorText('https://example.com/a', -105)
    expect(e.host).toBe('example.com')
    expect(e.code).toBe('')
    expect(e.reason).not.toBe('')
  })
  it('groups codes by cause', () => {
    expect(loadErrorText('https://a.com', -105).reason).toBe(loadErrorText('https://a.com', -137).reason)
    expect(loadErrorText('https://a.com', -201).reason).toBe(loadErrorText('https://a.com', -219).reason)
    expect(loadErrorText('https://a.com', -106).reason).not.toBe(loadErrorText('https://a.com', -105).reason)
  })
  it('unknown cause: the raw code is shown', () => {
    const e = loadErrorText('https://a.com', -999)
    expect(e.code).toContain('-999')
  })
  it('invalid URL: returns the input as is', () => {
    expect(loadErrorText('pas une url', -105).host).toBe('pas une url')
  })
})

describe('crashDocument', () => {
  it('escapes the address and offers to reload a web address', () => {
    const html = crashDocument('https://example.com/?a="><script>alert(1)</script>')
    expect(html).not.toContain('<script>')
    expect(html).toContain('href="https://example.com/?a=&quot;&gt;&lt;script&gt;')
  })
  it('no link for another scheme', () => {
    expect(crashDocument('javascript:alert(1)')).not.toContain('<a ')
    expect(crashDocument('signet://settings')).not.toContain('<a ')
  })
})

describe('injectionScript', () => {
  it('serializes the texts as JSON (no injection possible)', () => {
    const text = { title: 'T', host: '</script>"', reason: 'r', code: '', retry: 'x' }
    const js = injectionScript(text, 'a{}')
    expect(js).toContain(JSON.stringify(text))
    expect(js).toContain("location.protocol !== 'chrome-error:'")
    expect(() => new Function(js)).not.toThrow()
  })
})

describe('insecureSiteScript', () => {
  it('texts name the host', () => {
    const text = insecureSiteText('https://example.com:8443/a')
    expect(text.host).toBe('example.com:8443')
    expect(text.title).not.toBe('')
    expect(text.continue).not.toBe(text.back)
  })
  it('carries the token and title prefix, only works in the engine error page, no innerHTML', () => {
    const js = insecureSiteScript(insecureSiteText('https://example.com'), ERROR_CSS, 'tok-123', 'signet-https-continue:', true)
    expect(js).toContain('signet-https-continue:tok-123')
    expect(js).toContain("location.protocol !== 'chrome-error:'")
    expect(js).not.toContain('innerHTML')
    expect(js).toContain('history.back()')
  })
  it('no "Go back" button without history', () => {
    const js = insecureSiteScript(insecureSiteText('https://example.com'), ERROR_CSS, 't', 'p:', false)
    expect(js).toContain('false ? button')
  })
})

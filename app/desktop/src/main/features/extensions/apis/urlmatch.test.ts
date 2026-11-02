import { describe, expect, it } from 'vitest'
import { join, resolve } from 'node:path'
import { canAccessUrl, extensionFilePath, urlAsPattern, urlMatches } from './urlmatch'

const ID = 'a'.repeat(32)
const none = { permissions: [], origins: [] }

describe('urlMatches', () => {
  it('matches host patterns on the path only', () => {
    expect(urlMatches('https://*.example.com/*', 'https://www.example.com/a?b=1#c')).toBe(true)
    expect(urlMatches('https://example.com/docs/*', 'https://example.com/other')).toBe(false)
    expect(urlMatches('*://*/*', 'http://localhost:8080/x')).toBe(true)
    expect(urlMatches('<all_urls>', 'https://a.test/')).toBe(true)
    expect(urlAsPattern('not a url')).toBeNull()
  })
})

describe('canAccessUrl', () => {
  it('accepts activeTab, <all_urls>, a covering host pattern and the extension own pages', () => {
    expect(canAccessUrl({ permissions: ['activeTab'], origins: [] }, ID, 'https://a.test/')).toBe(true)
    expect(canAccessUrl({ permissions: [], origins: ['<all_urls>'] }, ID, 'https://a.test/')).toBe(true)
    expect(canAccessUrl({ permissions: [], origins: ['https://a.test/*'] }, ID, 'https://a.test/x')).toBe(true)
    expect(canAccessUrl({ permissions: [], origins: ['https://a.test/*'] }, ID, 'https://b.test/x')).toBe(false)
    expect(canAccessUrl(none, ID, 'https://a.test/')).toBe(false)
    expect(canAccessUrl(none, ID, `chrome-extension://${ID}/page.html`)).toBe(true)
  })
})

describe('extensionFilePath', () => {
  // Absolute on every system (`resolve` adds the drive letter on Windows), like a real extension folder.
  const root = resolve('/ext', 'root')
  it('resolves relative, rooted and own chrome-extension paths and refuses escapes', () => {
    expect(extensionFilePath(root, ID, 'css/a.css')).toBe(join(root, 'css', 'a.css'))
    expect(extensionFilePath(root, ID, '/css/a.css')).toBe(join(root, 'css', 'a.css'))
    expect(extensionFilePath(root, ID, `chrome-extension://${ID}/i.png`)).toBe(join(root, 'i.png'))
    expect(extensionFilePath(root, ID, `chrome-extension://${'b'.repeat(32)}/i.png`)).toBeNull()
    expect(extensionFilePath(root, ID, '../secret')).toBeNull()
    expect(extensionFilePath(root, ID, '')).toBeNull()
  })
})

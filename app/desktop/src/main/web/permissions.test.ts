import { describe, expect, it, vi } from 'vitest'

vi.mock('electron', () => ({
  app: { getPreferredSystemLanguages: () => ['en-US'], getPath: () => '' },
  webContents: { getAllWebContents: () => [] },
  BrowserWindow: class {},
  dialog: {},
  net: {}
}))

import { originOf } from './permissions'

describe('originOf', () => {
  it('origin of a URL', () => {
    expect(originOf('https://example.com/a?b#c')).toBe('https://example.com')
    expect(originOf('http://localhost:3000/x')).toBe('http://localhost:3000')
  })
  it('raw string if unusable', () => {
    expect(originOf('pas une url')).toBe('pas une url')
    expect(originOf('')).toBe('')
  })
})

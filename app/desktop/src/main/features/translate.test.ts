import { describe, expect, it, vi } from 'vitest'

vi.mock('electron', () => ({
  app: { getPreferredSystemLanguages: () => ['en-US'], getPath: () => '' },
  webContents: { getAllWebContents: () => [] },
  BrowserWindow: class {},
  dialog: {},
  net: {}
}))

import { serviceLang } from './translate'

describe('serviceLang', () => {
  it('base language in lowercase', () => {
    expect(serviceLang('fr-FR')).toBe('fr')
    expect(serviceLang('EN')).toBe('en')
    expect(serviceLang('pt-BR')).toBe('pt')
  })
  it('Chinese: simplified or traditional', () => {
    expect(serviceLang('zh')).toBe('zh-CN')
    expect(serviceLang('zh-CN')).toBe('zh-CN')
    expect(serviceLang('zh-Hans')).toBe('zh-CN')
    expect(serviceLang('zh-TW')).toBe('zh-TW')
    expect(serviceLang('zh-HK')).toBe('zh-TW')
    expect(serviceLang('zh-Hant')).toBe('zh-TW')
  })
})

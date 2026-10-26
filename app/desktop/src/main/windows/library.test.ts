import { describe, expect, it, vi } from 'vitest'

vi.mock('electron', () => ({
  app: { getPreferredSystemLanguages: () => ['en-US'], getPath: () => '' },
  webContents: { getAllWebContents: () => [] },
  nativeTheme: {},
  BrowserWindow: class {}
}))

import { firstEmoji } from './library'

describe('firstEmoji', () => {
  it('first grapheme (composite emoji included)', () => {
    expect(firstEmoji('🔥abc')).toBe('🔥')
    expect(firstEmoji('👨‍👩‍👧 famille')).toBe('👨‍👩‍👧')
    expect(firstEmoji('🇫🇷🇩🇪')).toBe('🇫🇷')
  })
  it('empty or not a string: absent', () => {
    expect(firstEmoji('')).toBeUndefined()
    expect(firstEmoji('   ')).toBeUndefined()
    expect(firstEmoji(null)).toBeUndefined()
    expect(firstEmoji(42)).toBeUndefined()
  })
})

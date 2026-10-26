import { describe, expect, it, vi } from 'vitest'

vi.mock('electron', () => ({
  app: { getPreferredSystemLanguages: () => ['en-US'], getPath: () => '' },
  webContents: { getAllWebContents: () => [] },
  BrowserWindow: class {},
  dialog: {},
  nativeImage: {},
  nativeTheme: {},
  screen: {},
  shell: {},
  session: {},
  ipcMain: {},
  protocol: {}
}))

import { cleanAppName } from './webapps'

describe('cleanAppName', () => {
  it('removes control characters, tightens spaces, caps at 60', () => {
    expect(cleanAppName('  Mon\u0000  App  Test ')).toBe('Mon App Test')
    expect(cleanAppName('App\nTest\tX')).toBe('App Test X')
    expect(cleanAppName('x'.repeat(100))).toHaveLength(60)
  })
  it('non-text value: empty', () => {
    expect(cleanAppName(undefined)).toBe('')
    expect(cleanAppName(42)).toBe('')
  })
})

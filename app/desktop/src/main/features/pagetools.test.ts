import { describe, expect, it, vi } from 'vitest'

vi.mock('electron', () => ({
  app: { getPreferredSystemLanguages: () => ['en-US'], getPath: () => '' },
  webContents: { getAllWebContents: () => [] },
  BrowserWindow: class {},
  dialog: {}
}))

import { safeFileBase } from './pagetools'

describe('safeFileBase', () => {
  it('replaces : and brackets (IPv6)', () => {
    expect(safeFileBase('[::1]:3000')).toBe('---1--3000')
  })
  it('removes trailing dots and spaces', () => {
    expect(safeFileBase('page. . ')).toBe('page')
  })
  it('suffixes reserved Windows names', () => {
    expect(safeFileBase('con')).toBe('con_')
    expect(safeFileBase('NUL')).toBe('NUL_')
    expect(safeFileBase('com1')).toBe('com1_')
    expect(safeFileBase('lpt9.txt')).toBe('lpt9.txt_')
    expect(safeFileBase('console')).toBe('console')
  })
  it('ordinary name unchanged', () => {
    expect(safeFileBase('Mon article')).toBe('Mon article')
  })
})

import { describe, expect, it } from 'vitest'
import { APP_ICONS, BACKUP_SECTIONS, IMPORT_KINDS, PASSWORD_EXTENSIONS } from './index'

// Runtime constants of types.ts: their shape is frozen (consumers depend on it).
describe('constantes de types.ts', () => {
  it('has lists without duplicates', () => {
    for (const list of [IMPORT_KINDS, BACKUP_SECTIONS, APP_ICONS.map((i) => i.id), PASSWORD_EXTENSIONS.map((p) => p.id)]) {
      expect(new Set(list).size).toBe(list.length)
    }
  })

  it('freezes the import and backup types', () => {
    expect([...IMPORT_KINDS]).toEqual(['spaces', 'bookmarks', 'passwords', 'extensions', 'history'])
    expect(BACKUP_SECTIONS).toContain('settings')
    expect(BACKUP_SECTIONS).toContain('notes')
  })

  it('has valid Chrome Web Store extension ids (32 letters a-p)', () => {
    for (const p of PASSWORD_EXTENSIONS) expect(p.extensionId, p.id).toMatch(/^[a-p]{32}$/)
  })
})

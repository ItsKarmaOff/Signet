/** Parts of a backup (`.signetbackup`), in Settings order. */
/** Data an import can take from another browser (`spaces`: Arc spaces, pinned tabs and favorites). No open tabs or cookies. */
export const IMPORT_KINDS = ['spaces', 'bookmarks', 'passwords', 'extensions', 'history'] as const

export type ImportKind = (typeof IMPORT_KINDS)[number]

export const BACKUP_SECTIONS = [
  'settings',
  'favorites',
  'spaces',
  'today',
  'bookmarks',
  'history',
  'archive',
  'boosts',
  'permissions',
  'mods',
  'notes',
  'passwords',
  'extensions'
] as const

export type BackupSection = (typeof BACKUP_SECTIONS)[number]

/** Import: add to existing data or replace it (settings are always replaced). */
export type BackupMode = 'add' | 'replace'

/** Settings Backup section: item count per part (current) and safety copies. */
export interface BackupInfo {
  counts: Partial<Record<BackupSection, number>>
  /** Copies made before each import, from newest to oldest. */
  safety: { name: string; createdAt: number }[]
}

/** Backup file that has been read, awaiting import: parts present (`passwords`: -1, count unknown before decryption). */
export interface BackupSummary {
  token: string
  createdAt: number
  appVersion: string
  counts: Partial<Record<BackupSection, number>>
  /** Safety copy: passwords are restored without a backup password. */
  systemSealed?: boolean
}

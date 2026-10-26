import type { PasswordManager } from '@signet/shared/types'

/** Managers provided by a store extension (Chrome Web Store id). */
export const PASSWORD_EXTENSIONS = [
  { id: 'bitwarden', name: 'Bitwarden', extensionId: 'nngceckbapebfimnlniiiahkandclblb' },
  { id: '1password', name: '1Password', extensionId: 'aeblfdkhhhdcdjpifhhbdiojplfjncoa' },
  { id: 'proton', name: 'Proton Pass', extensionId: 'ghmbeldphafepmbegfdlkpapadhbakde' },
  { id: 'dashlane', name: 'Dashlane', extensionId: 'fdjamakpfbbddfjaooikfcpapjohcfmg' },
  { id: 'lastpass', name: 'LastPass', extensionId: 'hdokiejnpimakedhajhdlcegeplioahd' }
] as const satisfies readonly { id: PasswordManager; name: string; extensionId: string }[]

/** Saved login, as Settings sees it: never the password. */
export interface SavedLogin {
  id: string
  /** Origine exacte (`https://exemple.com`). */
  origin: string
  username: string
  createdAt: number
  updatedAt: number
  lastUsedAt?: number
}

/** Result of a CSV password import: added, already saved, and the cause of a failure. */
export interface PasswordImportResult {
  added: number
  existing: number
  error?: 'format' | 'read' | 'encryption'
}

/** State of the Settings Passwords section (`passwords:list`). */
export interface PasswordsInfo {
  logins: SavedLogin[]
  /** False if the system cannot encrypt (nothing can be saved). */
  encryption: boolean
  /** True if revealing a password requires Touch ID. */
  touchId: boolean
}

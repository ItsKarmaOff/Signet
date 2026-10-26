import type { Browser } from '../browser/browser'
import type { Registrar } from './registrar'

/** Passwords: Settings (list without passwords, reveal after authentication) and save prompt. */
export function registerPasswordsIpc(b: Browser, { handle }: Registrar): void {
  handle('listPasswords', () => b.passwords.list())
  handle('revealPassword', (_c, id) => b.passwords.reveal(id))
  handle('copyPassword', (_c, id) => b.passwords.copy(id))
  handle('savePassword', (_c, id, origin, username, password) => b.passwords.upsert(id, origin, username, password))
  handle('deletePassword', (_c, id) => b.passwords.remove(id))
  handle('importPasswords', ({ sender }) => b.passwords.importCsv(sender))
  handle('exportPasswords', ({ sender }) => b.passwords.exportCsv(sender))
  handle('installPasswordExtension', (_c, manager) => b.extensions.installPasswordManager(manager))
  handle('openPasswordKey', (_c, tabId, anchor) => b.passwords.openKey(tabId, anchor))
  handle('answerPasswordPrompt', (_c, token, answer) => b.passwords.answer(token, answer))
}

import type { Browser } from '../browser/browser'
import type { Registrar } from './registrar'

/** Passwords: Settings (list without passwords, reveal after authentication) and save prompt. */
/** Autofill: Settings lists (cards without numbers, reveal after authentication). */
export function registerAutofillIpc(b: Browser, { handle }: Registrar): void {
  handle('listAutofill', () => b.autofill.list())
  handle('saveAddress', (_c, id, address) => b.autofill.upsertAddress(id, address))
  handle('deleteAddress', (_c, id) => b.autofill.removeAddress(id))
  handle('saveCard', (_c, id, card) => b.autofill.upsertCard(id, card))
  handle('deleteCard', (_c, id) => b.autofill.removeCard(id))
  handle('revealCard', (_c, id) => b.autofill.revealCard(id))
}

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
  // The same prompt serves the save offers of autofill: the token tells which module owns it.
  handle('answerPasswordPrompt', (_c, token, answer) => {
    if (!b.autofill.answer(token, answer)) b.passwords.answer(token, answer)
  })
}

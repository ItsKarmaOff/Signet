import { ipcMain } from 'electron'
import type { Browser } from '../browser/browser'
import { noteFormSubmit } from '../browser/tabs'
import { registerWebAuthnIpc } from '../web/webauthn'

/**
 * Web page channels (UNTRUSTED: they come from tabs, not the interface). They use neither the "interface only"
 * guard nor the contract schemas: each checks its sender, and the list is `PAGE_IPC`.
 */
export function registerPageIpc(b: Browser): void {
  // Swipe to navigate: the page script asks for the history state then the navigation.
  ipcMain.handle('swipe:can', (e) => b.swipe.can(e.sender))
  ipcMain.on('swipe:navigate', (e, direction: unknown) => {
    if (direction === 'back' || direction === 'forward') b.swipe.navigate(e.sender, direction)
  })
  // Form submission in a page (swipe script): only from a tab.
  ipcMain.on('page:submit', (e) => {
    if (b.views.tabIdOfContents(e.sender.id)) noteFormSubmit(e.sender)
  })
  // Notes and Easels: document page channels (sender's document checked in notes.ts).
  b.notes.registerIpc()
  // Passwords page script: main checks the tab and the frame's origin.
  b.passwords.registerPageIpc()
  // Extension pages calling the APIs Signet adds (proxy, privacy, alarms): sender checked in the router.
  b.extensions.registerPageIpc()
  // Passkey requests (WebAuthn page script): validated and answered in main.
  registerWebAuthnIpc(b)
}

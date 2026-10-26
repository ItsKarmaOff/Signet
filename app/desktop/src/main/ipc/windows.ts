import type { Browser } from '../browser/browser'
import type { Registrar } from './registrar'

/** Separate windows: mini player, Little Signet, private windows, installed apps. */
export function registerWindowsIpc(b: Browser, { handle, handlePip }: Registrar): void {
  handle('pipCommand', (_c, command, value) => b.pip.command(command, value))
  handle('openPip', (_c, tabId) => b.pip.open(tabId))
  // Mini player gestures (`window.signetPip`): only from the mini player view.
  handlePip('drag', ({ sender }, on) => {
    if (b.pip.ownsSender(sender)) b.pip.drag(on)
  })
  handlePip('resize', ({ sender }, phase, handle, dx, dy) => {
    if (b.pip.ownsSender(sender)) b.pip.resize(phase, handle, Number(dx), Number(dy))
  })

  handle('littleReady', ({ sender }) => b.little.ready(sender))
  handle('littleOpenInMain', ({ sender }) => b.little.openInMain(sender))
  handle('littleCopyUrl', ({ sender }) => b.tabs.copyUrl(b.little.tabIdForBar(sender)))

  // Passkey from a phone (QR code window).
  handle('getPasskeyPrompt', () => b.passkeys.prompt())
  handle('cancelPasskeyPrompt', () => b.passkeys.cancel())

  handle('openPrivateWindow', () => b.privates.open())
  handle('getPrivateState', ({ sender }) => b.privates.stateForBar(sender))
  handle('privateNewTab', ({ sender }) => b.privates.newTabFromBar(sender))
  handle('privateActivate', ({ sender }, id) => b.privates.activate(sender, id))
  handle('privateCloseTab', ({ sender }, id) => b.privates.closeFromBar(sender, id))
  handle('privateNavigate', ({ sender }, input) => b.privates.navigate(sender, input))

  // Apps: install (overlay), Settings › Apps, app window bar (which target their own window).
  handle('installWebApp', (_c, token, name, icon) => b.webApps.install(token, name, icon))
  handle('listWebApps', () => b.webApps.list())
  handle('openWebApp', (_c, id) => b.webApps.launch(id))
  handle('renameWebApp', (_c, id, name) => b.webApps.rename(id, name))
  handle('revealWebApp', (_c, id) => b.webApps.reveal(id))
  handle('removeWebApp', (_c, id) => b.webApps.remove(id))
  handle('webAppReady', ({ sender }) => b.webApps.ready(sender))
  handle('webAppHome', ({ sender }) => b.webApps.home(sender))
  handle('webAppOpenInMain', ({ sender }) => b.webApps.openInMain(sender))
  handle('webAppCopyUrl', ({ sender }) => b.webApps.copyUrl(sender))
}

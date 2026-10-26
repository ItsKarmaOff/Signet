import type { Browser } from '../browser/browser'
import type { Registrar } from './registrar'

/** Devices: switches, list of paired phones, QR code pairing. */
export function registerLinkIpc(b: Browser, { handle }: Registrar): void {
  handle('getLinkState', () => b.link.state())
  handle('setLinkGeneral', (_c, on) => b.link.setGeneral(on))
  handle('setLinkDevice', (_c, linkId, on) => b.link.setDevice(linkId, on))
  handle('forgetLink', (_c, linkId) => b.link.forget(linkId))
  handle('startLinkPairing', () => b.link.startPairing())
  handle('cancelLinkPairing', () => b.link.cancelPairing())
}

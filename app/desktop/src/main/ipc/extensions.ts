import type { Browser } from '../browser/browser'
import type { Registrar } from './registrar'

/** Extensions (store, unpacked, menus). */
export function registerExtensionsIpc(b: Browser, { handle }: Registrar): void {
  handle('openExtensionStore', () => b.extensions.openStore())
  handle('removeExtension', (_c, id) => b.extensions.remove(id))
  handle('loadUnpackedExtension', () => b.unpacked.pick())
  handle('reloadUnpackedExtension', (_c, path) => b.unpacked.reload(path))
  handle('removeUnpackedExtension', (_c, path) => b.unpacked.remove(path))
  handle('showExtensionMenu', (_c, id) => b.extensions.showMenu(id))
  handle('showExtensionsMenu', () => b.extensions.showListMenu())
}

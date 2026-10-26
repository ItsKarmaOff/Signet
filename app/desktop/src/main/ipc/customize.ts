import type { Browser } from '../browser/browser'
import type { Registrar } from './registrar'

/** Customization: interface mods (Settings › Mods) and Boosts. */
export function registerCustomizeIpc(b: Browser, { handle }: Registrar): void {
  handle('listMods', () => b.mods.info())
  handle('saveMod', (_c, id, name, css) => b.mods.save(id, name, css))
  handle('deleteMod', (_c, id) => b.mods.remove(id))
  handle('setModEnabled', (_c, id, enabled) => b.mods.setEnabled(id, enabled))
  handle('moveMod', (_c, id, delta) => b.mods.move(id, delta))
  handle('setModsDisabled', (_c, disabled) => b.mods.setDisabled(disabled))
  handle('importMod', ({ sender }) => b.mods.importFile(sender))
  handle('exportMod', ({ sender }, id) => b.mods.exportFile(id, sender))

  handle('saveBoost', (_c, site, patch) => b.boosts.save(site, patch))
  handle('startZap', (_c, site) => b.boosts.startZap(site))
  handle('stopZap', () => b.boosts.stopZap())
  handle('unzap', (_c, site, selector) => b.boosts.unzap(site, selector))
  handle('deleteBoost', (_c, site) => b.boosts.delete(site))
}

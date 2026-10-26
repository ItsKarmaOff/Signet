import type { Browser } from '../browser/browser'
import { setShortcutRecording } from '../app/menu'
import type { Registrar } from './registrar'

/** Settings, backup, import, browsing data. */
export function registerSettingsIpc(b: Browser, { handle }: Registrar): void {
  handle('openSettings', (_c, section) => b.settings.open(section))
  handle('getSettingsInfo', () => b.settings.info())
  handle('updateSettings', (_c, patch) => b.settings.update(patch))
  handle('setShortcutRecording', (_c, recording) => setShortcutRecording(b, recording))
  handle('setDefaultBrowser', () => b.settings.setDefaultBrowser())
  handle('chooseDownloadDir', () => b.settings.chooseDownloadDir())
  handle('runImport', (_c, id, kinds) => b.settings.runImport(id, kinds))
  handle('openWelcome', () => b.welcome.open())
  handle('checkForUpdates', () => b.updates.check(true))
  handle('welcomeFinish', () => b.welcome.finish())
  handle('openBoost', (_c, site) => b.boosts.open(site))
  handle('clearBrowsingData', (_c, kinds) => b.settings.clearData(kinds))

  // Backup: the part lists and the password are re-validated in backup.ts.
  handle('getBackupInfo', () => b.backup.info())
  handle('exportBackup', ({ sender }, sections, password) => b.backup.exportTo(sender, sections, password))
  handle('openBackup', ({ sender }) => b.backup.open(sender))
  handle('openSafetyBackup', (_c, name) => b.backup.openSafety(name))
  handle('applyBackup', ({ sender }, token, sections, mode, password) => b.backup.apply(sender, token, sections, mode, password))
  handle('revealSafetyBackups', () => b.backup.revealSafety())
}

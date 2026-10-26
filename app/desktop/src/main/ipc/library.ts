import type { Browser } from '../browser/browser'
import type { Registrar } from './registrar'

/** Library (history, archive, downloads), site panel, permissions. */
export function registerLibraryIpc(b: Browser, { handle }: Registrar): void {
  handle('openLibrary', (_c, section) => b.library.open(section))
  handle('openLibraryUrl', (_c, url) => {
    // Schemes saved in history; javascript:, data: etc. refused.
    if (!/^(https?|file|signet|chrome-extension):/i.test(url)) return
    b.window.show()
    b.tabs.open(url, { activate: true })
  })
  handle('queryHistory', (_c, q, offset, limit) => b.library.queryHistory(q, offset, limit))
  handle('deleteHistory', (_c, url) => b.library.deleteHistory(url))
  handle('listArchive', (_c, q) => b.library.listArchive(q))
  handle('restoreArchived', (_c, id) => b.library.restoreArchived(id))
  handle('deleteArchived', (_c, id) => b.library.deleteArchived(id))

  handle('openDownload', (_c, id) => b.downloads.open(id))
  handle('revealDownload', (_c, id) => b.downloads.reveal(id))
  handle('cancelDownload', (_c, id) => b.downloads.cancel(id))
  handle('retryDownload', (_c, id) => b.downloads.retry(id))
  handle('clearDownloads', () => b.downloads.clear())

  handle('openSitePanel', ({ sender }, anchor) => b.sitePanel.open(sender, anchor))
  handle('setSitePermission', (_c, origin, permission, state) => b.sitePanel.setPermission(origin, permission, state))
  handle('clearSiteData', (_c, origin) => b.sitePanel.clearData(origin))
  handle('listSitePermissions', () => b.permissions.listAll())
  handle('resetSitePermissions', (_c, origin, permission) => b.permissions.reset(origin, permission))
}

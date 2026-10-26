// Import / export of bookmarks as an HTML file (Netscape format). Returns the message to show (toast), or `null` when there is nothing to say.
import { File, Paths } from 'expo-file-system'
import * as Sharing from 'expo-sharing'
import { bookmarkLinks } from '@signet/shared/mobile/bookmarks'
import { exportBookmarksHtml, parseBookmarksHtml } from '@signet/shared/mobile/bookmarks-html'
import type { Bookmark } from '@signet/shared/types'
import { t } from '@/lib/i18n'
import { useStore } from '@/state'

export async function exportBookmarks(tree: readonly Bookmark[]): Promise<string | null> {
  if (bookmarkLinks(tree).length === 0) return t('transfer.exportEmpty')
  try {
    if (!(await Sharing.isAvailableAsync())) return t('transfer.shareUnavailable')
    const file = new File(Paths.cache, 'Signet-bookmarks.html')
    file.create({ overwrite: true })
    await file.write(exportBookmarksHtml(tree))
    await Sharing.shareAsync(file.uri, { mimeType: 'text/html', UTI: 'public.html', dialogTitle: t('bookmarks.export') })
    return null
  } catch {
    return t('transfer.exportFailed')
  }
}

/** Opens the file picker, reads an HTML bookmarks file and adds it to a new folder. */
export async function importBookmarksFromFile(): Promise<string | null> {
  try {
    const picked = await File.pickFileAsync({ mimeTypes: ['text/html'] })
    if (picked.canceled) return null
    const nodes = parseBookmarksHtml(await picked.result.text())
    const count = nodes.reduce(function total(n: number, node: Bookmark): number {
      return n + (node.kind === 'link' ? 1 : node.items.reduce(total, 0))
    }, 0)
    if (count === 0) return t('transfer.importNone')
    useStore.getState().importBookmarks(nodes, t('transfer.importedFolder'))
    return t('transfer.importDone', { count })
  } catch {
    return t('transfer.importFailed')
  }
}

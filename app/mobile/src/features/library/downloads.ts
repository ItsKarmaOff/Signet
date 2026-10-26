// Downloads: fetching the file (expo-file-system) into `Documents/Downloads`, tracking in the store, opening and deleting.
// The folder is visible in the Files app ("On My iPhone > Signet") thanks to UIFileSharingEnabled in app.json.
import { Directory, File, Paths } from 'expo-file-system'
import * as Sharing from 'expo-sharing'
import { fileNameFromUrl, sanitizeFileName, uniqueFileName, type DownloadEntry } from '@signet/shared/mobile/downloads'
import { newId } from '@signet/shared/ids'
import { haptic } from '@/lib/haptics'
import { t } from '@/lib/i18n'
import { useStore } from '@/state'
import { toast } from '@/ui'

const downloadsDir = (): Directory => {
  const dir = new Directory(Paths.document, 'Downloads')
  if (!dir.exists) dir.create({ idempotent: true })
  return dir
}

/** Opens the file: preview (Quick Look on iOS) if possible, otherwise the share sheet ("Save to Files"). */
export async function openDownload(entry: DownloadEntry): Promise<void> {
  const file = new File(entry.path)
  if (!entry.path || !file.exists) {
    toast.show({ message: t('downloads.missing') })
    return
  }
  try {
    if (await File.canPreview(file)) {
      await File.preview(file)
      return
    }
  } catch {
    // no preview: share instead
  }
  await shareDownload(entry)
}

/** System share sheet: save to Files, send, open with another app. */
export async function shareDownload(entry: DownloadEntry): Promise<void> {
  const file = new File(entry.path)
  if (!entry.path || !file.exists) {
    toast.show({ message: t('downloads.missing') })
    return
  }
  try {
    if (await Sharing.isAvailableAsync()) await Sharing.shareAsync(entry.path, { dialogTitle: entry.name })
  } catch {
    // sheet dismissed or denied
  }
}

/** Deletes the file (if it still exists) and the row. */
export function deleteDownload(entry: DownloadEntry): void {
  try {
    const file = new File(entry.path)
    if (entry.path && file.exists) file.delete()
  } catch {
    // already deleted from the Files app
  }
  useStore.getState().removeDownload(entry.id)
}

/** Deletes all completed or failed files and empties the list. */
export function clearAllDownloads(): void {
  for (const d of useStore.getState().downloads) {
    if (d.status !== 'downloading') {
      try {
        const file = new File(d.path)
        if (d.path && file.exists) file.delete()
      } catch {
        // ignored
      }
    }
  }
  useStore.getState().clearDownloads()
}

const active = new Set<string>()

/**
 * Download from a private tab: nothing in the list or the backups, the file goes to the app's cache folder
 * (the system purges it on its own) and the toast offers to open it or save it to Files via the share sheet.
 */
async function startPrivateDownload(url: string): Promise<void> {
  const id = newId()
  const guess = fileNameFromUrl(url)
  active.add(url)
  toast.show({ message: t('downloads.started', { name: guess }) })
  haptic('light')
  try {
    const dir = new Directory(Paths.cache, `private-downloads-${id}`)
    dir.create({ idempotent: true })
    const file = await File.downloadFileAsync(url, dir, { idempotent: true })
    const name = sanitizeFileName(file.name || guess, guess)
    const entry: DownloadEntry = { id, name, url, size: file.size ?? 0, date: Date.now(), status: 'done', path: file.uri }
    haptic('success')
    toast.show({ message: t('downloads.done', { name }), actionLabel: t('downloads.open'), duration: 6000, onAction: () => void openDownload(entry) })
  } catch {
    haptic('error')
    toast.show({ message: t('downloads.failed', { name: guess }) })
  } finally {
    active.delete(url)
  }
}

/** Downloads an address. Errors (network, disk) are recorded in the list and reported by a toast, never thrown. `isPrivate`: see `startPrivateDownload`. */
export async function startDownload(url: string, isPrivate = false): Promise<void> {
  if (!/^https?:\/\//i.test(url) || active.has(url)) return
  if (isPrivate) return startPrivateDownload(url)
  const store = useStore.getState()
  const id = newId()
  const guess = fileNameFromUrl(url)
  active.add(url)
  store.addDownload({ id, name: guess, url, size: 0, date: Date.now(), status: 'downloading', path: '', progress: 0 })
  toast.show({ message: t('downloads.started', { name: guess }) })
  haptic('light')
  let temp: Directory | null = null
  try {
    const dir = downloadsDir()
    // One temporary folder per download: the final name (Content-Disposition header) is only known on arrival.
    temp = new Directory(dir, `.tmp-${id}`)
    temp.create({ idempotent: true })
    let lastReport = 0
    const file = await File.downloadFileAsync(url, temp, {
      idempotent: true,
      onProgress: ({ bytesWritten, totalBytes }) => {
        const now = Date.now()
        if (now - lastReport < 150 || totalBytes <= 0) return
        lastReport = now
        useStore.getState().updateDownload(id, { progress: Math.min(1, bytesWritten / totalBytes) })
      }
    })
    const taken = dir
      .list()
      .filter((x): x is File => x instanceof File)
      .map((x) => x.name)
    const name = uniqueFileName(sanitizeFileName(file.name || guess, guess), taken)
    const finalFile = new File(dir, name)
    await file.move(finalFile)
    useStore.getState().updateDownload(id, { name, size: finalFile.size ?? 0, status: 'done', path: finalFile.uri, progress: undefined })
    haptic('success')
    const entry = useStore.getState().downloads.find((d) => d.id === id)
    toast.show({
      message: t('downloads.done', { name }),
      actionLabel: t('downloads.open'),
      duration: 6000,
      onAction: () => {
        if (entry) void openDownload(entry)
      }
    })
  } catch {
    useStore.getState().updateDownload(id, { status: 'failed', progress: undefined })
    haptic('error')
    toast.show({ message: t('downloads.failed', { name: guess }) })
  } finally {
    active.delete(url)
    try {
      if (temp?.exists) temp.delete()
    } catch {
      // best-effort cleanup
    }
  }
}

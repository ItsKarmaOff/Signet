import { app, shell, type DownloadItem, type Session } from 'electron'
import { randomUUID } from 'node:crypto'
import { existsSync, statSync } from 'node:fs'
import { basename, extname, join } from 'node:path'
import type { DownloadInfo, Settings } from '@shared/types'
import type { Browser } from '../browser/browser'
import { DOWNLOADS_MAX } from '../core/store'

const PUSH_INTERVAL = 250

/** Folder where downloads go: the one from Settings if it still exists, otherwise the system's Downloads folder. */
export function downloadDir(settings: Settings): string {
  const dir = settings.downloadDir
  try {
    if (dir && statSync(dir).isDirectory()) return dir
  } catch {
    // folder deleted or disk unplugged: back to the system folder
  }
  return app.getPath('downloads')
}

/** Downloads: saved directly to the chosen folder (or asking each time), tracking and progress in the Dock. */
export class Downloads {
  private items = new Map<string, DownloadItem>()
  /** Origin session of private downloads (never written, removed from the list when the window closes). */
  private privateOf = new Map<string, Session>()
  /** Session of each download: to restart its address. */
  private sessionOf = new Map<string, Session>()
  private lastPush = 0

  constructor(private b: Browser) {}

  /** Listens to a session's downloads (called for each session by profiles.ts). */
  attach(ses: Session, opts: { private?: boolean } = {}): void {
    ses.on('will-download', (_e, item, wc) => this.start(item, ses, wc?.id, opts.private ? ses : undefined))
  }

  /** Private window closed: its finished downloads leave the list (the file itself stays). */
  dropPrivate(ses: Session): void {
    const gone = new Set([...this.privateOf].filter(([id, s]) => s === ses && !this.items.has(id)).map(([id]) => id))
    if (gone.size === 0) return
    for (const id of gone) this.privateOf.delete(id)
    const { state } = this.b.store
    state.downloads = state.downloads.filter((d) => !gone.has(d.id))
    this.b.broadcast.changed({ persist: false })
  }

  private get list(): DownloadInfo[] {
    return this.b.store.state.downloads
  }

  private start(item: DownloadItem, ses: Session, contentsId?: number, privateSession?: Session): void {
    const path = this.uniquePath(item.getFilename())
    // "Ask where to save": with no imposed path, Electron opens its save dialog, proposed at this path.
    const ask = this.b.store.state.settings.askDownloadPath === true
    if (ask) item.setSaveDialogOptions({ defaultPath: path })
    else item.setSavePath(path)
    const info: DownloadInfo = {
      id: randomUUID(),
      filename: basename(path),
      path,
      url: item.getURL(),
      status: 'progressing',
      receivedBytes: 0,
      totalBytes: item.getTotalBytes(),
      startedAt: Date.now(),
      ...(privateSession && { private: true })
    }
    if (privateSession) this.privateOf.set(info.id, privateSession)
    this.list.unshift(info)
    this.list.length = Math.min(this.list.length, DOWNLOADS_MAX)
    this.items.set(info.id, item)
    this.sessionOf.set(info.id, ses)

    // Path chosen in the save dialog: known only once the dialog is closed.
    const syncPath = () => {
      const chosen = item.getSavePath()
      if (ask && chosen && chosen !== info.path) {
        info.path = chosen
        info.filename = basename(chosen)
      }
    }
    item.on('updated', (_e, state) => {
      syncPath()
      const before = info.status
      info.receivedBytes = item.getReceivedBytes()
      info.totalBytes = item.getTotalBytes()
      info.status = state === 'interrupted' ? 'interrupted' : item.isPaused() ? 'paused' : 'progressing'
      const now = Date.now()
      // A state change is never held back by the delay.
      if (info.status === before && now - this.lastPush < PUSH_INTERVAL) return
      this.lastPush = now
      // Interrupted with no possible resume: we cancel so that "done" closes the entry instead of leaving it in progress.
      if (info.status === 'interrupted' && !item.canResume()) item.cancel()
      // Plain progress: no layout nor write; a state change (pause, interruption) is persisted.
      this.push(info.status !== before)
    })
    item.once('done', (_e, state) => {
      this.items.delete(info.id)
      // Save dialog canceled: nothing was downloaded, the entry disappears.
      if (ask && state === 'cancelled' && !item.getSavePath()) {
        this.b.store.state.downloads = this.list.filter((d) => d.id !== info.id)
        this.privateOf.delete(info.id)
        this.sessionOf.delete(info.id)
        return this.push()
      }
      syncPath()
      // Private window already closed: the download does not stay in the list once finished.
      if (privateSession && !this.b.privates.hasSession(privateSession)) {
        this.privateOf.delete(info.id)
        this.b.store.state.downloads = this.list.filter((d) => d.id !== info.id)
      }
      info.receivedBytes = item.getReceivedBytes()
      info.totalBytes = item.getTotalBytes()
      info.status = state
      if (state === 'completed') app.dock?.downloadFinished(info.path)
      this.push()
    })

    this.push()
    if (contentsId !== undefined) this.b.tabs.discardBlank(contentsId)
  }

  /** `name.ext` → `name (1).ext`, `name (2).ext`… as long as the file exists or a download reserves it. */
  private uniquePath(filename: string): string {
    const dir = downloadDir(this.b.store.state.settings)
    const reserved = new Set(this.list.filter((d) => this.items.has(d.id)).map((d) => d.path))
    const ext = extname(filename)
    const stem = basename(filename, ext)
    let path = join(dir, filename)
    for (let n = 1; existsSync(path) || reserved.has(path); n++) path = join(dir, `${stem} (${n})${ext}`)
    return path
  }

  private push(persist = true): void {
    this.updateProgress()
    this.b.broadcast.changed({ layout: false, persist })
  }

  /** Dock progress bar: −1 with no download, 2 (indeterminate) if a size is unknown. */
  private updateProgress(): void {
    const win = this.b.win
    if (!win || win.isDestroyed()) return
    const active = this.list.filter((d) => this.items.has(d.id))
    if (active.length === 0) return win.setProgressBar(-1)
    if (active.some((d) => d.totalBytes === 0)) return win.setProgressBar(2)
    const total = active.reduce((sum, d) => sum + d.totalBytes, 0)
    win.setProgressBar(active.reduce((sum, d) => sum + d.receivedBytes, 0) / total)
  }

  open(id: string): void {
    const path = this.list.find((d) => d.id === id)?.path
    if (path && existsSync(path)) void shell.openPath(path)
  }

  reveal(id: string): void {
    const path = this.list.find((d) => d.id === id)?.path
    if (path && existsSync(path)) shell.showItemInFolder(path)
  }

  cancel(id: string): void {
    this.items.get(id)?.cancel()
  }

  /** Resumes an interrupted download; otherwise (or if canceled) restarts its address and replaces the row. */
  retry(id: string): void {
    const info = this.list.find((d) => d.id === id)
    if (!info || info.status === 'completed') return
    const item = this.items.get(id)
    if (item?.canResume()) {
      item.resume()
      return
    }
    const ses = this.sessionOf.get(id)
    if (!ses) return
    const url = info.url
    this.b.store.state.downloads = this.list.filter((d) => d.id !== id)
    this.sessionOf.delete(id)
    this.privateOf.delete(id)
    item?.cancel()
    ses.downloadURL(url)
    this.b.broadcast.changed()
  }

  /** Removes everything except downloads in progress. */
  clear(): void {
    const { state } = this.b.store
    state.downloads = state.downloads.filter((d) => this.items.has(d.id))
    for (const id of this.privateOf.keys()) if (!this.items.has(id)) this.privateOf.delete(id)
    for (const id of this.sessionOf.keys()) if (!this.items.has(id) && !state.downloads.some((d) => d.id === id)) this.sessionOf.delete(id)
    this.b.broadcast.changed()
  }
}

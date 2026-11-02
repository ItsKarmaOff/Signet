import { BrowserWindow, dialog, session, type Session } from 'electron'
import { existsSync, watch, type FSWatcher } from 'node:fs'
import { readdir, stat } from 'node:fs/promises'
import { homedir } from 'node:os'
import { basename, join } from 'node:path'
import type { Browser } from '../../browser/browser'
import { t } from '../../app/i18n'
import { IGNORED, ReloadScheduler, safeReload, type Snapshot } from './unpacked-reload'

/** Delay after the last file change before checking for a real change (a save often touches several files). */
const RELOAD_DELAY = 400
/** How long to wait for Electron to unload an extension, and for its service worker to stop. */
const UNLOAD_TIMEOUT = 3000
const MAX_SCANNED_FILES = 20000

/** Size and modification time of every file of a folder (the watcher's "did anything really change" reference). */
async function scanFolder(root: string): Promise<Snapshot> {
  const out: Snapshot = new Map()
  const walk = async (dir: string, rel: string): Promise<void> => {
    for (const entry of await readdir(dir, { withFileTypes: true })) {
      const relative = rel ? `${rel}/${entry.name}` : entry.name
      if (IGNORED.test(relative) || out.size >= MAX_SCANNED_FILES) continue
      if (entry.isDirectory()) await walk(join(dir, entry.name), relative)
      else if (entry.isFile()) {
        const s = await stat(join(dir, entry.name)).catch(() => null)
        if (s) out.set(relative, `${s.size}:${s.mtimeMs}`)
      }
    }
  }
  await walk(root, '')
  return out
}

const pause = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms))

interface Entry {
  path: string
  /** Identifier once loaded (Chromium derives it from the path: it does not change from one reload to the next). */
  id?: string
  name: string
  error?: string
  reloadedAt?: number
  watcher?: FSWatcher
  scheduler?: ReloadScheduler
  /** Loads and removals of the same extension, one after the other. */
  chain: Promise<void>
}

/**
 * Unpacked extensions (Settings › Extensions, with developer mode): loaded from their folder,
 * reloaded as soon as a file changes, remembered from one launch to the next. Removal never erases the folder.
 * Loading happens in the default session; extensions.ts propagates it to the other sessions.
 */
export class UnpackedExtensions {
  private entries = new Map<string, Entry>()

  constructor(private b: Browser) {}

  private get paths(): string[] {
    return this.b.store.state.settings.unpackedExtensions ?? []
  }

  private savePaths(paths: string[]): void {
    const { settings } = this.b.store.state
    if (paths.length) settings.unpackedExtensions = paths
    else delete settings.unpackedExtensions
    this.b.broadcast.changed()
  }

  /** At startup, once the store extensions are loaded. */
  async init(): Promise<void> {
    for (const path of this.paths) await this.load(path)
  }

  /** Folder of a loaded unpacked extension, or undefined. */
  pathOf(id: string): string | undefined {
    for (const e of this.entries.values()) if (e.id === id) return e.path
    return undefined
  }

  /** State of an unpacked extension (Settings), from its folder. */
  stateOf(path: string): { path: string; display: string; error?: string; reloadedAt?: number } {
    const e = this.entries.get(path)
    const home = homedir()
    return { path, display: path.startsWith(home) ? `~${path.slice(home.length)}` : path, error: e?.error, reloadedAt: e?.reloadedAt }
  }

  /** Extensions remembered but not loaded (folder not found, invalid manifest): name and folder. */
  failed(): { path: string; name: string }[] {
    return this.paths.filter((p) => !this.entries.get(p)?.id).map((p) => ({ path: p, name: this.entries.get(p)?.name ?? basename(p) }))
  }

  /** "Load a folder…": folder choice, manifest check, loading. */
  async pick(): Promise<void> {
    const parent = BrowserWindow.getFocusedWindow()
    const options: Electron.OpenDialogOptions = { title: t('unpacked.pickTitle'), buttonLabel: t('unpacked.pickButton'), properties: ['openDirectory'] }
    const result = parent ? await dialog.showOpenDialog(parent, options) : await dialog.showOpenDialog(options)
    const path = result.filePaths[0]
    if (result.canceled || !path) return
    if (!existsSync(join(path, 'manifest.json'))) return this.alert(t('unpacked.noManifest', { folder: basename(path) }))
    const error = await this.add(path)
    if (error) this.alert(t('unpacked.loadFailed', { folder: basename(path) }), error)
  }

  /** Remembers the folder and loads its extension; returns the load error if there is one. */
  async add(path: string): Promise<string | undefined> {
    if (!this.paths.includes(path)) this.savePaths([...this.paths, path])
    await this.load(path)
    return this.entries.get(path)?.error
  }

  private alert(message: string, detail?: string): void {
    const parent = BrowserWindow.getFocusedWindow()
    const options: Electron.MessageBoxOptions = { type: 'warning', message, detail, buttons: ['OK'] }
    void (parent ? dialog.showMessageBox(parent, options) : dialog.showMessageBox(options))
  }

  private entry(path: string): Entry {
    let e = this.entries.get(path)
    if (!e) {
      const entry: Entry = { path, name: basename(path), chain: Promise.resolve() }
      entry.scheduler = new ReloadScheduler({
        delay: RELOAD_DELAY,
        scan: () => scanFolder(path),
        reload: () => this.reload(path),
        setTimer: (fn, ms) => setTimeout(fn, ms),
        clearTimer: (h) => clearTimeout(h as NodeJS.Timeout)
      })
      e = entry
      this.entries.set(path, e)
    }
    return e
  }

  /** Loads (or reloads) the extension of a folder, then watches its files. */
  private load(path: string, reloading = false): Promise<void> {
    const e = this.entry(path)
    const run = async () => {
      if (!this.paths.includes(path)) return // removed in the meantime
      const ses = session.defaultSession
      const exts = ses.extensions
      // Snapshot BEFORE loading: a file written while loading triggers one more (safe) reload instead of being lost.
      e.scheduler?.baseline(await scanFolder(path).catch(() => new Map()))
      try {
        const old = e.id
        await safeReload({
          isLoaded: () => !!old && !!exts.getExtension(old),
          unload: () => this.unloadAndWait(ses, old!),
          workerStopped: () => this.workerStopped(ses, old!),
          load: async () => {
            const ext = await exts.loadExtension(path, { allowFileAccess: true })
            e.id = ext.id
            e.name = ext.name
          }
        })
        e.error = undefined
        if (reloading) e.reloadedAt = Date.now()
      } catch (err) {
        e.id = undefined
        e.error = err instanceof Error ? err.message : String(err)
      }
      this.watch(e)
      this.b.broadcast.changed({ layout: false, persist: false })
    }
    e.chain = e.chain.then(run, run)
    return e.chain
  }

  /**
   * Removes an extension and resolves once Electron says it is unloaded. Loading it again before that (the previous
   * behaviour) raced Chromium's teardown of the extension, including its running service worker.
   */
  private unloadAndWait(ses: Session, id: string): Promise<void> {
    return new Promise((resolve) => {
      const done = () => {
        clearTimeout(timer)
        ses.extensions.off('extension-unloaded', onUnloaded)
        resolve()
      }
      const onUnloaded = (_e: unknown, ext: { id: string }) => ext.id === id && done()
      const timer = setTimeout(done, UNLOAD_TIMEOUT)
      ses.extensions.on('extension-unloaded', onUnloaded)
      try {
        ses.extensions.removeExtension(id)
      } catch {
        done()
      }
    })
  }

  /** Electron has no call to stop a service worker: removing the extension makes Chromium stop it, so wait until it has. */
  private async workerStopped(ses: Session, id: string): Promise<void> {
    const scope = `chrome-extension://${id}/`
    const running = () => {
      try {
        return Object.values(ses.serviceWorkers.getAllRunning()).some((w) => w.scope.startsWith(scope))
      } catch {
        return false
      }
    }
    for (const start = Date.now(); running() && Date.now() - start < UNLOAD_TIMEOUT;) await pause(100)
    await pause(150) // let Chromium finish the registration cleanup
  }

  reload(path: string): Promise<void> {
    return this.paths.includes(path) ? this.load(path, true) : Promise.resolve()
  }

  /** Removes the extension from Signet; its folder stays as is. */
  remove(path: string): void {
    const e = this.entries.get(path)
    this.savePaths(this.paths.filter((p) => p !== path))
    if (!e) return
    e.watcher?.close()
    e.scheduler?.dispose()
    this.entries.delete(path)
    e.chain = e.chain.then(() => {
      if (e.id) this.b.extensions.forgetData(e.id)
      if (e.id && session.defaultSession.extensions.getExtension(e.id)) session.defaultSession.extensions.removeExtension(e.id)
      this.b.broadcast.changed({ layout: false, persist: false })
    })
  }

  /** Session attached after the initial load (isolated space opened later): it also receives these extensions. */
  loadInto(ses: Session): void {
    for (const e of this.entries.values())
      if (e.id && !ses.extensions.getExtension(e.id)) ses.extensions.loadExtension(e.path, { allowFileAccess: true }).catch(() => {})
  }

  /** Watches the folder: a change reloads the extension (even after a failure, to recover once fixed). */
  private watch(e: Entry): void {
    if (e.watcher || !existsSync(e.path)) return
    try {
      e.watcher = watch(e.path, { recursive: true }, (_event, file) => {
        if (IGNORED.test(String(file ?? ''))) return
        e.scheduler?.notify()
      })
      e.watcher.on('error', () => {
        e.watcher?.close()
        e.watcher = undefined
      })
    } catch {
      // unreadable folder: no automatic reload
    }
  }
}

import { BrowserWindow, dialog, session, type Session } from 'electron'
import { existsSync, watch, type FSWatcher } from 'node:fs'
import { homedir } from 'node:os'
import { basename, join } from 'node:path'
import type { Browser } from '../../browser/browser'
import { t } from '../../app/i18n'

/** Delay after the last file change before reloading (a save often touches several files). */
const RELOAD_DELAY = 400
/** Files whose change does not reload the extension. */
const IGNORED = /(^|[\\/])(\.git|node_modules)([\\/]|$)|\.DS_Store$|~$|\.sw[po]$/

interface Entry {
  path: string
  /** Identifier once loaded (Chromium derives it from the path: it does not change from one reload to the next). */
  id?: string
  name: string
  error?: string
  reloadedAt?: number
  watcher?: FSWatcher
  timer?: NodeJS.Timeout
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
      e = { path, name: basename(path), chain: Promise.resolve() }
      this.entries.set(path, e)
    }
    return e
  }

  /** Loads (or reloads) the extension of a folder, then watches its files. */
  private load(path: string, reloading = false): Promise<void> {
    const e = this.entry(path)
    const run = async () => {
      if (!this.paths.includes(path)) return // removed in the meantime
      const exts = session.defaultSession.extensions
      if (e.id && exts.getExtension(e.id)) exts.removeExtension(e.id)
      try {
        const ext = await exts.loadExtension(path, { allowFileAccess: true })
        e.id = ext.id
        e.name = ext.name
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

  reload(path: string): Promise<void> {
    return this.paths.includes(path) ? this.load(path, true) : Promise.resolve()
  }

  /** Removes the extension from Signet; its folder stays as is. */
  remove(path: string): void {
    const e = this.entries.get(path)
    this.savePaths(this.paths.filter((p) => p !== path))
    if (!e) return
    e.watcher?.close()
    clearTimeout(e.timer)
    this.entries.delete(path)
    e.chain = e.chain.then(() => {
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
        clearTimeout(e.timer)
        e.timer = setTimeout(() => void this.reload(e.path), RELOAD_DELAY)
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

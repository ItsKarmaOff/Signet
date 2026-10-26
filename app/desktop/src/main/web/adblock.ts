import { app, ipcMain, net, webContents, type IpcMainInvokeEvent, type Session } from 'electron'
import { promises as fs } from 'node:fs'
import { join } from 'node:path'
import type { ElectronBlocker } from '@ghostery/adblocker-electron'
import type { Store } from '../core/store'

const MAX_AGE = 7 * 24 * 3_600_000
/** Global `ipcMain` channels that the library (re)declares on each activation in a session. */
const INJECT_CHANNEL = '@ghostery/adblocker/inject-cosmetic-filters'
const COSMETIC_CHANNELS = [INJECT_CHANNEL, '@ghostery/adblocker/is-mutation-observer-enabled']

/**
 * The library runs each scriptlet separately, at the page's global level. Several YouTube scriptlets
 * each declare `class JSONPath`: from the second one on, "already declared", the scriptlet is not executed and
 * the interceptions are left half-installed (the player no longer mounts after an internal navigation). Each
 * scriptlet is therefore wrapped in a block: its `class` and `const` stay local there, its `var` and `function`
 * stay global (as in a browser, where each scriptlet has its own scope).
 */
function scopedInjection(blocker: ElectronBlocker): (event: IpcMainInvokeEvent, url: string, msg?: unknown) => Promise<void> {
  return (event, url, msg) => {
    // Through the main frame rather than the webContents: the latter waits for loading to finish, with two listeners
    // (`did-stop-loading`, `destroyed`) per waiting scriptlet. A heavy page accumulates more than ten of them
    // (MaxListenersExceededWarning), and its scriptlets ran after loading instead of immediately.
    const run = (code: string, userGesture?: boolean) => {
      try {
        const frame = event.sender.isDestroyed() ? null : event.sender.mainFrame
        return frame && !frame.detached ? frame.executeJavaScript(`{\n${code}\n}`, userGesture).catch(() => {}) : Promise.resolve()
      } catch {
        return Promise.resolve() // frame destroyed in the meantime
      }
    }
    const sender = {
      insertCSS: (css: string, options?: Electron.InsertCSSOptions) => event.sender.insertCSS(css, options),
      executeJavaScript: run
    }
    type Inject = ElectronBlocker['onInjectCosmeticFilters']
    return blocker.onInjectCosmeticFilters(
      { ...event, frameId: event.frameId, processId: event.processId, sender } as unknown as Parameters<Inject>[0],
      url,
      msg as Parameters<Inject>[2]
    )
  }
}

/** Ad and tracker blocker (Ghostery), enabled on each configured session (see profiles.ts). */
export class Adblock {
  private loading: Promise<ElectronBlocker> | null = null
  private enabled: ElectronBlocker | null = null
  private cache = join(app.getPath('userData'), 'adblock.bin')
  private sessions = new Set<Session>()
  /** Requests blocked since the main frame's last navigation, per `webContents`. */
  private blocked = new Map<number, number>()

  constructor(private store: Store) {}

  /** Adds a session; it is protected as soon as the blocker is active. */
  attach(ses: Session): void {
    this.sessions.add(ses)
    if (this.enabled) this.activate(this.enabled, ses)
  }

  /** Removes a session (private window closed): it is no longer tracked. */
  detach(ses: Session): void {
    if (!this.sessions.delete(ses)) return
    try {
      if (this.enabled?.isBlockingEnabled(ses)) this.enabled.disableBlockingInSession(ses)
    } catch {
      // session already released
    }
    this.restoreHandlers()
  }

  /**
   * Disabling a session removes the global `ipcMain` handlers, shared by all the others:
   * we redeclare them by re-enabling a remaining session (otherwise no cosmetic filtering anywhere).
   */
  private restoreHandlers(): void {
    const blocker = this.enabled
    if (!blocker) return
    try {
      const rest = [...this.sessions].find((s) => blocker.isBlockingEnabled(s))
      if (!rest) return
      blocker.disableBlockingInSession(rest)
      for (const channel of COSMETIC_CHANNELS) ipcMain.removeHandler(channel)
      blocker.enableBlockingInSession(rest)
      this.scopeScriptlets(blocker)
    } catch (error) {
      console.error('Ad blocker: handlers not restored:', error)
    }
  }

  /**
   * Enables the engine on a session. The library declares global `ipcMain` handlers on each
   * activation and throws on the second session (before having installed the network listeners): we therefore remove the
   * previous ones first. They are bound to the same engine, so the replacement has no effect on the other sessions.
   */
  private activate(blocker: ElectronBlocker, ses: Session): void {
    if (blocker.isBlockingEnabled(ses)) return
    for (const channel of COSMETIC_CHANNELS) ipcMain.removeHandler(channel)
    try {
      blocker.enableBlockingInSession(ses)
      this.scopeScriptlets(blocker)
    } catch (error) {
      console.error('Ad blocker: could not enable on a session:', error)
    }
  }

  /** Replaces the library's injection handler with the version that isolates each scriptlet (see `scopedInjection`). */
  private scopeScriptlets(blocker: ElectronBlocker): void {
    ipcMain.removeHandler(INJECT_CHANNEL)
    ipcMain.handle(INJECT_CHANNEL, scopedInjection(blocker))
  }

  /** Number of requests blocked on the page displayed by this `webContents`. */
  blockedCount(webContentsId: number): number {
    return this.blocked.get(webContentsId) ?? 0
  }

  /** To call when the main frame navigates (or the view is destroyed). */
  resetBlocked(webContentsId: number): void {
    this.blocked.delete(webContentsId)
  }

  /** Loads the engine in the background and aligns the sessions with `settings.adblock`. */
  async sync(): Promise<void> {
    const wanted = this.store.state.settings.adblock
    if (!wanted) {
      const blocker = this.enabled
      this.enabled = null
      if (blocker) for (const ses of this.sessions) if (blocker.isBlockingEnabled(ses)) blocker.disableBlockingInSession(ses)
      for (const channel of COSMETIC_CHANNELS) ipcMain.removeHandler(channel) // none left, even if a session failed
      return
    }
    try {
      const blocker = await this.engine()
      // The user may have toggled during loading.
      if (!this.store.state.settings.adblock || this.enabled) return
      for (const ses of this.sessions) this.activate(blocker, ses)
      this.enabled = blocker
    } catch (error) {
      console.error('Bloqueur de pubs indisponible :', error)
      this.loading = null // we will retry on the next toggle
    }
  }

  private engine(): Promise<ElectronBlocker> {
    this.loading ??= this.load()
    return this.loading
  }

  private async load(): Promise<ElectronBlocker> {
    const blocker = await this.fetchEngine()
    // `tabId` is the identifier of the `webContents` that issued the request.
    blocker.on('request-blocked', (request) => {
      const id = request.tabId
      if (!this.blocked.has(id)) {
        // OAuth popups, extension pages: nobody else resets the counter to zero.
        webContents.fromId(id)?.once('destroyed', () => this.blocked.delete(id))
      }
      this.blocked.set(id, (this.blocked.get(id) ?? 0) + 1)
    })
    return blocker
  }

  private async fetchEngine(): Promise<ElectronBlocker> {
    const stale = await fs.stat(this.cache).then(
      (s) => Date.now() - s.mtimeMs > MAX_AGE,
      () => false
    )
    // Loaded on demand: the library does not weigh down startup.
    const { ElectronBlocker } = await import('@ghostery/adblocker-electron')
    const fromDisk = (ignoreCache: boolean) =>
      ElectronBlocker.fromPrebuiltAdsAndTracking(net.fetch as typeof fetch, {
        path: this.cache,
        read: ignoreCache ? () => Promise.reject(new Error('stale cache')) : fs.readFile,
        write: fs.writeFile
      })
    if (!stale) return fromDisk(false)
    // Cache older than 7 days: we try the network, and keep the old cache if we are offline.
    return fromDisk(true).catch(() => fromDisk(false))
  }
}

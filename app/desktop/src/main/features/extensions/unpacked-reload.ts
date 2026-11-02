// Reload logic of an unpacked extension, kept free of Electron so it can be tested with fakes.
//
// Why this exists: on macOS the folder watcher (FSEvents) replays the events of files written just before the watch
// started, so an extension was reloaded about 400 ms after it was loaded, while its service worker was still starting.
// Unloading and loading an extension under a starting worker killed the whole process in the API probe. A file event
// now only schedules a CHECK; the extension reloads when a file really differs from the snapshot taken before the last
// load, never while another reload runs, and events that arrive meanwhile are coalesced into one more check.

/** Files whose change does not reload the extension. */
export const IGNORED = /(^|[\\/])(\.git|node_modules)([\\/]|$)|\.DS_Store$|~$|\.sw[po]$/

/** Relative path -> `size:mtimeMs`. */
export type Snapshot = Map<string, string>

export function sameSnapshot(a: Snapshot, b: Snapshot): boolean {
  if (a.size !== b.size) return false
  for (const [file, signature] of a) if (b.get(file) !== signature) return false
  return true
}

export interface ReloadDeps {
  /** Reads the current state of the extension folder. */
  scan(): Promise<Snapshot>
  /** Unloads then loads the extension (must resolve when done, even on failure). */
  reload(): Promise<void>
  /** Debounce delay in milliseconds. */
  delay: number
  setTimer(fn: () => void, ms: number): unknown
  clearTimer(handle: unknown): void
}

export class ReloadScheduler {
  private snapshot: Snapshot | null = null
  private timer: unknown
  private busy = false
  private again = false
  private disposed = false

  constructor(private deps: ReloadDeps) {}

  /** Records the state the loaded extension was built from. Take it BEFORE loading, so a write during the load is not lost. */
  baseline(snapshot: Snapshot): void {
    this.snapshot = snapshot
  }

  get running(): boolean {
    return this.busy
  }

  /** A watcher event happened (any file): debounce, then compare. */
  notify(): void {
    if (this.disposed) return
    this.deps.clearTimer(this.timer)
    this.timer = this.deps.setTimer(() => void this.check(), this.deps.delay)
  }

  dispose(): void {
    this.disposed = true
    this.deps.clearTimer(this.timer)
  }

  /** Compares the folder with the snapshot and reloads if it differs. Safe to call at any time. */
  async check(): Promise<void> {
    if (this.disposed) return
    if (this.busy) {
      this.again = true
      return
    }
    this.busy = true
    try {
      do {
        this.again = false
        let current: Snapshot
        try {
          current = await this.deps.scan()
        } catch {
          return // folder unreadable: try again at the next event
        }
        // No baseline (the load failed): any event may be the fix, so reload.
        if (this.snapshot && sameSnapshot(this.snapshot, current)) continue
        this.snapshot = current
        await this.deps.reload()
        if (this.disposed) return
      } while (this.again)
    } finally {
      this.busy = false
    }
  }
}

/** Order of a safe reload, injected so it can be tested. */
export interface SafeReloadDeps {
  /** True while the extension is registered in the session. */
  isLoaded(): boolean
  /** Removes it; resolves once Electron emitted `extension-unloaded` (or after `timeout`). */
  unload(): Promise<void>
  /** Resolves when no service worker of the extension is running anymore (or after `timeout`). */
  workerStopped(): Promise<void>
  load(): Promise<void>
}

/** Unload, wait for the worker to stop, then load: never load over a live registration. */
export async function safeReload(deps: SafeReloadDeps): Promise<void> {
  if (deps.isLoaded()) {
    await deps.unload()
    await deps.workerStopped()
  }
  await deps.load()
}

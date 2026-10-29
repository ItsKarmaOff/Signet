import { readFileSync } from 'node:fs'
import type { Session } from 'electron'
import { AtomicWriter } from '../../../core/atomic'
import type { ExtensionEmitter } from './types'
import { ApiError } from './types'

/** One `ChromeSetting` (`chrome.proxy.settings`, `chrome.privacy.network.webRTCIPHandlingPolicy`…). */
export interface SettingDef {
  /** Dotted API path, also the method prefix (`proxy.settings` -> `proxy.settings.get`). */
  key: string
  /** Manifest permission required to read or write it. */
  permission: string
  defaultValue: unknown
  /** Returns the clean value or throws an `ApiError`. */
  parse(value: unknown): unknown
  /** Makes the value effective in a regular session; settings with no effect leave it out. */
  apply?(ses: Session, value: unknown): void | Promise<void>
}

export type LevelOfControl = 'controlled_by_this_extension' | 'controllable_by_this_extension' | 'controlled_by_other_extensions'

export interface SettingDetails {
  value: unknown
  levelOfControl: LevelOfControl
  incognitoSpecific: boolean
}

interface Controller {
  extensionId: string
  value: unknown
}

interface Stored {
  version: 1
  settings: Record<string, Controller>
}

/**
 * State of every ChromeSetting: at most one extension controls each (the last one to set it wins; Chrome orders them by
 * install time, which would need an install clock we do not keep). Persisted so the control survives a restart: the
 * value is applied again when the controlling extension loads.
 *
 * Incognito scopes are accepted and ignored: private windows run without extensions and without these settings.
 */
export class ChromeSettings {
  private defs = new Map<string, SettingDef>()
  private state = new Map<string, Controller>()
  private writer: AtomicWriter

  constructor(
    private file: string,
    defs: SettingDef[],
    private emitter: ExtensionEmitter,
    /** Regular sessions the effective value must be applied to. */
    private sessions: () => Iterable<Session>
  ) {
    for (const def of defs) this.defs.set(def.key, def)
    this.load()
    this.writer = new AtomicWriter(file, () => this.serialize())
  }

  private load(): void {
    try {
      const data = JSON.parse(readFileSync(this.file, 'utf8')) as Partial<Stored>
      for (const [key, c] of Object.entries(data.settings ?? {})) {
        const def = this.defs.get(key)
        if (!def || typeof c?.extensionId !== 'string') continue
        try {
          this.state.set(key, { extensionId: c.extensionId, value: def.parse(c.value) })
        } catch {
          // stale or corrupt value: dropped
        }
      }
    } catch {
      // no file yet
    }
  }

  private serialize(): string {
    const settings: Stored['settings'] = {}
    for (const [key, c] of this.state) settings[key] = c
    return JSON.stringify({ version: 1, settings } satisfies Stored)
  }

  /** Writes pending changes now (shutdown, tests). */
  flush(): boolean {
    return this.writer.flushSync()
  }

  has(key: string): boolean {
    return this.defs.has(key)
  }

  permissionOf(key: string): string | undefined {
    return this.defs.get(key)?.permission
  }

  private def(key: string): SettingDef {
    const def = this.defs.get(key)
    if (!def) throw new ApiError(`Unknown setting ${key}`)
    return def
  }

  /** Value in force: the controller's, else the default. */
  effective(key: string): unknown {
    return this.state.get(key)?.value ?? this.def(key).defaultValue
  }

  controllerOf(key: string): string | undefined {
    return this.state.get(key)?.extensionId
  }

  levelOf(key: string, extensionId: string): LevelOfControl {
    const owner = this.state.get(key)?.extensionId
    if (!owner) return 'controllable_by_this_extension'
    return owner === extensionId ? 'controlled_by_this_extension' : 'controlled_by_other_extensions'
  }

  details(key: string, extensionId: string): SettingDetails {
    return { value: this.effective(key), levelOfControl: this.levelOf(key, extensionId), incognitoSpecific: false }
  }

  // ── chrome.*.set / clear ──

  set(key: string, extensionId: string, details: unknown): void {
    const def = this.def(key)
    const value = def.parse((details as { value?: unknown } | null)?.value)
    this.change(key, extensionId, { extensionId, value })
  }

  clear(key: string, extensionId: string): void {
    // Only the controlling extension can give the setting back.
    if (this.state.get(key)?.extensionId === extensionId) this.change(key, extensionId, undefined)
  }

  /** Extension unloaded or uninstalled: it no longer controls anything. */
  release(extensionId: string): void {
    for (const [key, c] of [...this.state]) if (c.extensionId === extensionId) this.change(key, extensionId, undefined)
  }

  private change(key: string, caller: string, next: Controller | undefined): void {
    const before = this.state.get(key)
    const audience = new Set([...this.emitter.extensionsWith(this.def(key).permission), caller, before?.extensionId ?? caller])
    const snapshot = () => new Map([...audience].map((id) => [id, JSON.stringify(this.details(key, id))]))
    const was = snapshot()
    if (next) this.state.set(key, next)
    else this.state.delete(key)
    this.writer.request()
    this.applyAll(key)
    // Listeners hear about their own level of control as well as the value. The extensions whose control changed hand
    // are woken; the others only get the event if they are running (a stopped worker will read the state when it asks).
    const now = snapshot()
    for (const id of audience) {
      if (was.get(id) === now.get(id)) continue
      const wake = id === before?.extensionId || id === next?.extensionId
      this.emitter.emit(id, `${key}.onChange`, [this.details(key, id)], wake)
    }
  }

  // ── applying ──

  private applyTo(ses: Session, key: string): void {
    const def = this.def(key)
    if (!def.apply) return
    Promise.resolve()
      .then(() => def.apply!(ses, this.effective(key)))
      .catch((e) => console.error(`Extension setting ${key}: cannot apply`, e))
  }

  private applyAll(key: string): void {
    for (const ses of this.sessions()) this.applyTo(ses, key)
  }

  /** A controlling extension loaded in a session (startup, new session): apply what it set there. */
  reapply(ses: Session, extensionId: string): void {
    for (const [key, c] of this.state) if (c.extensionId === extensionId) this.applyTo(ses, key)
  }

  /** Settings that currently have a controller, with their value (for contexts created later, e.g. new tabs). */
  active(): { key: string; def: SettingDef; value: unknown }[] {
    return [...this.state].map(([key, c]) => ({ key, def: this.def(key), value: c.value }))
  }
}

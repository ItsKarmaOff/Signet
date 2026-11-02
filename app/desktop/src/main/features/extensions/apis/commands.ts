import {
  EXECUTE_COMMANDS,
  readCommands,
  resolveCommands,
  type CommandPlatform,
  type ExtensionCommandSet,
  type ResolvedCommand
} from '@shared/extension-commands'
import type { ApiHandler, ExtensionEmitter } from './types'

export const COMMAND_EVENT = 'commands.onCommand'

export interface CommandsDeps {
  platform: CommandPlatform
  emitter: ExtensionEmitter
  /** Signet's own shortcuts as they are now (identity key to owner label), user overrides included. */
  signetKeys(): ReadonlyMap<string, string>
  /** Text of an extension's manifest, `__MSG_key__` resolved in its locale. */
  localize(extensionId: string, text: string): string
  /** Does what a click on the extension's toolbar button does, for `target` (what the shortcut was pressed on). */
  runAction(extensionId: string, target: unknown): void
  /** The tab in Chrome's `tabs.Tab` shape, if there is one. */
  activeTab(target: unknown): unknown
  /** Development only: where clash lines go. */
  log(message: string): void
}

export interface CommandInfo {
  name: string
  description: string
  shortcut: string
}

export interface CommandBinding {
  accel: string
  extensionId: string
  name: string
}

/**
 * `chrome.commands`: the commands of every loaded extension and who owns each shortcut. Nothing is cached about
 * ownership: it is worked out from the registered extensions and Signet's current shortcuts each time, so a change in
 * either side takes effect without any bookkeeping.
 */
export class ExtensionCommands {
  /** Insertion order is registration order: the first extension to register keeps a contested shortcut. */
  private sets = new Map<string, ExtensionCommandSet>()
  private logged = new Set<string>()

  constructor(private d: CommandsDeps) {}

  /** An extension was loaded (or reloaded: it keeps its place in line). */
  register(id: string, name: string, manifest: unknown): void {
    this.sets.set(id, { id, name, commands: readCommands(manifest, this.d.platform) })
  }

  unregister(id: string): void {
    this.sets.delete(id)
  }

  private resolve(): ResolvedCommand[] {
    const { commands, clashes } = resolveCommands([...this.sets.values()], this.d.signetKeys(), this.d.platform)
    for (const c of clashes) {
      const line = `[extension-commands] ${c.extensionId} ${c.command} ${c.accel} taken by ${c.owner}`
      if (!this.logged.has(line)) {
        this.logged.add(line)
        this.d.log(line)
      }
    }
    return commands
  }

  private info(c: ResolvedCommand): CommandInfo {
    return { name: c.name, description: this.d.localize(c.extensionId, c.description), shortcut: c.shortcut }
  }

  /** `commands.getAll` for an extension. */
  listFor(id: string): CommandInfo[] {
    return this.resolve()
      .filter((c) => c.extensionId === id)
      .map((c) => this.info(c))
  }

  /** Commands of every extension that has any (Settings). */
  listAll(): Map<string, CommandInfo[]> {
    const out = new Map<string, CommandInfo[]>()
    for (const c of this.resolve()) out.set(c.extensionId, [...(out.get(c.extensionId) ?? []), this.info(c)])
    return out
  }

  /** Shortcuts to register in the application menu. */
  bindings(): CommandBinding[] {
    return this.resolve().flatMap((c) => (c.accel ? [{ accel: c.accel, extensionId: c.extensionId, name: c.name }] : []))
  }

  /** Changes whenever the menu has to be rebuilt. */
  key(): string {
    return this.bindings()
      .map((b) => `${b.accel}=${b.extensionId}/${b.name}`)
      .join()
  }

  /** A shortcut was pressed; `target` is handed back to the hooks (the tab it was pressed on). */
  run(extensionId: string, name: string, target?: unknown): void {
    if (EXECUTE_COMMANDS.has(name)) return this.d.runAction(extensionId, target)
    const tab = this.d.activeTab(target)
    // Wakes a stopped service worker, like alarms; open extension pages receive it too.
    this.d.emitter.emit(extensionId, COMMAND_EVENT, tab ? [name, tab] : [name], true)
  }

  /** Router handlers. */
  methods(): Record<string, ApiHandler> {
    return { 'commands.getAll': ({ extensionId }) => this.listFor(extensionId) }
  }
}

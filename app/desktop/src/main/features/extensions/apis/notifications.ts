import { ApiError, type ApiCaller, type ApiHandler, type ExtensionEmitter } from './types'

export const CLICKED_EVENT = 'notifications.onClicked'
export const BUTTON_EVENT = 'notifications.onButtonClicked'
export const CLOSED_EVENT = 'notifications.onClosed'

/** Chrome shows at most two buttons. */
export const MAX_BUTTONS = 2

export interface ChromeNotification {
  type: string
  iconUrl?: string
  title: string
  message: string
  contextMessage?: string
  priority?: number
  silent?: boolean
  requireInteraction?: boolean
  items?: { title: string; message: string }[]
  buttons?: { title: string }[]
}

export type Platform = 'darwin' | 'win32' | 'linux'

/** What we hand to Electron's `Notification`. */
export interface ElectronNotification {
  title: string
  subtitle?: string
  body: string
  silent?: boolean
  icon?: string
  urgency: 'low' | 'normal' | 'critical'
  timeoutType: 'default' | 'never'
  actions?: { type: 'button'; text: string }[]
}

export function urgencyOf(priority?: number): 'low' | 'normal' | 'critical' {
  if (typeof priority !== 'number') return 'normal'
  return priority >= 2 ? 'critical' : priority < 0 ? 'low' : 'normal'
}

/** Buttons Electron can show: macOS only (Windows would need toast XML, Linux ignores them). At most two. */
export function buttonsOf(opts: Pick<ChromeNotification, 'buttons'>, platform: Platform): { title: string }[] {
  if (platform !== 'darwin') return []
  return (opts.buttons ?? []).filter((b) => b && typeof b.title === 'string').slice(0, MAX_BUTTONS)
}

/** Chrome notification options to Electron's. `icon` is a path or data URL already checked by the caller. */
export function toElectron(opts: ChromeNotification, platform: Platform, appName: string, icon?: string): ElectronNotification {
  const body = opts.type === 'list' && Array.isArray(opts.items) ? opts.items.map((i) => `${i.title} - ${i.message}`).join('\n') : (opts.message ?? '')
  const buttons = buttonsOf(opts, platform)
  return {
    title: opts.title,
    subtitle: opts.contextMessage || appName,
    body,
    ...(opts.silent !== undefined && { silent: opts.silent }),
    ...(icon && { icon }),
    urgency: urgencyOf(opts.priority),
    timeoutType: opts.requireInteraction ? 'never' : 'default',
    ...(buttons.length && { actions: buttons.map((b) => ({ type: 'button' as const, text: b.title })) })
  }
}

/**
 * Electron's `action` event gives the index among the actions we passed, which are the buttons we kept: the same indexes
 * as the extension's own list, because we only drop buttons from the end.
 */
export function buttonIndex(index: number, opts: Pick<ChromeNotification, 'buttons'>, platform: Platform): number | undefined {
  return Number.isInteger(index) && index >= 0 && index < buttonsOf(opts, platform).length ? index : undefined
}

/** Options of `update` laid over the ones the notification has; `type` cannot change. */
export function mergeOptions(current: ChromeNotification, changes: Partial<ChromeNotification>): ChromeNotification {
  const next: ChromeNotification = { ...current }
  for (const [k, v] of Object.entries(changes)) if (v !== undefined && k !== 'type') (next as unknown as Record<string, unknown>)[k] = v
  return next
}

export interface ShownNotification {
  show(): void
  close(): void
  on(event: 'click' | 'close', listener: () => void): unknown
  on(event: 'action', listener: (event: unknown, index: number) => void): unknown
}

export interface NotificationsDeps {
  emitter: Pick<ExtensionEmitter, 'emit'>
  platform: Platform
  appName: string
  /** Creates a notification (Electron's `Notification`). */
  make(options: ElectronNotification): ShownNotification
  /** A path or data URL for an icon, or null when it is not the extension's own file. */
  icon(caller: ApiCaller, iconUrl: string): string | null
  supported(): boolean
}

interface Entry {
  opts: ChromeNotification
  shown: ShownNotification
}

/** `chrome.notifications`: Signet's version replaces the library's (no buttons, no update, a fixed id). */
export class ExtensionNotifications {
  private entries = new Map<string, Map<string, Entry>>()
  private counter = 0

  constructor(private d: NotificationsDeps) {}

  private of(extensionId: string): Map<string, Entry> {
    let m = this.entries.get(extensionId)
    if (!m) this.entries.set(extensionId, (m = new Map()))
    return m
  }

  private present(caller: ApiCaller, id: string, opts: ChromeNotification): void {
    const { extensionId } = caller
    const icon = opts.iconUrl ? this.d.icon(caller, opts.iconUrl) : undefined
    if (opts.iconUrl && !icon) throw new ApiError('Unable to download all specified images.')
    const shown = this.d.make(toElectron(opts, this.d.platform, this.d.appName, icon ?? undefined))
    const entry: Entry = { opts, shown }
    this.of(extensionId).set(id, entry)
    // An entry that was replaced no longer speaks for this id.
    const current = (): boolean => this.entries.get(extensionId)?.get(id) === entry
    shown.on('click', () => current() && this.d.emitter.emit(extensionId, CLICKED_EVENT, [id], true))
    shown.on('action', (_e, index) => {
      const i = buttonIndex(index, opts, this.d.platform)
      if (current() && i !== undefined) this.d.emitter.emit(extensionId, BUTTON_EVENT, [id, i], true)
    })
    shown.on('close', () => {
      if (!current()) return
      this.entries.get(extensionId)?.delete(id)
      this.d.emitter.emit(extensionId, CLOSED_EVENT, [id, true], true)
    })
    shown.show()
  }

  create(caller: ApiCaller, first: unknown, second?: unknown): string {
    const named = typeof first === 'string'
    const opts = (named ? second : first) as ChromeNotification | undefined
    if (!opts || typeof opts !== 'object' || !opts.type || !opts.title || !opts.message) throw new ApiError('Missing required notification options.')
    if (!['basic', 'image', 'list', 'progress'].includes(opts.type)) throw new ApiError(`Invalid notification type: ${opts.type}.`)
    const id = named && first ? (first as string) : `signet-notification-${++this.counter}`
    // Same id: the old one is replaced without telling the extension it was closed.
    const old = this.entries.get(caller.extensionId)?.get(id)
    this.entries.get(caller.extensionId)?.delete(id)
    old?.shown.close()
    this.present(caller, id, opts)
    return id
  }

  /** Electron cannot change a notification that is showing: it is shown again with the merged options. */
  update(caller: ApiCaller, id: unknown, changes: unknown): boolean {
    const entry = typeof id === 'string' ? this.entries.get(caller.extensionId)?.get(id) : undefined
    if (!entry || typeof id !== 'string') return false
    const opts = mergeOptions(entry.opts, (changes ?? {}) as Partial<ChromeNotification>)
    this.entries.get(caller.extensionId)?.delete(id)
    entry.shown.close()
    this.present(caller, id, opts)
    return true
  }

  clear(caller: ApiCaller, id: unknown): boolean {
    const entry = typeof id === 'string' ? this.entries.get(caller.extensionId)?.get(id) : undefined
    if (!entry || typeof id !== 'string') return false
    this.entries.get(caller.extensionId)?.delete(id)
    entry.shown.close()
    this.d.emitter.emit(caller.extensionId, CLOSED_EVENT, [id, false], true)
    return true
  }

  getAll(caller: ApiCaller): Record<string, true> {
    return Object.fromEntries([...(this.entries.get(caller.extensionId)?.keys() ?? [])].map((id) => [id, true as const]))
  }

  /** The extension is gone: its notifications go with it, silently. */
  unload(extensionId: string): void {
    const entries = this.entries.get(extensionId)
    this.entries.delete(extensionId)
    for (const entry of entries?.values() ?? []) entry.shown.close()
  }

  methods(): Record<string, ApiHandler> {
    return {
      'notifications.create': (c, a, b) => this.create(c, a, b),
      'notifications.update': (c, id, changes) => this.update(c, id, changes),
      'notifications.clear': (c, id) => this.clear(c, id),
      'notifications.getAll': (c) => this.getAll(c),
      'notifications.getPermissionLevel': () => (this.d.supported() ? 'granted' : 'denied')
    }
  }
}

import { BrowserWindow, dialog, type Session, type WebContents } from 'electron'
import type { PermissionState } from '@shared/types'
import type { Store } from '../core/store'
import { t } from '../app/i18n'
import { attachDisplayMedia } from './displaymedia'

/** Allowed without asking. */
const FREE = new Set(['clipboard-sanitized-write', 'fullscreen', 'pointerLock'])

/** Permissions we know how to ask for (the request text comes from the catalog). */
const ASKABLE = new Set(['media', 'camera', 'microphone', 'geolocation', 'notifications', 'clipboard-read', 'midi', 'display-capture'])

type Decision = 'allow' | 'deny'
type Table = Record<string, Record<string, Decision>>

/** Origine http(s) exploitable, ou null (origine opaque `null`, about:, data:, file:, etc.). */
function webOrigin(value: string | undefined): string | null {
  if (!value) return null
  try {
    const u = new URL(value)
    return u.protocol === 'https:' || u.protocol === 'http:' ? u.origin : null
  } catch {
    return null
  }
}

/** Schemes never handed to an external application. */
const BLOCKED_EXTERNAL = new Set([
  'javascript:',
  'file:',
  'data:',
  'vbscript:',
  'blob:',
  'about:',
  'view-source:',
  'devtools:',
  'signet:',
  'signetui:',
  'chrome:'
])

/** Origin of a URL (`https://example.com`), or the raw string if unexpected. */
export function originOf(url: string): string {
  try {
    return new URL(url).origin
  } catch {
    return url
  }
}

/**
 * Permission decisions remembered per origin (in `store.state.sitePermissions`).
 * The identifiers are Electron's, except `media` which splits into `camera` and `microphone`.
 */
export class Permissions {
  /** Pending requests: a single dialog per `origin|permission`. */
  private asking = new Map<string, Promise<Decision>>()

  /** Tells whether the view is displayed (a view with no tab, Peek and Little Signet included, counts as displayed). */
  private isShown: (wc: WebContents) => boolean = () => true
  /** Requests waiting for their tab to come to the foreground. */
  private waiting = new Set<() => void>()
  /** `webContents` that obtained the camera, the microphone or screen sharing. */
  private devices = new Set<number>()
  /** Number of each in-memory table (private windows), to keep pending requests apart. */
  private scopes = new WeakMap<Table, number>()
  private nextScope = 1

  constructor(private store: Store) {}

  setVisibility(isShown: (wc: WebContents) => boolean): void {
    this.isShown = isShown
  }

  /** To call when the displayed tabs change: wakes up the waiting requests. */
  onLayout(): void {
    for (const check of [...this.waiting]) check()
  }

  /** Waits for the tab to be displayed; false if it is closed in the meantime. */
  private async whenShown(wc: WebContents): Promise<boolean> {
    if (wc.isDestroyed()) return false
    if (this.isShown(wc)) return true
    return new Promise<boolean>((resolve) => {
      const done = (ok: boolean) => {
        this.waiting.delete(check)
        if (!wc.isDestroyed()) wc.removeListener('destroyed', onGone)
        resolve(ok)
      }
      const check = () => {
        if (wc.isDestroyed()) done(false)
        else if (this.isShown(wc)) done(true)
      }
      const onGone = () => done(false)
      this.waiting.add(check)
      wc.once('destroyed', onGone)
    })
  }

  private externalBusy = new Set<number>()
  private externalUntil = new Map<number, number>()

  private get table(): Table {
    return this.store.state.sitePermissions
  }

  /** `media` groups camera and microphone: denied if either is denied, granted if both are. */
  get(origin: string, permission: string, table: Table = this.table): Decision | undefined {
    const own = this.getIn(origin, permission, table)
    // Private window: denials from the normal table also apply (read-only, never rewritten).
    if (own !== 'deny' && table !== this.table && this.getIn(origin, permission, this.table) === 'deny') return 'deny'
    return own
  }

  private getIn(origin: string, permission: string, table: Table): Decision | undefined {
    const entry = table[origin]
    if (!entry) return undefined
    if (permission === 'media') {
      const states = [entry.camera ?? entry.media, entry.microphone ?? entry.media]
      if (states.includes('deny')) return 'deny'
      return states.every((s) => s === 'allow') ? 'allow' : undefined
    }
    return entry[permission]
  }

  set(origin: string, permission: string, state: PermissionState, table: Table = this.table): void {
    if (origin === 'null') return // never a decision for an opaque origin
    const names = permission === 'media' ? ['camera', 'microphone'] : [permission]
    const entry = (table[origin] ??= {})
    for (const name of names) {
      if (state === 'ask') delete entry[name]
      else entry[name] = state
    }
    if (permission === 'media') delete entry.media // ancien format
    if (Object.keys(entry).length === 0) delete table[origin]
    if (table === this.table) this.store.schedule()
  }

  /** All remembered decisions (private windows excluded: they stay in memory). */
  listAll(): { origin: string; permission: string; decision: Decision }[] {
    const out: { origin: string; permission: string; decision: Decision }[] = []
    for (const [origin, entry] of Object.entries(this.table))
      for (const [permission, decision] of Object.entries(entry)) if (decision === 'allow' || decision === 'deny') out.push({ origin, permission, decision })
    return out.sort((a, b) => a.origin.localeCompare(b.origin) || a.permission.localeCompare(b.permission))
  }

  /** Forgets one decision, all those of an origin, or everything (no argument). */
  reset(origin?: string, permission?: string): void {
    if (!origin) {
      for (const o of Object.keys(this.table)) delete this.table[o]
    } else if (!permission) {
      delete this.table[origin]
    } else {
      this.set(origin, permission, 'ask')
      return
    }
    this.store.schedule()
  }

  list(origin: string): Record<string, Decision> {
    return { ...this.table[origin] }
  }

  /** True if the page obtained the camera, the microphone or screen sharing (it must not be unloaded). */
  usesDevices(wc: WebContents): boolean {
    return this.devices.has(wc.id)
  }

  /**
   * Wires the request and check handlers onto a session.
   * `ephemeral` (private window): decisions kept in memory for this session only, never written.
   */
  attach(ses: Session, opts: { ephemeral?: boolean } = {}): void {
    const table = opts.ephemeral ? ({} as Table) : undefined
    ses.setPermissionRequestHandler((wc, permission, callback, details) => {
      if (permission === 'openExternal') {
        const url = 'externalURL' in details ? details.externalURL : undefined
        this.confirmExternal(wc, url).then(callback, () => callback(false))
        return
      }
      // Always granted, even without a web origin (interface views, loaded from a file): like the check below.
      if (FREE.has(permission)) return callback(true)
      const origin = this.originFor(wc, 'securityOrigin' in details ? details.securityOrigin : undefined, details.requestingUrl)
      if (!origin) return callback(false)
      this.request(wc, permission, origin, 'mediaTypes' in details ? details.mediaTypes : undefined, table).then(
        (granted) => {
          if (granted && (permission === 'media' || permission === 'display-capture')) this.markDevice(wc)
          callback(granted)
        },
        () => callback(false)
      )
    })
    ses.setPermissionCheckHandler((wc, permission, requestingOrigin, details) => {
      if (FREE.has(permission)) return true
      const origin = this.originFor(wc, 'securityOrigin' in details ? details.securityOrigin : undefined, requestingOrigin)
      if (!origin) return false
      const names = this.names(permission, 'mediaType' in details && details.mediaType ? [details.mediaType] : undefined)
      return names.length > 0 && names.every((n) => this.get(origin, n, table) === 'allow')
    })
    // Screen sharing (getDisplayMedia): same `display-capture` decision as above.
    attachDisplayMedia(ses, async (wc, frameUrl) => {
      const origin = this.originFor(wc, frameUrl)
      if (!origin || !(await this.request(wc, 'display-capture', origin, undefined, table))) return false
      this.markDevice(wc)
      return true
    })
  }

  /** Notes that the page holds a device (it must not be unloaded). */
  private markDevice(wc: WebContents): void {
    if (wc.isDestroyed() || this.devices.has(wc.id)) return
    const id = wc.id
    this.devices.add(id)
    wc.once('destroyed', () => this.devices.delete(id))
  }

  /**
   * Origin to attach the decision to. An opaque origin (`null`: about:blank, srcdoc, data:, file:)
   * is replaced by the top-level page's if it is http(s), otherwise denied (null).
   */
  private originFor(wc: WebContents | null, ...candidates: (string | undefined)[]): string | null {
    for (const c of candidates) {
      const origin = webOrigin(c)
      if (origin) return origin
    }
    if (!wc || wc.isDestroyed()) return null
    return webOrigin(wc.getURL())
  }

  /** Link to an external application (mailto:, tel:, zoommtg:...): confirmation every time, never remembered. */
  private async confirmExternal(wc: WebContents, externalUrl: string | undefined): Promise<boolean> {
    let protocol: string
    try {
      protocol = new URL(externalUrl ?? '').protocol
    } catch {
      return false
    }
    if (BLOCKED_EXTERNAL.has(protocol) || protocol === 'http:' || protocol === 'https:') return false
    // A single dialog per tab; repeats ignored while it is open and shortly after a denial.
    const wcId = wc.id
    if (this.externalBusy.has(wcId) || Date.now() < (this.externalUntil.get(wcId) ?? 0)) return false
    this.externalBusy.add(wcId)
    try {
      return await this.askExternal(wc, externalUrl ?? '', protocol)
    } finally {
      this.externalBusy.delete(wcId)
    }
  }

  private async askExternal(wc: WebContents, externalUrl: string, protocol: string): Promise<boolean> {
    if (!(await this.whenShown(wc))) return false
    const parent = (!wc.isDestroyed() && BrowserWindow.fromWebContents(wc)) || BrowserWindow.getFocusedWindow()
    const link = (externalUrl ?? '').length > 300 ? `${(externalUrl ?? '').slice(0, 300)}…` : (externalUrl ?? '')
    const options = {
      type: 'question' as const,
      message: t('dialog.external.message', { scheme: protocol.replace(/:$/, '') }),
      detail: t('dialog.external.detail', { url: link }),
      buttons: [t('dialog.external.open'), t('dialog.permission.deny')],
      defaultId: 1,
      cancelId: 1
    }
    const { response } = parent ? await dialog.showMessageBox(parent, options) : await dialog.showMessageBox(options)
    if (response !== 0) {
      this.externalUntil.set(wc.id, Date.now() + 5000)
      if (this.externalUntil.size > 50) for (const [k, at] of this.externalUntil) if (Date.now() > at) this.externalUntil.delete(k)
    }
    return response === 0
  }

  private scopeOf(table: Table): number {
    if (table === this.table) return 0
    let n = this.scopes.get(table)
    if (!n) this.scopes.set(table, (n = this.nextScope++))
    return n
  }

  /** Concrete permissions targeted by a request (`media` → camera and/or microphone). */
  private names(permission: string, mediaTypes?: readonly string[]): string[] {
    if (permission !== 'media') return ASKABLE.has(permission) ? [permission] : []
    const names = new Set<string>()
    for (const type of mediaTypes ?? []) {
      if (type === 'video') names.add('camera')
      if (type === 'audio') names.add('microphone')
    }
    return names.size ? [...names] : ['camera', 'microphone']
  }

  private async request(wc: WebContents, permission: string, origin: string, mediaTypes?: readonly string[], table: Table = this.table): Promise<boolean> {
    if (FREE.has(permission)) return true
    const names = this.names(permission, mediaTypes)
    if (names.length === 0) return false
    const known = names.map((n) => this.get(origin, n, table))
    if (known.includes('deny')) return false
    if (known.every((s) => s === 'allow')) return true

    // A private request is never merged with another session's.
    // While the tab is hidden, the request is its own (its dialog is not displayed yet).
    const key = `${this.scopeOf(table)}|${origin}|${names.join('+')}${this.isShown(wc) ? '' : `|wc${wc.id}`}`
    let pending = this.asking.get(key)
    if (!pending) {
      pending = this.ask(wc, origin, names, table).finally(() => this.asking.delete(key))
      this.asking.set(key, pending)
    }
    return (await pending) === 'allow'
  }

  private async ask(wc: WebContents, origin: string, names: string[], table: Table): Promise<Decision> {
    // Background tab: the dialog waits for it to be displayed; if closed in the meantime, denied without remembering.
    if (!(await this.whenShown(wc))) return 'deny'
    const key = names.length === 2 && names.includes('camera') ? 'media' : (names[0] as 'media')
    const parent = (!wc.isDestroyed() && BrowserWindow.fromWebContents(wc)) || BrowserWindow.getFocusedWindow()
    const options = {
      type: 'question' as const,
      message: t(`dialog.permission.${key}`, { origin: origin.replace(/^\w+:\/\//, '') }),
      detail: t('dialog.permission.detail'),
      buttons: [t('dialog.permission.allow'), t('dialog.permission.deny'), t('dialog.permission.later')],
      defaultId: 2,
      cancelId: 2
    }
    const { response } = parent ? await dialog.showMessageBox(parent, options) : await dialog.showMessageBox(options)
    // "Not now" (or Esc) denies this request only: nothing is remembered.
    if (response === 2) return 'deny'
    const decision: Decision = response === 0 ? 'allow' : 'deny'
    for (const name of names) this.set(origin, name, decision, table)
    return decision
  }
}

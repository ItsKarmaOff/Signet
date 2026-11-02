import type { Session } from 'electron'

/**
 * Electron keeps ONE listener per `session.webRequest` event: a second registration silently replaces the first.
 * The hub owns the real listeners of a session and dispatches to several named consumers (ad blocker,
 * extensions' declarativeNetRequest, HTTPS-only, request headers, developer mode...).
 */

/** Run order of the blocking consumers (lower runs first). Gaps are left on purpose. */
export const NetworkOrder = {
  httpsOnly: 50,
  adblock: 100,
  dnr: 200,
  headers: 300,
  devmode: 900
} as const

/** A handler that never answers must not hang the request. */
export const HANDLER_TIMEOUT_MS = 5_000

export interface NetworkFilter {
  urls: string[]
  /** Resource types (`mainFrame`, `script`...); all of them if absent. */
  types?: string[]
}

type Reply = Electron.CallbackResponse & Electron.BeforeSendResponse & Electron.HeadersReceivedResponse

interface BlockingEvents {
  onBeforeRequest: { details: Electron.OnBeforeRequestListenerDetails; headers: null }
  onBeforeSendHeaders: { details: Electron.OnBeforeSendHeadersListenerDetails; headers: 'requestHeaders' }
  onHeadersReceived: { details: Electron.OnHeadersReceivedListenerDetails; headers: 'responseHeaders' }
}
interface ObserverEvents {
  onCompleted: Electron.OnCompletedListenerDetails
  onErrorOccurred: Electron.OnErrorOccurredListenerDetails
  onBeforeRedirect: Electron.OnBeforeRedirectListenerDetails
  onResponseStarted: Electron.OnResponseStartedListenerDetails
}

export type BlockingEvent = keyof BlockingEvents
export type ObserverEvent = keyof ObserverEvents
export type NetworkEvent = BlockingEvent | ObserverEvent
type Details<E extends NetworkEvent> = E extends BlockingEvent ? BlockingEvents[E]['details'] : ObserverEvents[E & ObserverEvent]
export type BlockingHandler<E extends BlockingEvent> = (details: Details<E>, callback: (response: Reply) => void) => void
export type ObserverHandler<E extends ObserverEvent> = (details: Details<E>) => void
type Handler<E extends NetworkEvent> = E extends BlockingEvent ? BlockingHandler<E> : ObserverHandler<E & ObserverEvent>

const HEADER_KEY: Record<BlockingEvent, 'requestHeaders' | 'responseHeaders' | null> = {
  onBeforeRequest: null,
  onBeforeSendHeaders: 'requestHeaders',
  onHeadersReceived: 'responseHeaders'
}
const BLOCKING = new Set<string>(Object.keys(HEADER_KEY))
const EVENTS: NetworkEvent[] = [
  'onBeforeRequest',
  'onBeforeSendHeaders',
  'onHeadersReceived',
  'onCompleted',
  'onErrorOccurred',
  'onBeforeRedirect',
  'onResponseStarted'
]

// ---------------------------------------------------------------- match patterns

const escapeRe = (s: string) => s.replace(/[.+?^${}()|[\]\\]/g, '\\$&')

/** Compiles a Chrome match pattern (`<all_urls>`, `*://*.example.com/path*`); null if it is invalid. */
export function compilePattern(pattern: string): ((url: string) => boolean) | null {
  if (pattern === '<all_urls>') return (url) => /^(https?|wss?|ftp|file):/i.test(url)
  const m = /^(\*|[a-z][a-z0-9+.-]*):\/\/([^/]*)(\/.*)$/i.exec(pattern)
  if (!m) return null
  const [, scheme, authority, path] = m
  const portAt = /:(\d+|\*)$/.exec(authority)
  const host = (portAt ? authority.slice(0, portAt.index) : authority).toLowerCase()
  const port = portAt?.[1]
  const pathRe = new RegExp(`^${path.split('*').map(escapeRe).join('.*')}$`)
  const schemeRe = scheme === '*' ? /^https?$/ : new RegExp(`^${escapeRe(scheme.toLowerCase())}$`)
  return (raw) => {
    let u: URL
    try {
      u = new URL(raw)
    } catch {
      return false
    }
    if (!schemeRe.test(u.protocol.slice(0, -1))) return false
    const h = u.hostname.toLowerCase()
    if (host === '*') {
      // any host
    } else if (host.startsWith('*.')) {
      const base = host.slice(2)
      if (h !== base && !h.endsWith(`.${base}`)) return false
    } else if (h !== host) return false
    if (port && port !== '*' && u.port !== port) return false
    return pathRe.test(u.pathname + u.search)
  }
}

interface Matcher {
  matches(url: string, type?: string): boolean
}

function compileFilter(filter: NetworkFilter | undefined): Matcher {
  const urls = filter?.urls?.length ? filter.urls : ['<all_urls>']
  const tests = urls.map(compilePattern).filter((t): t is (url: string) => boolean => t !== null)
  const types = filter?.types?.length ? new Set(filter.types) : null
  return { matches: (url, type) => (!types || (type !== undefined && types.has(type))) && tests.some((t) => t(url)) }
}

// ---------------------------------------------------------------- hub

interface Entry {
  name: string
  order: number
  match: Matcher
  handler: (...args: never[]) => void
}

export class NetworkHub {
  private entries = new Map<NetworkEvent, Entry[]>()
  private warned = new Set<string>()

  constructor(private ses: Session) {}

  /** Registers (or replaces) the consumer `name` on an event. Returns a function that removes it. */
  on<E extends NetworkEvent>(event: E, name: string, order: number, filter: NetworkFilter | undefined, handler: Handler<E>): () => void {
    const list = (this.entries.get(event) ?? []).filter((e) => e.name !== name)
    list.push({ name, order, match: compileFilter(filter), handler: handler as Entry['handler'] })
    list.sort((a, b) => a.order - b.order)
    const first = !this.entries.get(event)?.length
    this.entries.set(event, list)
    if (first) this.install(event)
    return () => this.off(event, name)
  }

  off(event: NetworkEvent, name: string): void {
    const list = this.entries.get(event)
    if (!list?.some((e) => e.name === name)) return
    const rest = list.filter((e) => e.name !== name)
    if (rest.length) {
      this.entries.set(event, rest)
      return
    }
    this.entries.delete(event)
    this.uninstall(event)
  }

  /** Whether the real Electron listener of this event is currently registered. */
  has(event: NetworkEvent): boolean {
    return (this.entries.get(event)?.length ?? 0) > 0
  }

  /** Removes everything a consumer registered. */
  release(name: string): void {
    for (const event of EVENTS) this.off(event, name)
  }

  /**
   * A `session.webRequest` look-alike for one consumer, for code written against Electron's API (Ghostery):
   * `onX(filter, listener)`, `onX(listener)`, `onX(null)`.
   */
  consumer(name: string, order: number): Electron.WebRequest {
    const api: Record<string, unknown> = {}
    for (const event of EVENTS) {
      api[event] = (a?: NetworkFilter | Handler<NetworkEvent> | null, b?: Handler<NetworkEvent>) => {
        const handler = typeof a === 'function' ? a : b
        if (a === null || a === undefined || !handler) return this.off(event, name)
        this.on(event, name, order, typeof a === 'function' ? undefined : a, handler as never)
      }
    }
    return api as unknown as Electron.WebRequest
  }

  private install(event: NetworkEvent): void {
    const wr = this.ses.webRequest as unknown as Record<string, (filter: NetworkFilter, listener: unknown) => void>
    const filter = { urls: ['<all_urls>'] }
    if (BLOCKING.has(event)) wr[event](filter, (d: never, cb: (r: Reply) => void) => this.chain(event as BlockingEvent, d, cb))
    else wr[event](filter, (d: never) => this.fan(event, d))
  }

  private uninstall(event: NetworkEvent): void {
    try {
      ;(this.ses.webRequest as unknown as Record<string, (l: null) => void>)[event](null)
    } catch {
      // session already released
    }
  }

  private warn(entry: Entry, event: string, error: unknown): void {
    const key = `${event}:${entry.name}`
    if (this.warned.has(key)) return
    this.warned.add(key)
    console.error(`Network hub: "${entry.name}" failed on ${event}:`, error)
  }

  private matching(event: NetworkEvent, details: { url: string; resourceType?: string }): Entry[] {
    return (this.entries.get(event) ?? []).filter((e) => e.match.matches(details.url, details.resourceType))
  }

  /** Observers: every matching consumer, each isolated from the others. */
  private fan(event: NetworkEvent, details: { url: string; resourceType?: string }): void {
    for (const entry of this.matching(event, details)) {
      try {
        ;(entry.handler as (d: unknown) => void)(details)
      } catch (error) {
        this.warn(entry, event, error)
      }
    }
  }

  /** Blocking events: handlers in order; cancel or redirect ends the chain, headers are handed on. */
  private chain(event: BlockingEvent, details: { url: string; resourceType?: string }, done: (r: Reply) => void): void {
    const list = this.matching(event, details)
    const key = HEADER_KEY[event]
    const acc: Record<string, unknown> = {}
    let current = details as Record<string, unknown>
    let i = 0
    const next = (): void => {
      const entry = list[i++]
      if (!entry) return done(acc as Reply)
      let settled = false
      const finish = (res: Reply | undefined): void => {
        if (settled) return
        settled = true
        clearTimeout(timer)
        if (res?.cancel || res?.redirectURL) return done(res)
        if (res) {
          Object.assign(acc, res)
          if (key && res[key]) current = { ...current, [key]: res[key] }
        }
        next()
      }
      const timer = setTimeout(() => {
        this.warn(entry, event, new Error('no answer before the timeout'))
        finish(undefined)
      }, HANDLER_TIMEOUT_MS)
      try {
        ;(entry.handler as (d: unknown, cb: (r: Reply) => void) => void)(current, finish)
      } catch (error) {
        this.warn(entry, event, error)
        finish(undefined)
      }
    }
    next()
  }
}

const hubs = new WeakMap<Session, NetworkHub>()

/** The hub of a session (created on first use, released with the session). */
export function networkHub(ses: Session): NetworkHub {
  let hub = hubs.get(ses)
  if (!hub) {
    hub = new NetworkHub(ses)
    hubs.set(ses, hub)
  }
  return hub
}

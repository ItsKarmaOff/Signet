import { app, ipcMain, webContents, type ServiceWorkerMain, type Session } from 'electron'
import { CRX_EVENT, CRX_INVOKE, INSTALLED_METHOD, type CrxResult } from '@shared/extension-apis'
import { ApiError, NO_CONTEXT, type ApiContext, type ApiHandler, type ExtensionEmitter } from './types'

const SCOPE = /^chrome-extension:\/\/([a-p]{32})\/$/
const PAGE = /^chrome-extension:\/\/([a-p]{32})\//

/** Extension id of a service worker scope, or null for anything else. */
export const extensionOfScope = (scope: string): string | null => SCOPE.exec(scope)?.[1] ?? null

/** Extension id of a page URL, or null if it is not an extension page. */
export const extensionOfPage = (url: string | undefined): string | null => (url ? (PAGE.exec(url)?.[1] ?? null) : null)

/** Wait before retrying to start a worker that failed (then the event stays queued until it runs by itself). */
export const WAKE_BACKOFF_MS = [1000, 2000, 5000]
/** A freshly running worker needs a moment to evaluate its script and register listeners. */
const SETTLE_MS = 300
const MAX_QUEUED = 50

interface Queued {
  events: { name: string; args: unknown[] }[]
  attempt: number
  timer?: ReturnType<typeof setTimeout>
}

interface WorkerEntry {
  id: string
  worker: ServiceWorkerMain
  gone: (() => void)[]
}

interface Entry {
  /** Any of these manifest permissions unlocks the method; empty = no permission needed. */
  permissions: string[]
  fn: ApiHandler
}

/**
 * Entry point for the extension APIs Signet adds. Every call arrives with a sender (a service worker or an extension
 * page); the extension id is read from it, never from the arguments, and checked against what the session has loaded and
 * the manifest permissions before a handler runs.
 */
export class ApiRouter implements ExtensionEmitter {
  private handlers = new Map<string, Entry>()
  private sessions = new Set<Session>()
  /** Running extension workers per session, by version id. */
  private workers = new Map<Session, Map<number, WorkerEntry>>()
  /** Events that could not reach a worker that failed to start, per session then extension. */
  private queued = new Map<Session, Map<string, Queued>>()
  private handled = new WeakSet<ServiceWorkerMain>()

  /** Declares `method` (e.g. `alarms.create`), usable by extensions with `permission`. */
  register(permission: string | string[], method: string, fn: ApiHandler): void {
    if (this.handlers.has(method)) throw new Error(`Extension API ${method} already registered`)
    this.handlers.set(method, { permissions: ([] as string[]).concat(permission).filter(Boolean), fn })
  }

  // ───────────────────────── Calls ─────────────────────────

  async dispatch(ses: Session, extensionId: string, method: unknown, args: unknown, context: ApiContext = NO_CONTEXT): Promise<CrxResult> {
    try {
      if (!this.sessions.has(ses)) throw new ApiError('Extension APIs are not available in this session.')
      const entry = typeof method === 'string' ? this.handlers.get(method) : undefined
      if (!entry) throw new ApiError(`Unknown Extension API - ${String(method)}`)
      const ext = ses.extensions.getExtension(extensionId)
      if (!ext) throw new ApiError('Extension is not loaded.')
      if (entry.permissions.length && !entry.permissions.some((p) => hasPermission(ext.manifest, p))) {
        throw new ApiError(`Access to extension API denied: the '${entry.permissions.join("' or '")}' permission is required.`)
      }
      if (!Array.isArray(args)) throw new ApiError('Invalid arguments.')
      const value = await entry.fn({ ses, extensionId, context }, ...args)
      this.trace(extensionId, method, 'ok')
      return { ok: true, value }
    } catch (e) {
      const error = e instanceof ApiError ? e.message : 'Internal error.' + (e instanceof Error ? ` ${e.message}` : '')
      this.trace(extensionId, method, `error: ${error}`)
      return { ok: false, error }
    }
  }

  /** One terminal line per call, development only (the user cannot open the worker console). */
  private trace(extensionId: string, method: unknown, outcome: string): void {
    if (!app.isPackaged && method !== INSTALLED_METHOD) console.log(`[extension-apis] ${extensionId} ${String(method)} ${outcome}`.slice(0, 300))
  }

  /** Extension pages: only the main frame of a chrome-extension:// page, whose URL names the extension. */
  registerPageIpc(): void {
    ipcMain.handle(CRX_INVOKE, (e, method: unknown, args: unknown): Promise<CrxResult> => {
      const frame = e.senderFrame
      const id = frame && frame === e.sender.mainFrame ? extensionOfPage(frame.url) : null
      if (!id) return Promise.resolve({ ok: false, error: 'Access to extension API denied.' })
      const wc = e.sender
      const context: ApiContext = { kind: 'page', id: `page-${wc.id}`, url: frame!.url, onGone: (fn) => wc.once('destroyed', fn) }
      return this.dispatch(wc.session, id, method, args, context)
    })
  }

  // ───────────────────────── Sessions and workers ─────────────────────────

  /** Starts serving a session: handles calls from its extension workers. Call once per regular session. */
  attach(ses: Session): void {
    if (this.sessions.has(ses)) return
    this.sessions.add(ses)
    this.workers.set(ses, new Map())
    ses.serviceWorkers.on('running-status-changed', ({ versionId, runningStatus }) => {
      // 'starting' too: the handler must exist before the worker's first call, which can come as soon as its script runs.
      if (runningStatus === 'running' || runningStatus === 'starting') this.track(ses, versionId)
      else this.forget(ses, versionId)
    })
    // Why a worker fails to start: its console errors, and our own preload lines, in the terminal.
    ses.serviceWorkers.on('console-message', (_event, details) => {
      const extensionId = extensionOfScope(ses.serviceWorkers.getWorkerFromVersionID(details.versionId)?.scope ?? '') ?? extensionOfPage(details.sourceUrl)
      if (!extensionId) return
      const message = String(details.message)
      if (details.level >= 3 || message.includes('[signet]')) {
        console.log(`[extension-apis] ${extensionId} worker console (${details.level >= 3 ? 'error' : 'log'}): ${message.replace(/\s+/g, ' ')}`.slice(0, 400))
      }
    })
    // Workers that were already up before we listened.
    try {
      for (const id of Object.keys(ses.serviceWorkers.getAllRunning())) this.track(ses, Number(id))
    } catch {
      // older API surface: the status event will catch them
    }
  }

  private track(ses: Session, versionId: number): void {
    const worker = ses.serviceWorkers.getWorkerFromVersionID(versionId)
    const id = worker && extensionOfScope(worker.scope)
    if (!worker || !id) return
    const map = this.workers.get(ses)
    if (!map) return
    const entry = map.get(versionId) ?? { id, worker, gone: [] }
    map.set(versionId, entry)
    if (!this.handled.has(worker)) {
      this.handled.add(worker)
      const context: ApiContext = { kind: 'worker', id: `worker-${versionId}`, url: worker.scope, onGone: (fn) => entry.gone.push(fn) }
      // The id comes from the worker's own scope, fixed by Chromium, not from the message.
      worker.ipc.handle(CRX_INVOKE, (_event, method: unknown, args: unknown) => this.dispatch(ses, id, method, args, context))
    }
    // A worker that is up can take the events it missed (after it has had time to register its listeners).
    if (this.queued.get(ses)?.has(id)) setTimeout(() => this.flush(ses, id, worker), SETTLE_MS)
  }

  private forget(ses: Session, versionId: number): void {
    const entry = this.workers.get(ses)?.get(versionId)
    if (!entry) return
    this.workers.get(ses)?.delete(versionId)
    for (const fn of entry.gone) fn()
  }

  // ───────────────────────── Events ─────────────────────────

  emit(extensionId: string, name: string, args: unknown[], wake: boolean, queue = true): void {
    for (const ses of this.sessions) {
      if (!ses.extensions.getExtension(extensionId)) continue
      const running = [...(this.workers.get(ses)?.values() ?? [])].filter((w) => w.id === extensionId)
      for (const { worker } of running) worker.send(CRX_EVENT, name, args)
      if (!running.length && wake) this.wakeAndSend(ses, extensionId, name, args, queue)
      for (const wc of webContents.getAllWebContents()) {
        if (!wc.isDestroyed() && wc.session === ses && extensionOfPage(wc.getURL()) === extensionId) wc.send(CRX_EVENT, name, args)
      }
    }
  }

  private wakeAndSend(ses: Session, id: string, name: string, args: unknown[], queue: boolean): void {
    ses.serviceWorkers
      .startWorkerForScope(`chrome-extension://${id}/`)
      .then((worker) => worker.send(CRX_EVENT, name, args))
      .catch((e) => {
        if (queue) this.enqueue(ses, id, name, args)
        else console.warn(`Extension ${id}: cannot wake the worker for ${name}`, e)
      })
  }

  /** The worker would not start (typical at launch, right after the extension loads): keep the event and try again. */
  private enqueue(ses: Session, id: string, name: string, args: unknown[]): void {
    const perSession = this.queued.get(ses) ?? this.queued.set(ses, new Map()).get(ses)!
    let q = perSession.get(id)
    if (!q) {
      perSession.set(id, (q = { events: [], attempt: 0 }))
      console.log(`[extension-apis] ${id} worker not up yet, queueing ${name} (will retry)`)
    }
    if (q.events.length < MAX_QUEUED) q.events.push({ name, args })
    this.scheduleRetry(ses, id, q)
  }

  private scheduleRetry(ses: Session, id: string, q: Queued): void {
    if (q.timer || q.attempt >= WAKE_BACKOFF_MS.length) return
    q.timer = setTimeout(() => {
      q.timer = undefined
      q.attempt++
      if (!ses.extensions.getExtension(id)) return void this.queued.get(ses)?.delete(id)
      ses.serviceWorkers
        .startWorkerForScope(`chrome-extension://${id}/`)
        .then((worker) => setTimeout(() => this.flush(ses, id, worker), SETTLE_MS))
        .catch(() => this.scheduleRetry(ses, id, q))
    }, WAKE_BACKOFF_MS[q.attempt])
  }

  private flush(ses: Session, id: string, worker: ServiceWorkerMain): void {
    const q = this.queued.get(ses)?.get(id)
    if (!q) return
    this.queued.get(ses)!.delete(id)
    if (q.timer) clearTimeout(q.timer)
    for (const { name, args } of q.events) worker.send(CRX_EVENT, name, args)
    console.log(`[extension-apis] ${id} worker is up, delivered ${q.events.length} queued event(s)`)
  }

  /** Running service worker contexts of an extension (for runtime.getContexts). */
  runningWorkers(ses: Session, extensionId: string): number {
    return [...(this.workers.get(ses)?.values() ?? [])].filter((w) => w.id === extensionId).length
  }

  extensionsWith(permission: string): string[] {
    const ids = new Set<string>()
    for (const ses of this.sessions) for (const ext of ses.extensions.getAllExtensions()) if (hasPermission(ext.manifest, permission)) ids.add(ext.id)
    return [...ids]
  }

  /** True if some attached session other than `except` still has the extension (it is not really gone yet). */
  loadedElsewhere(extensionId: string, except: Session): boolean {
    for (const ses of this.sessions) if (ses !== except && ses.extensions.getExtension(extensionId)) return true
    return false
  }
}

export function hasPermission(manifest: unknown, permission: string): boolean {
  const list = (manifest as { permissions?: unknown } | undefined)?.permissions
  return Array.isArray(list) && list.includes(permission)
}

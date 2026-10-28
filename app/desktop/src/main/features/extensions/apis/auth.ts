import { AUTH_EVENT } from '@shared/extension-apis'
import type { ApiContext, ExtensionEmitter } from './types'

/** Chrome's `BlockingResponse` for onAuthRequired, as far as we read it. */
interface AuthResponse {
  cancel?: boolean
  authCredentials?: { username?: unknown; password?: unknown }
}

export type AuthOutcome = { action: 'skip' } | { action: 'cancel'; reason: string } | { action: 'credentials'; username: string; password: string }

export interface ProxyChallenge {
  url: string
  host: string
  port: number
  scheme: string
  realm: string
}

interface Listening {
  kind: ApiContext['kind']
  url: string
  count: number
}

interface Pending {
  extensionId: string
  /** Contexts whose answer we still wait for; `worker:*` stands for a worker that is being woken. */
  expected: Set<string>
  sawCancel: boolean
  resolve: (o: AuthOutcome) => void
  timer: ReturnType<typeof setTimeout>
}

const WAKING_WORKER = 'worker:*'

/**
 * Answers proxy 407 challenges with the extension that controls `proxy.settings`, through its onAuthRequired listeners.
 * Electron may not deliver the native event for proxy auth, so main turns `app.on('login')` into the event itself.
 *
 * An extension can have several contexts with a listener (its worker, a popup, an offscreen page) and only some know
 * the credentials. Like Chrome merging blocking responses, credentials from ANY context win; an empty or cancelling
 * answer is only final once every context that listens has answered. Credentials that keep being refused are not
 * retried forever.
 */
export class ProxyAuthBroker {
  private pending = new Map<string, Pending>()
  private recent = new Map<string, number[]>()
  /** Extension -> context id -> its listeners. Workers restart, so contexts are dropped when they go away. */
  private listening = new Map<string, Map<string, Listening>>()
  private lastCredentials = new Map<string, number>()
  private counter = 0

  constructor(
    private deps: {
      emitter: Pick<ExtensionEmitter, 'emit'>
      /** Extension controlling the proxy that may answer (has the webRequest permission), if any. */
      responder(): string | undefined
      log(message: string): void
      now?: () => number
    },
    private timeoutMs = 30_000,
    private maxAttempts = 5,
    private windowMs = 60_000
  ) {}

  /** A context reports how many onAuthRequired listeners it has now. */
  setListeners(extensionId: string, context: ApiContext, count: number): void {
    const map = this.listening.get(extensionId) ?? this.listening.set(extensionId, new Map()).get(extensionId)!
    if (!(count > 0)) return void this.drop(extensionId, context.id)
    if (!map.has(context.id)) context.onGone(() => this.drop(extensionId, context.id))
    map.set(context.id, { kind: context.kind, url: context.url, count })
  }

  private drop(extensionId: string, contextId: string): void {
    this.listening.get(extensionId)?.delete(contextId)
    // A context that went away will never answer.
    for (const p of this.pending.values()) {
      if (p.extensionId === extensionId && p.expected.delete(contextId)) this.settle(p)
    }
  }

  /** Synchronous: should the `login` event be taken over? */
  canHandle(isProxy: boolean): boolean {
    // Even before the worker registered a listener (after a restart): it is woken and registers when it starts.
    return isProxy && !!this.deps.responder()
  }

  async handle(c: ProxyChallenge): Promise<AuthOutcome> {
    const extensionId = this.deps.responder()
    if (!extensionId) return { action: 'skip' }
    const now = (this.deps.now ?? Date.now)()
    const attemptKey = `${extensionId}|${c.host}:${c.port}|${c.realm}`
    const recent = (this.recent.get(attemptKey) ?? []).filter((t) => now - t < this.windowMs)
    const lastOk = this.lastCredentials.get(attemptKey)
    if (recent.length >= this.maxAttempts) {
      this.deps.log(`[extension-apis] ${extensionId} proxy auth refused ${recent.length} times in a row for ${c.host}:${c.port}, cancelling`)
      return { action: 'cancel', reason: 'too many attempts' }
    }
    recent.push(now)
    this.recent.set(attemptKey, recent)

    const key = `signet-auth-${++this.counter}`
    const expected = new Set([...(this.listening.get(extensionId)?.keys() ?? [])])
    // No worker among the known listeners: it was stopped, wake it (it registers its listener again when it starts).
    if (![...expected].some((id) => id.startsWith('worker-'))) expected.add(WAKING_WORKER)
    this.deps.log(
      `[extension-apis] ${extensionId} proxy auth requested by ${c.host}:${c.port} (${c.scheme}, realm "${c.realm}", attempt ${recent.length}, url ${c.url.slice(0, 120)})` +
        (lastOk && now - lastOk < 10_000 ? ' - asked again right after credentials were supplied: the proxy may be refusing them' : '')
    )
    const details = {
      requestId: key,
      url: c.url,
      method: 'GET',
      frameId: 0,
      parentFrameId: -1,
      tabId: -1,
      type: 'other',
      timeStamp: now,
      scheme: c.scheme,
      realm: c.realm,
      challenger: { host: c.host, port: c.port },
      isProxy: true,
      responseHeaders: [],
      statusCode: 407,
      statusLine: 'HTTP/1.1 407 Proxy Authentication Required'
    }
    const outcome = await new Promise<AuthOutcome>((resolve) => {
      const timer = setTimeout(() => {
        const p = this.pending.get(key)
        this.pending.delete(key)
        this.deps.log(`[extension-apis] ${extensionId} proxy auth: no answer from ${[...(p?.expected ?? [])].join(', ')}`)
        resolve({ action: 'cancel', reason: 'no answer' })
      }, this.timeoutMs)
      this.pending.set(key, { extensionId, expected, sawCancel: false, resolve, timer })
      // Waking the worker is the point; the event is time-sensitive, so it is not queued for later.
      this.deps.emitter.emit(extensionId, AUTH_EVENT, [key, details], true, false)
    })
    if (outcome.action === 'credentials') this.lastCredentials.set(attemptKey, now)
    return outcome
  }

  /** A context's answer (`webRequest.authResponse`); only the extension the request was sent to counts. */
  respond(extensionId: string, context: ApiContext, key: unknown, response: unknown): void {
    const p = typeof key === 'string' ? this.pending.get(key) : undefined
    if (!p || p.extensionId !== extensionId) return
    const r = (response && typeof response === 'object' ? response : {}) as AuthResponse
    const { username, password } = r.authCredentials ?? {}
    const who = `${context.kind} ${context.url}`
    if (!r.cancel && typeof username === 'string' && typeof password === 'string') {
      this.deps.log(`[extension-apis] ${extensionId} proxy auth answered by ${who}: credentials supplied`)
      this.finish(key as string, p, { action: 'credentials', username, password })
      return
    }
    this.deps.log(`[extension-apis] ${extensionId} proxy auth answered by ${who}: ${r.cancel ? 'cancelled' : 'empty'}`)
    if (r.cancel) p.sawCancel = true
    p.expected.delete(context.id)
    // A worker answering satisfies the placeholder for the worker we were waking.
    if (context.kind === 'worker') p.expected.delete(WAKING_WORKER)
    this.settle(p)
  }

  /** Nobody left to wait for and nobody supplied credentials. */
  private settle(p: Pending): void {
    if (p.expected.size) return
    const key = [...this.pending].find(([, v]) => v === p)?.[0]
    if (!key) return
    this.finish(key, p, { action: 'cancel', reason: p.sawCancel ? 'extension cancelled' : 'no listener supplied credentials' })
  }

  private finish(key: string, p: Pending, outcome: AuthOutcome): void {
    clearTimeout(p.timer)
    this.pending.delete(key)
    if (outcome.action === 'cancel') this.deps.log(`[extension-apis] ${p.extensionId} proxy auth cancelled: ${outcome.reason}`)
    p.resolve(outcome)
  }
}

import { readFile } from 'node:fs/promises'
import { resolve as resolvePath, sep } from 'node:path'
import type { Session } from 'electron'
import { DNR_CONSTANTS } from '@shared/extension-apis'
import type { ExtensionActionCounts } from '@shared/types'
import { NetworkOrder, compilePattern, networkHub, type NetworkHub } from '../../../web/network'
import { validateRule, type DnrStore } from './dnr'
import {
  RuleIndex,
  applyHeaderOps,
  compileRule,
  dnrType,
  makeRequest,
  redirectTarget,
  resolve,
  upgradedUrl,
  type CompiledRule,
  type DnrRequest,
  type Resolution
} from './dnr-match'
import { ApiError, type ApiCaller, type ApiHandler } from './types'

/**
 * Enforcement of `chrome.declarativeNetRequest`. Rules come from three places: the static rulesets of the manifest (loaded
 * from disk without blocking, only the enabled ones), the extension's dynamic rules and its session rules. They are
 * compiled into one `RuleIndex`, rebuilt (debounced) when anything changes, and consulted by hub consumers named `dnr`.
 *
 * Approximations to know about:
 * - Chrome gives precedence to the extension installed most recently; Signet only knows the order in which extensions
 *   were loaded since launch, and uses that.
 * - A winning `allow` of one extension ends the decision for lower-precedence extensions, as in Chrome.
 * - `allowAllRequests` on a sub frame is remembered under the frame Electron reports for that request, which may be the
 *   parent frame; the main frame case (what blockers use) is exact.
 */

/** Chrome's global pool of static rules on top of the guaranteed minimum. */
const GLOBAL_STATIC_RULES = 300_000
const LOG_WINDOW_MS = 5 * 60_000
const LOG_MAX_PER_TAB = 1000
const REBUILD_DELAY_MS = 50
/** Longest delay between two pushes of the action counts to the toolbar. */
const COUNTS_PUSH_MS = 250

interface StaticRuleset {
  id: string
  path: string
  enabledByDefault: boolean
  loading?: Promise<void>
  rules: CompiledRule[] | null
}

interface ExtState {
  id: string
  dir: string
  version: string
  seq: number
  /** Has `declarativeNetRequest` or `declarativeNetRequestWithHostAccess`: without one, its rules do nothing. */
  active: boolean
  /** Only the "with host access" permission: every action needs host access. */
  hostOnly: boolean
  feedback: boolean
  hosts: ((url: string) => boolean)[]
  sessions: Set<Session>
  rulesets: Map<string, StaticRuleset>
}

interface FrameLike {
  frameTreeNodeId?: number
  origin?: string
  parent?: FrameLike | null
}

/** The fields of Electron's request details that the engine reads. */
export interface RequestDetails {
  url: string
  method: string
  resourceType?: string
  webContentsId?: number
  frame?: FrameLike | null
  referrer?: string
  requestHeaders?: Record<string, string>
  responseHeaders?: Record<string, string[]>
}

export interface EngineDeps {
  log: (message: string) => void
  now?: () => number
  /** Runs `fn` when the tab (webContents id) is destroyed. */
  watchTab?: (id: number, fn: () => void) => void
  /** Receives the action counts (see `actionCounts`) after they changed, at most every 250 ms. */
  onCounts?: (counts: ExtensionActionCounts) => void
}

interface MatchLog {
  ext: string
  set: string
  id: number
  time: number
}

const hasPermission = (manifest: unknown, permission: string): boolean => {
  const list = (manifest as { permissions?: unknown }).permissions
  return Array.isArray(list) && list.includes(permission)
}
const isObject = (v: unknown): v is Record<string, unknown> => !!v && typeof v === 'object' && !Array.isArray(v)
const ids = (v: unknown): string[] => (Array.isArray(v) ? v.map(String) : [])
const yieldToLoop = () => new Promise<void>((r) => setImmediate(r))

type DnrEvent = 'onBeforeRequest' | 'onBeforeSendHeaders' | 'onHeadersReceived'
const EVENTS: DnrEvent[] = ['onBeforeRequest', 'onBeforeSendHeaders', 'onHeadersReceived']

export class DnrEngine {
  private exts = new Map<string, ExtState>()
  private seq = 0
  private index = new RuleIndex([])
  private wanted = new Set<DnrEvent>()
  private timer: ReturnType<typeof setTimeout> | null = null
  private sessions = new Map<Session, Set<DnrEvent>>()
  /** `allowAllRequests` grants: tab, then frame key (`main` or `f<id>`), then extension, then priority. */
  private frames = new Map<number, Map<string, Map<string, number>>>()
  private matchedLog = new Map<number, MatchLog[]>()
  private lastSweep = 0
  /** Action counts: tab, then extension. Kept for the extensions that turned `displayActionCountAsBadgeText` on. */
  private counts = new Map<number, Map<string, number>>()
  private countedTabs = new Set<number>()
  private countsTimer: ReturnType<typeof setTimeout> | null = null
  private now: () => number

  constructor(
    private store: DnrStore,
    private deps: EngineDeps
  ) {
    this.now = deps.now ?? Date.now
    store.onChange(() => this.schedule())
  }

  // ───────────────────────── Extensions and sessions ─────────────────────────

  /** Starts enforcing in a regular session: follows the extensions loaded in it. */
  attach(ses: Session): void {
    if (this.sessions.has(ses)) return
    this.sessions.set(ses, new Set())
    for (const ext of ses.extensions.getAllExtensions()) this.addExtension(ses, ext)
    ses.extensions.on('extension-loaded', (_e, ext) => this.addExtension(ses, ext))
    ses.extensions.on('extension-unloaded', (_e, ext) => this.removeExtension(ses, ext.id))
    this.schedule()
  }

  addExtension(ses: Session, ext: { id: string; path: string; version?: string; manifest: unknown }): void {
    const manifest = (isObject(ext.manifest) ? ext.manifest : {}) as {
      declarative_net_request?: { rule_resources?: { id?: unknown; enabled?: unknown; path?: unknown }[] }
      permissions?: unknown
      host_permissions?: unknown
      optional_host_permissions?: unknown
    }
    const version = String(ext.version ?? '')
    let x = this.exts.get(ext.id)
    if (!x || x.version !== version || x.dir !== ext.path) {
      x = {
        id: ext.id,
        dir: ext.path,
        version,
        seq: x?.seq ?? ++this.seq,
        active: false,
        hostOnly: false,
        feedback: false,
        hosts: [],
        sessions: x?.sessions ?? new Set(),
        rulesets: new Map()
      }
      this.exts.set(ext.id, x)
      for (const r of manifest.declarative_net_request?.rule_resources ?? []) {
        if (isObject(r) && typeof r.id === 'string' && typeof r.path === 'string')
          x.rulesets.set(r.id, { id: r.id, path: r.path, enabledByDefault: r.enabled === true, rules: null })
      }
    }
    const perms = Array.isArray(manifest.permissions) ? manifest.permissions.map(String) : []
    x.active = hasPermission(manifest, 'declarativeNetRequest') || hasPermission(manifest, 'declarativeNetRequestWithHostAccess')
    x.hostOnly = !hasPermission(manifest, 'declarativeNetRequest')
    x.feedback = hasPermission(manifest, 'declarativeNetRequestFeedback')
    // Optional host permissions count as granted: Signet has no prompt to grant them later (uBlock Origin Lite asks for
    // all sites that way, and its redirect and parameter rules need it).
    const list = (v: unknown) => (Array.isArray(v) ? v.map(String) : [])
    const patterns = [
      ...list(manifest.host_permissions),
      ...list(manifest.optional_host_permissions),
      ...perms.filter((p) => p.includes('://') || p === '<all_urls>')
    ]
    x.hosts = patterns.map(compilePattern).filter((t): t is (url: string) => boolean => t !== null)
    x.sessions.add(ses)
    this.schedule()
  }

  removeExtension(ses: Session, id: string): void {
    const x = this.exts.get(id)
    if (!x) return
    x.sessions.delete(ses)
    if (!x.sessions.size) {
      this.exts.delete(id)
      let had = false
      for (const [tab, per] of this.counts) {
        had = per.delete(id) || had
        if (!per.size) this.counts.delete(tab)
      }
      if (had) this.pushCounts()
    }
    this.schedule()
  }

  // ───────────────────────── Index ─────────────────────────

  private schedule(): void {
    if (this.timer) return
    this.timer = setTimeout(() => {
      this.timer = null
      this.rebuild()
    }, REBUILD_DELAY_MS)
  }

  /** Recompiles every active rule into a fresh index and aligns the hub consumers of each session. */
  rebuild(): void {
    if (this.timer) clearTimeout(this.timer)
    this.timer = null
    const rules: CompiledRule[] = []
    const wanted = new Set<DnrEvent>()
    for (const x of this.exts.values()) {
      if (!x.active) continue
      for (const id of this.enabledIds(x)) {
        const rs = x.rulesets.get(id)
        if (!rs) continue
        if (rs.rules) rules.push(...rs.rules)
        else void this.load(x, rs)
      }
      for (const kind of ['dynamic', 'session'] as const) {
        for (const rule of this.store.get(kind, x.id, undefined)) {
          const c = compileRule(x.id, kind === 'dynamic' ? DNR_CONSTANTS.DYNAMIC_RULESET_ID : DNR_CONSTANTS.SESSION_RULESET_ID, rule)
          if (c) rules.push(c)
        }
      }
    }
    for (const r of rules) {
      wanted.add('onBeforeRequest')
      if (r.type === 'modifyHeaders') {
        const a = r.raw.action
        if (Array.isArray(a.requestHeaders) && a.requestHeaders.length) wanted.add('onBeforeSendHeaders')
        if (Array.isArray(a.responseHeaders) && a.responseHeaders.length) wanted.add('onHeadersReceived')
      }
    }
    this.index = new RuleIndex(rules)
    this.wanted = wanted
    for (const ses of this.sessions.keys()) this.syncSession(ses)
  }

  /**
   * Registers the hub consumers that are needed now and removes the others. Taking `onBeforeSendHeaders` (or
   * `onHeadersReceived`) from Electron silences the extensions' own `chrome.webRequest` listeners on that event, so it is
   * only taken while an active rule changes those headers.
   */
  private syncSession(ses: Session): void {
    const on = this.sessions.get(ses)!
    const hub = networkHub(ses)
    for (const event of EVENTS) {
      const want = this.wanted.has(event)
      if (want && !on.has(event)) {
        this.register(hub, ses, event)
        on.add(event)
      } else if (!want && on.has(event)) {
        hub.off(event, 'dnr')
        on.delete(event)
      }
    }
  }

  private register(hub: NetworkHub, ses: Session, event: DnrEvent): void {
    if (event === 'onBeforeRequest') hub.on(event, 'dnr', NetworkOrder.dnr, undefined, (d, cb) => this.beforeRequest(ses, d, cb))
    else if (event === 'onBeforeSendHeaders') hub.on(event, 'dnr', NetworkOrder.dnr, undefined, (d, cb) => this.sendHeaders(ses, d, cb))
    else hub.on(event, 'dnr', NetworkOrder.dnr, undefined, (d, cb) => this.receivedHeaders(ses, d, cb))
  }

  /** Events currently taken from Electron in a session (tests, diagnostics). */
  registered(ses: Session): DnrEvent[] {
    return [...(this.sessions.get(ses) ?? [])]
  }

  // ───────────────────────── Static rulesets ─────────────────────────

  private enabledIds(x: ExtState): string[] {
    const chosen = this.store.enabledRulesets(x.id, x.version)
    const list = chosen ?? [...x.rulesets.values()].filter((r) => r.enabledByDefault).map((r) => r.id)
    return list.filter((id) => x.rulesets.has(id))
  }

  /** Reads and compiles a ruleset file; invalid rules are skipped with one log line for the whole ruleset. */
  private load(x: ExtState, rs: StaticRuleset): Promise<void> {
    rs.loading ??= (async () => {
      try {
        const root = resolvePath(x.dir)
        // Manifest paths are relative to the extension even with a leading slash (`/rulesets/main/x.json`).
        const file = resolvePath(root, rs.path.replace(/^[/\\]+/, ''))
        if (!file.startsWith(root + sep)) throw new Error('path outside the extension')
        const data: unknown = JSON.parse(await readFile(file, 'utf8'))
        if (!Array.isArray(data)) throw new Error('not a list of rules')
        const compiled: CompiledRule[] = []
        let skipped = 0
        for (let i = 0; i < data.length; i++) {
          try {
            const c = compileRule(x.id, rs.id, validateRule(data[i]))
            if (c) compiled.push(c)
            else skipped++
          } catch {
            skipped++
          }
          if (i % 5000 === 4999) await yieldToLoop()
        }
        rs.rules = compiled
        if (skipped) this.deps.log(`[extension-apis] ${x.id} declarativeNetRequest: ruleset "${rs.id}" skipped ${skipped} invalid rule(s)`)
      } catch (e) {
        rs.rules = []
        this.deps.log(`[extension-apis] ${x.id} declarativeNetRequest: ruleset "${rs.id}" cannot be loaded: ${e instanceof Error ? e.message : String(e)}`)
      }
      if (this.exts.get(x.id) === x) this.schedule()
    })()
    return rs.loading
  }

  private state(caller: ApiCaller): ExtState {
    let x = this.exts.get(caller.extensionId)
    if (!x) {
      const ext = caller.ses.extensions.getExtension(caller.extensionId)
      if (!ext) throw new ApiError('Extension is not loaded.')
      this.addExtension(caller.ses, ext)
      x = this.exts.get(caller.extensionId)!
    }
    return x
  }

  private async updateEnabled(caller: ApiCaller, options: unknown): Promise<void> {
    const x = this.state(caller)
    const o = isObject(options) ? options : {}
    const enable = ids(o.enableRulesetIds)
    const disable = ids(o.disableRulesetIds)
    for (const id of [...enable, ...disable]) if (!x.rulesets.has(id)) throw new ApiError(`Invalid ruleset id: ${id}.`)
    const next = new Set(this.enabledIds(x))
    for (const id of disable) next.delete(id)
    for (const id of enable) next.add(id)
    if (next.size > DNR_CONSTANTS.MAX_NUMBER_OF_ENABLED_STATIC_RULESETS) throw new ApiError('The number of enabled static rulesets exceeds the limit.')
    this.store.setEnabledRulesets(x.id, x.version, [...next])
    await Promise.all(enable.map((id) => this.load(x, x.rulesets.get(id)!)))
    this.rebuild()
  }

  private availableStaticRules(x: ExtState): number {
    let used = 0
    for (const id of this.enabledIds(x)) used += x.rulesets.get(id)?.rules?.length ?? 0
    return Math.max(0, DNR_CONSTANTS.GUARANTEED_MINIMUM_STATIC_RULES + GLOBAL_STATIC_RULES - used)
  }

  // ───────────────────────── Requests ─────────────────────────

  private request(d: RequestDetails): { q: DnrRequest; frame: FrameLike | null } | null {
    let frame: FrameLike | null = null
    try {
      frame = d.frame ?? null
    } catch {
      // the frame is gone
    }
    const type = dnrType(d.resourceType)
    let initiator: string | undefined
    if (type !== 'main_frame') {
      try {
        initiator = frame?.origin
      } catch {
        // the frame is gone
      }
      if (!initiator || initiator === 'null') initiator = d.referrer || undefined
      if (!initiator) initiator = d.requestHeaders?.Origin
    }
    const q = makeRequest({ url: d.url, type, method: d.method, initiator, tabId: d.webContentsId ?? -1 })
    return q ? { q, frame } : null
  }

  private canAccess(x: ExtState, q: DnrRequest): boolean {
    if (!x.hosts.some((t) => t(q.url))) return false
    return !q.initiator || !/^(https?|wss?|ftp):/.test(q.initiator) || x.hosts.some((t) => t(`${q.initiator}/`))
  }

  private canAct(ses: Session, r: CompiledRule, q: DnrRequest): boolean {
    const x = this.exts.get(r.ext)
    if (!x || !x.active || !x.sessions.has(ses)) return false
    // Requests of an extension are only affected by its own rules.
    if (q.initiator?.startsWith('chrome-extension://') && q.initiatorHost !== r.ext) return false
    if (r.type === 'redirect' || r.type === 'modifyHeaders' || x.hostOnly) return this.canAccess(x, q)
    return true
  }

  private frameKeys(frame: FrameLike | null): string[] {
    const keys = ['main']
    for (let f = frame, i = 0; f && i < 32; f = f.parent ?? null, i++) if (f.frameTreeNodeId !== undefined) keys.push(`f${f.frameTreeNodeId}`)
    return keys
  }

  private decide(ses: Session, q: DnrRequest, frame: FrameLike | null, startOfRequest: boolean): Resolution | null {
    if (startOfRequest && q.type === 'main_frame') {
      this.frames.delete(q.tabId)
      if (this.counts.delete(q.tabId)) this.pushCounts()
    }
    const matches = this.index.match(q)
    const inherited = this.inheritedAllows(q, frame)
    if (!matches.length && !inherited) return null
    return resolve(matches, q, { rank: (e) => -(this.exts.get(e)?.seq ?? 0), frameAllows: inherited, canAct: (r) => this.canAct(ses, r, q) })
  }

  private inheritedAllows(q: DnrRequest, frame: FrameLike | null): Map<string, number> | undefined {
    const per = this.frames.get(q.tabId)
    if (!per) return undefined
    let out: Map<string, number> | undefined
    for (const key of this.frameKeys(frame)) {
      for (const [ext, priority] of per.get(key) ?? []) {
        out ??= new Map()
        out.set(ext, Math.max(out.get(ext) ?? 0, priority))
      }
    }
    return out
  }

  private remember(q: DnrRequest, frame: FrameLike | null, allows: Resolution['frameAllows']): void {
    if (!allows.length || q.tabId < 0) return
    const key = q.type === 'main_frame' ? 'main' : frame?.frameTreeNodeId !== undefined ? `f${frame.frameTreeNodeId}` : null
    if (!key) return
    let per = this.frames.get(q.tabId)
    if (!per) {
      this.frames.set(q.tabId, (per = new Map()))
      this.deps.watchTab?.(q.tabId, () => this.frames.delete(q.tabId))
    }
    const grants = per.get(key) ?? per.set(key, new Map()).get(key)!
    for (const a of allows) grants.set(a.ext, Math.max(grants.get(a.ext) ?? 0, a.priority))
  }

  private beforeRequest(ses: Session, d: RequestDetails, cb: (r: { cancel?: boolean; redirectURL?: string }) => void): void {
    const parsed = this.request(d)
    const res = parsed && this.decide(ses, parsed.q, parsed.frame, true)
    if (!parsed || !res) return cb({})
    const { q } = parsed
    this.remember(q, parsed.frame, res.frameAllows)
    if (res.rule) this.record(q, [res.rule])
    if (res.action === 'block') {
      this.count(q.tabId, res.rule?.ext, 1)
      return cb({ cancel: true })
    }
    if (res.action === 'redirect' || res.action === 'upgrade') {
      const url = res.action === 'upgrade' ? upgradedUrl(q.url) : redirectTarget(res.rule!, q)
      if (url) {
        this.count(q.tabId, res.rule?.ext, 1)
        return cb({ redirectURL: url })
      }
    }
    cb({})
  }

  // ───────────────────────── Action count ─────────────────────────

  /** Adds `by` (possibly negative, never below 0) to the count of a tab for an extension that displays it. */
  private count(tab: number, ext: string | undefined, by: number): void {
    if (!ext || tab < 0 || !Number.isFinite(by) || !this.store.displayActionCount(ext)) return
    let per = this.counts.get(tab)
    const next = Math.max(0, (per?.get(ext) ?? 0) + by)
    if (next === (per?.get(ext) ?? 0)) return
    if (!per) this.counts.set(tab, (per = new Map()))
    if (next) per.set(ext, next)
    else per.delete(ext)
    if (!per.size) this.counts.delete(tab)
    if (!this.countedTabs.has(tab)) {
      this.countedTabs.add(tab)
      this.deps.watchTab?.(tab, () => {
        this.countedTabs.delete(tab)
        if (this.counts.delete(tab)) this.pushCounts()
      })
    }
    this.pushCounts()
  }

  /** Current counts, only for the extensions that display them: tab, then extension. */
  actionCounts(): ExtensionActionCounts {
    const out: ExtensionActionCounts = {}
    for (const [tab, per] of this.counts) {
      for (const [ext, n] of per) {
        if (n > 0 && this.store.displayActionCount(ext)) (out[tab] ??= {})[ext] = n
      }
    }
    return out
  }

  /** Tells the toolbar, at most every 250 ms (trailing), so a page with hundreds of blocked requests sends few messages. */
  private pushCounts(): void {
    if (this.countsTimer || !this.deps.onCounts) return
    this.countsTimer = setTimeout(() => {
      this.countsTimer = null
      this.deps.onCounts?.(this.actionCounts())
    }, COUNTS_PUSH_MS)
  }

  private setActionOptions(caller: ApiCaller, options: unknown): void {
    const x = this.state(caller)
    const o = isObject(options) ? options : {}
    if (o.displayActionCountAsBadgeText !== undefined) {
      const on = o.displayActionCountAsBadgeText === true
      this.store.setDisplayActionCount(x.id, on)
      if (!on) {
        for (const [tab, per] of this.counts) {
          per.delete(x.id)
          if (!per.size) this.counts.delete(tab)
        }
      }
      this.pushCounts()
    }
    if (o.tabUpdate !== undefined) {
      const u = o.tabUpdate
      if (!isObject(u) || typeof u.tabId !== 'number' || !Number.isInteger(u.tabId) || typeof u.increment !== 'number' || !Number.isFinite(u.increment))
        throw new ApiError('Invalid arguments.')
      this.count(u.tabId, x.id, Math.trunc(u.increment))
    }
  }

  private sendHeaders(ses: Session, d: RequestDetails, cb: (r: { requestHeaders?: Record<string, string> }) => void): void {
    const parsed = this.request(d)
    const res = parsed && this.decide(ses, parsed.q, parsed.frame, false)
    if (!parsed || !res || !res.requestHeaders.length || !d.requestHeaders) return cb({})
    this.record(
      parsed.q,
      res.used.filter((r) => hasList(r, 'requestHeaders'))
    )
    cb({ requestHeaders: applyHeaderOps(d.requestHeaders, res.requestHeaders, false) })
  }

  private receivedHeaders(ses: Session, d: RequestDetails, cb: (r: { responseHeaders?: Record<string, string[]> }) => void): void {
    const parsed = this.request(d)
    const res = parsed && this.decide(ses, parsed.q, parsed.frame, false)
    if (!parsed || !res || !res.responseHeaders.length || !d.responseHeaders) return cb({})
    this.record(
      parsed.q,
      res.used.filter((r) => hasList(r, 'responseHeaders'))
    )
    cb({ responseHeaders: applyHeaderOps(d.responseHeaders, res.responseHeaders, true) })
  }

  // ───────────────────────── Matched rules ─────────────────────────

  private record(q: DnrRequest, rules: CompiledRule[]): void {
    const time = this.now()
    for (const r of rules) {
      if (!this.exts.get(r.ext)?.feedback) continue
      const list = this.matchedLog.get(q.tabId) ?? this.matchedLog.set(q.tabId, []).get(q.tabId)!
      list.push({ ext: r.ext, set: r.set, id: r.id, time })
      if (list.length > LOG_MAX_PER_TAB) list.splice(0, list.length - LOG_MAX_PER_TAB)
    }
    if (time - this.lastSweep > 60_000) {
      this.lastSweep = time
      for (const [tab, list] of this.matchedLog) {
        const fresh = list.filter((e) => e.time >= time - LOG_WINDOW_MS)
        if (fresh.length) this.matchedLog.set(tab, fresh)
        else this.matchedLog.delete(tab)
      }
    }
  }

  private matched(
    caller: ApiCaller,
    filter: unknown
  ): { rulesMatchedInfo: { rule: { ruleId: number; rulesetId: string }; tabId: number; timeStamp: number }[] } {
    const x = this.state(caller)
    if (!x.feedback) throw new ApiError("The 'declarativeNetRequestFeedback' permission is required.")
    const f = isObject(filter) ? filter : {}
    const since = Math.max(typeof f.minTimeStamp === 'number' ? f.minTimeStamp : 0, this.now() - LOG_WINDOW_MS)
    const tabs = typeof f.tabId === 'number' ? [f.tabId] : [...this.matchedLog.keys()]
    const out = []
    for (const tabId of tabs) {
      for (const e of this.matchedLog.get(tabId) ?? []) {
        if (e.ext === x.id && e.time >= since) out.push({ rule: { ruleId: e.id, rulesetId: e.set }, tabId, timeStamp: e.time })
      }
    }
    return { rulesMatchedInfo: out }
  }

  /** `testMatchOutcome`: which of the extension's rules decide for a described request. */
  private testOutcome(caller: ApiCaller, request: unknown): { matchedRules: { ruleId: number; rulesetId: string }[] } {
    const x = this.state(caller)
    const o = isObject(request) ? request : {}
    if (typeof o.url !== 'string' || typeof o.type !== 'string') throw new ApiError('Invalid arguments.')
    const q = makeRequest({
      url: o.url,
      type: o.type,
      method: typeof o.method === 'string' ? o.method : 'get',
      initiator: typeof o.initiator === 'string' ? o.initiator : undefined,
      tabId: typeof o.tabId === 'number' ? o.tabId : -1
    })
    if (!q) throw new ApiError('Invalid arguments.')
    const matches = this.index.match(q).filter((r) => r.ext === x.id)
    const res = resolve(matches, q, { rank: () => 0, canAct: (r) => this.canAct(caller.ses, r, q) })
    return { matchedRules: res.used.map((r) => ({ ruleId: r.id, rulesetId: r.set })) }
  }

  // ───────────────────────── API ─────────────────────────

  methods(): Record<string, ApiHandler> {
    return {
      'declarativeNetRequest.getEnabledRulesets': (c) => this.enabledIds(this.state(c)),
      'declarativeNetRequest.updateEnabledRulesets': (c, o) => this.updateEnabled(c, o),
      'declarativeNetRequest.getAvailableStaticRuleCount': (c) => this.availableStaticRules(this.state(c)),
      'declarativeNetRequest.getMatchedRules': (c, f) => this.matched(c, f),
      'declarativeNetRequest.testMatchOutcome': (c, r) => this.testOutcome(c, r),
      'declarativeNetRequest.setExtensionActionOptions': (c, o) => this.setActionOptions(c, o)
    }
  }
}

const hasList = (r: CompiledRule, key: 'requestHeaders' | 'responseHeaders'): boolean => {
  const list = r.raw.action[key]
  return Array.isArray(list) && list.length > 0
}

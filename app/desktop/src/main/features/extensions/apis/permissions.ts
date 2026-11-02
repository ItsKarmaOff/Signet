import { readFileSync } from 'node:fs'
import { AtomicWriter } from '../../../core/atomic'
import { compilePattern } from '../../../web/network'
import { ApiError, type ApiCaller, type ApiHandler, type ExtensionEmitter } from './types'

/** What an extension holds or asks for: API permissions and host match patterns. */
export interface PermissionSet {
  permissions: string[]
  origins: string[]
}

/** Shows the user what is about to be granted; resolves true to allow. */
export type PermissionPrompt = (caller: Pick<ApiCaller, 'ses' | 'extensionId'>, wanted: PermissionSet) => Promise<boolean>

const NOT_DECLARED = 'Only permissions specified in the manifest may be requested.'
const REQUIRED = 'You cannot remove required permissions.'

const strings = (v: unknown): string[] => (Array.isArray(v) ? v.filter((s): s is string => typeof s === 'string') : [])
const uniq = (list: string[]): string[] => [...new Set(list)]
/** MV2 manifests list host patterns inside `permissions`. */
const isHost = (p: string): boolean => p === '<all_urls>' || p.includes('://')
const empty = (s: PermissionSet): boolean => !s.permissions.length && !s.origins.length

/** What the manifest declares, split into API permissions and hosts, required and optional. */
export function declaredOf(manifest: unknown): { required: PermissionSet; optional: PermissionSet } {
  const m = (manifest && typeof manifest === 'object' ? manifest : {}) as Record<string, unknown>
  const required = strings(m.permissions)
  const optional = strings(m.optional_permissions)
  return {
    required: { permissions: required.filter((p) => !isHost(p)), origins: [...strings(m.host_permissions), ...required.filter(isHost)] },
    optional: { permissions: optional.filter((p) => !isHost(p)), origins: [...strings(m.optional_host_permissions), ...optional.filter(isHost)] }
  }
}

const ALL_URL_SCHEMES = new Set(['http', 'https', 'ws', 'wss', 'ftp', 'file'])

interface Parsed {
  scheme: string
  host: string
  port?: string
  path: string
}

function parse(pattern: string): Parsed | null {
  const m = /^(\*|[a-z][a-z0-9+.-]*):\/\/([^/]*)(\/.*)$/i.exec(pattern)
  if (!m) return null
  const portAt = /:(\d+|\*)$/.exec(m[2])
  return { scheme: m[1].toLowerCase(), host: (portAt ? m[2].slice(0, portAt.index) : m[2]).toLowerCase(), port: portAt?.[1], path: m[3] }
}

/** True if every URL matched by the match pattern `wanted` is also matched by `held`. */
export function patternCovers(held: string, wanted: string): boolean {
  if (held === wanted) return true
  const w = wanted === '<all_urls>' ? null : parse(wanted)
  if (held === '<all_urls>') return wanted === '<all_urls>' || (!!w && (w.scheme === '*' || ALL_URL_SCHEMES.has(w.scheme)))
  const h = parse(held)
  if (!h || !w) return false
  if (h.scheme !== '*' ? h.scheme !== w.scheme : w.scheme !== '*' && w.scheme !== 'http' && w.scheme !== 'https') return false
  if (h.host.startsWith('*.')) {
    const base = h.host.slice(2)
    const host = w.host.startsWith('*.') ? w.host.slice(2) : w.host
    if (host !== base && !host.endsWith(`.${base}`)) return false
  } else if (h.host !== '*' && h.host !== w.host) return false
  if (h.port && h.port !== '*' && h.port !== w.port) return false
  const re = new RegExp(
    `^${h.path
      .split('*')
      .map((s) => s.replace(/[.+?^${}()|[\]\\]/g, '\\$&'))
      .join('.*')}$`
  )
  return re.test(w.path)
}

const coveredByAny = (held: string[], origin: string): boolean => held.some((p) => patternCovers(p, origin))

/** Validates the argument shared by request, contains and remove. */
export function parseRequest(raw: unknown): PermissionSet {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) throw new ApiError('Invalid arguments.')
  const { permissions, origins } = raw as Record<string, unknown>
  for (const list of [permissions, origins]) {
    if (list !== undefined && (!Array.isArray(list) || list.some((s) => typeof s !== 'string'))) throw new ApiError('Invalid arguments.')
  }
  const set = { permissions: uniq(strings(permissions)), origins: uniq(strings(origins)) }
  for (const o of set.origins) if (!compilePattern(o)) throw new ApiError(`Invalid host permission pattern '${o}'.`)
  return set
}

/**
 * Optional permissions granted at runtime (`chrome.permissions.request`), persisted per extension in
 * `{ [extensionId]: { permissions, origins } }`. Only runtime grants live here; what the manifest requires is read from
 * the manifest. The effective permissions of an extension are both together.
 */
export class PermissionGrants {
  private grants = new Map<string, PermissionSet>()
  private writer: AtomicWriter
  private listeners = new Set<(extensionId: string) => void>()
  private queues = new Map<string, Promise<unknown>>()
  /** Set by the app: shows the native dialog. Denies until then. */
  prompt: PermissionPrompt = () => Promise.resolve(false)

  constructor(
    file: string,
    private emitter: Pick<ExtensionEmitter, 'emit'>
  ) {
    try {
      const data = JSON.parse(readFileSync(file, 'utf8')) as Record<string, Partial<PermissionSet> | null>
      for (const [id, g] of Object.entries(data)) {
        const set = { permissions: uniq(strings(g?.permissions)), origins: uniq(strings(g?.origins)) }
        if (!empty(set)) this.grants.set(id, set)
      }
    } catch {
      // no file yet
    }
    this.writer = new AtomicWriter(file, () => JSON.stringify(Object.fromEntries(this.grants)))
  }

  flush(): boolean {
    return this.writer.flushSync()
  }

  /** Called with the extension id after its grants changed (the DNR engine recomputes its host access). */
  onChange(fn: (extensionId: string) => void): void {
    this.listeners.add(fn)
  }

  private changed(id: string): void {
    this.writer.request()
    for (const fn of this.listeners) fn(id)
  }

  /** API permissions granted at runtime. */
  permissionsOf(id: string): string[] {
    return this.grants.get(id)?.permissions ?? []
  }

  /** Host patterns granted at runtime. */
  originsOf(id: string): string[] {
    return this.grants.get(id)?.origins ?? []
  }

  /** Manifest requirements plus runtime grants. */
  effective(manifest: unknown, id: string): PermissionSet {
    const { required } = declaredOf(manifest)
    return { permissions: uniq([...required.permissions, ...this.permissionsOf(id)]), origins: uniq([...required.origins, ...this.originsOf(id)]) }
  }

  /** The part of `wanted` the extension does not hold yet. */
  private missing(manifest: unknown, id: string, wanted: PermissionSet): PermissionSet {
    const held = this.effective(manifest, id)
    return {
      permissions: wanted.permissions.filter((p) => !held.permissions.includes(p)),
      origins: wanted.origins.filter((o) => !coveredByAny(held.origins, o))
    }
  }

  // ───────────────────────── chrome.permissions ─────────────────────────

  contains(manifest: unknown, id: string, raw: unknown): boolean {
    return empty(this.missing(manifest, id, parseRequest(raw)))
  }

  getAll(manifest: unknown, id: string): PermissionSet {
    return this.effective(manifest, id)
  }

  /** Validates, then asks once for what is new. Requests of one extension are served one after the other. */
  request(caller: Pick<ApiCaller, 'ses' | 'extensionId'>, manifest: unknown, raw: unknown): Promise<boolean> {
    const req = parseRequest(raw)
    const { required, optional } = declaredOf(manifest)
    const declaredOrigins = [...required.origins, ...optional.origins]
    if (req.permissions.some((p) => !required.permissions.includes(p) && !optional.permissions.includes(p))) throw new ApiError(NOT_DECLARED)
    if (req.origins.some((o) => !coveredByAny(declaredOrigins, o))) throw new ApiError(NOT_DECLARED)
    const id = caller.extensionId
    return this.enqueue(id, async () => {
      // Recomputed here: an earlier request in the queue may have granted part of it.
      const need = this.missing(manifest, id, req)
      if (empty(need)) return true
      if (!(await this.prompt(caller, need))) return false
      const set = this.grants.get(id) ?? { permissions: [], origins: [] }
      set.permissions = uniq([...set.permissions, ...need.permissions])
      set.origins = uniq([...set.origins, ...need.origins])
      this.grants.set(id, set)
      this.changed(id)
      this.emitter.emit(id, 'permissions.onAdded', [need], true)
      return true
    })
  }

  /** Required items cannot be removed; optional grants are dropped. Removing what is not held succeeds, as in Chrome. */
  remove(manifest: unknown, id: string, raw: unknown): boolean {
    const req = parseRequest(raw)
    const { required } = declaredOf(manifest)
    if (req.permissions.some((p) => required.permissions.includes(p)) || req.origins.some((o) => coveredByAny(required.origins, o)))
      throw new ApiError(REQUIRED)
    const set = this.grants.get(id)
    if (!set) return true
    const removed = {
      permissions: set.permissions.filter((p) => req.permissions.includes(p)),
      origins: set.origins.filter((o) => req.origins.includes(o))
    }
    if (empty(removed)) return true
    set.permissions = set.permissions.filter((p) => !removed.permissions.includes(p))
    set.origins = set.origins.filter((o) => !removed.origins.includes(o))
    if (empty(set)) this.grants.delete(id)
    this.changed(id)
    this.emitter.emit(id, 'permissions.onRemoved', [removed], true)
    return true
  }

  private enqueue<T>(id: string, task: () => Promise<T>): Promise<T> {
    const run = (this.queues.get(id) ?? Promise.resolve()).then(task, task)
    const tail = run.catch(() => {})
    this.queues.set(id, tail)
    void tail.then(() => this.queues.get(id) === tail && this.queues.delete(id))
    return run
  }

  // ───────────────────────── Lifecycle ─────────────────────────

  /** The extension (re)loaded: grants for items its manifest no longer lists as optional are dropped. */
  reconcile(id: string, manifest: unknown): void {
    const set = this.grants.get(id)
    if (!set) return
    const { required, optional } = declaredOf(manifest)
    const keep: PermissionSet = {
      permissions: set.permissions.filter((p) => optional.permissions.includes(p) && !required.permissions.includes(p)),
      origins: set.origins.filter((o) => coveredByAny(optional.origins, o) && !coveredByAny(required.origins, o))
    }
    if (keep.permissions.length === set.permissions.length && keep.origins.length === set.origins.length) return
    if (empty(keep)) this.grants.delete(id)
    else this.grants.set(id, keep)
    this.changed(id)
  }

  /** The extension is uninstalled. */
  drop(id: string): void {
    if (this.grants.delete(id)) this.changed(id)
  }

  methods(): Record<string, ApiHandler> {
    const manifestOf = ({ ses, extensionId }: ApiCaller): unknown => ses.extensions.getExtension(extensionId)?.manifest
    return {
      'permissions.request': (c, req) => this.request(c, manifestOf(c), req),
      'permissions.contains': (c, req) => this.contains(manifestOf(c), c.extensionId, req),
      'permissions.getAll': (c) => this.getAll(manifestOf(c), c.extensionId),
      'permissions.remove': (c, req) => this.remove(manifestOf(c), c.extensionId, req)
    }
  }
}

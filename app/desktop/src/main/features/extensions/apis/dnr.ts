import { readFileSync } from 'node:fs'
import { AtomicWriter } from '../../../core/atomic'
import { DNR_CONSTANTS, DNR_ENUMS } from '@shared/extension-apis'
import { ApiError, type ApiHandler } from './types'

/** Shape we rely on; the rest of a rule is kept untouched. */
export interface Rule {
  id: number
  priority?: number
  action: { type: string; [k: string]: unknown }
  condition: Record<string, unknown>
  [k: string]: unknown
}

const isObject = (v: unknown): v is Record<string, unknown> => !!v && typeof v === 'object' && !Array.isArray(v)

const values = (e: Record<string, string>) => Object.values(e)
const ACTIONS = values(DNR_ENUMS.RuleActionType)
const OPERATIONS = values(DNR_ENUMS.HeaderOperation)
const RESOURCES = values(DNR_ENUMS.ResourceType)
const METHODS = values(DNR_ENUMS.RequestMethod)
const DOMAIN_TYPES = values(DNR_ENUMS.DomainType)

/** Every item of `list` (if present) must be one of `allowed`: Chrome's enums are exact strings. */
function oneOf(ruleId: number, where: string, list: unknown, allowed: string[]): void {
  if (list === undefined) return
  if (!Array.isArray(list) || list.some((v) => !allowed.includes(String(v))))
    throw new ApiError(`Rule with id ${ruleId}: invalid ${where}, expected one of: ${allowed.join(', ')}.`)
}

/** Throws an `ApiError` like Chrome for a rule that cannot be stored. */
export function validateRule(rule: unknown): Rule {
  if (!isObject(rule)) throw new ApiError('Invalid rule.')
  const { id, action, condition } = rule
  if (typeof id !== 'number' || !Number.isInteger(id) || id < 1) throw new ApiError(`Rule with id ${String(id)} must have a positive integer id.`)
  if (!isObject(action) || !ACTIONS.includes(String(action.type)))
    throw new ApiError(`Rule with id ${id}: invalid action type, expected one of: ${ACTIONS.join(', ')}.`)
  if (!isObject(condition)) throw new ApiError(`Rule with id ${id}: missing condition.`)
  if (rule.priority !== undefined && (typeof rule.priority !== 'number' || !Number.isInteger(rule.priority) || rule.priority < 1)) {
    throw new ApiError(`Rule with id ${id}: priority must be an integer of at least 1.`)
  }
  for (const key of ['requestHeaders', 'responseHeaders']) {
    const headers = action[key]
    if (headers === undefined) continue
    if (!Array.isArray(headers)) throw new ApiError(`Rule with id ${id}: invalid ${key}.`)
    oneOf(
      id,
      `${key} operation`,
      headers.map((h) => (isObject(h) ? h.operation : undefined)),
      OPERATIONS
    )
  }
  oneOf(id, 'resourceTypes', condition.resourceTypes, RESOURCES)
  oneOf(id, 'excludedResourceTypes', condition.excludedResourceTypes, RESOURCES)
  oneOf(id, 'requestMethods', condition.requestMethods, METHODS)
  oneOf(id, 'excludedRequestMethods', condition.excludedRequestMethods, METHODS)
  if (condition.domainType !== undefined) oneOf(id, 'domainType', [condition.domainType], DOMAIN_TYPES)
  return rule as Rule
}

/**
 * `chrome.declarativeNetRequest` rule storage. Dynamic rules persist per extension (like Chrome), session rules live in
 * memory. NOTHING IS ENFORCED YET: Electron has one `session.webRequest` handler per event, and taking
 * onBeforeSendHeaders would silence the extensions' own `chrome.webRequest` listeners. Rules are only kept so the
 * extension's bookkeeping (getDynamicRules, unique ids) behaves.
 */
export class DnrStore {
  private dynamic = new Map<string, Map<number, Rule>>()
  private session = new Map<string, Map<number, Rule>>()
  private writer: AtomicWriter
  private warned = new Set<string>()

  constructor(
    file: string,
    private log: (message: string) => void = () => {}
  ) {
    try {
      const data = JSON.parse(readFileSync(file, 'utf8')) as { dynamic?: Record<string, unknown[]> }
      for (const [id, rules] of Object.entries(data.dynamic ?? {})) {
        const map = new Map<number, Rule>()
        let dropped = 0
        for (const r of Array.isArray(rules) ? rules : []) {
          try {
            const rule = validateRule(r)
            map.set(rule.id, rule)
          } catch {
            dropped++ // corrupt, or written with enum names instead of Chrome's values
          }
        }
        if (dropped)
          this.log(`[extension-apis] ${id} declarativeNetRequest: dropped ${dropped} stored rule(s) that are not valid (the extension recreates them)`)
        if (map.size) this.dynamic.set(id, map)
      }
    } catch {
      // no file yet
    }
    this.writer = new AtomicWriter(file, () =>
      JSON.stringify({ version: 1, dynamic: Object.fromEntries([...this.dynamic].map(([id, m]) => [id, [...m.values()]])) })
    )
  }

  flush(): boolean {
    return this.writer.flushSync()
  }

  private store(kind: 'dynamic' | 'session', id: string): Map<number, Rule> {
    const m = this[kind]
    return m.get(id) ?? m.set(id, new Map()).get(id)!
  }

  /** All-or-nothing, removals first, like Chrome. */
  update(kind: 'dynamic' | 'session', id: string, options: unknown): void {
    const o = (isObject(options) ? options : {}) as { addRules?: unknown; removeRuleIds?: unknown }
    const removeIds = o.removeRuleIds === undefined ? [] : o.removeRuleIds
    const add = o.addRules === undefined ? [] : o.addRules
    if (!Array.isArray(removeIds) || removeIds.some((n) => typeof n !== 'number') || !Array.isArray(add)) throw new ApiError('Invalid arguments.')
    const next = new Map(this.store(kind, id))
    for (const n of removeIds as number[]) next.delete(n)
    for (const raw of add) {
      const rule = validateRule(raw)
      if (next.has(rule.id)) throw new ApiError(`Rule with id ${rule.id} does not have a unique ID.`)
      next.set(rule.id, rule)
    }
    const max = kind === 'dynamic' ? DNR_CONSTANTS.MAX_NUMBER_OF_DYNAMIC_RULES : DNR_CONSTANTS.MAX_NUMBER_OF_SESSION_RULES
    if (next.size > max) throw new ApiError('Rule count exceeds the limit.')
    this[kind].set(id, next)
    if (kind === 'dynamic') this.writer.request()
    if (add.length && !this.warned.has(id)) {
      this.warned.add(id)
      this.log(`[extension-apis] ${id} declarativeNetRequest: ${next.size} ${kind} rule(s) stored, NOT enforced yet`)
    }
  }

  get(kind: 'dynamic' | 'session', id: string, filter: unknown): Rule[] {
    const ids = isObject(filter) && Array.isArray(filter.ruleIds) ? new Set(filter.ruleIds) : null
    return [...this.store(kind, id).values()].filter((r) => !ids || ids.has(r.id))
  }

  /** Extension uninstalled: its rules go with it. */
  remove(id: string): void {
    this.session.delete(id)
    if (this.dynamic.delete(id)) this.writer.request()
  }

  methods(): Record<string, ApiHandler> {
    return {
      'declarativeNetRequest.updateDynamicRules': ({ extensionId }, o) => this.update('dynamic', extensionId, o),
      'declarativeNetRequest.getDynamicRules': ({ extensionId }, f) => this.get('dynamic', extensionId, f),
      'declarativeNetRequest.updateSessionRules': ({ extensionId }, o) => this.update('session', extensionId, o),
      'declarativeNetRequest.getSessionRules': ({ extensionId }, f) => this.get('session', extensionId, f),
      'declarativeNetRequest.getEnabledRulesets': () => [],
      'declarativeNetRequest.updateEnabledRulesets': () => undefined,
      'declarativeNetRequest.getAvailableStaticRuleCount': () => DNR_CONSTANTS.GUARANTEED_MINIMUM_STATIC_RULES,
      'declarativeNetRequest.isRegexSupported': (_c, o) => ({ isSupported: isObject(o) && typeof o.regex === 'string' }),
      'declarativeNetRequest.getMatchedRules': () => ({ rulesMatchedInfo: [] })
    }
  }
}

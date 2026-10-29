import { ApiError } from './types'

/** MV3 minimum: Chrome rounds shorter delays and periods up to 30 seconds. */
export const MIN_ALARM_MS = 30_000

/** `chrome.alarms.Alarm`. */
export interface Alarm {
  name: string
  scheduledTime: number
  periodInMinutes?: number
}

/** `chrome.alarms.AlarmCreateInfo`. */
export interface AlarmInfo {
  when?: number
  delayInMinutes?: number
  periodInMinutes?: number
}

const finite = (v: unknown): v is number => typeof v === 'number' && Number.isFinite(v)

/** Builds the alarm an `alarms.create` call describes; throws an `ApiError` for what Chrome rejects. */
export function buildAlarm(name: string, info: unknown, now: number): Alarm {
  const i = (info && typeof info === 'object' ? info : {}) as Record<string, unknown>
  for (const key of ['when', 'delayInMinutes', 'periodInMinutes']) {
    if (i[key] !== undefined && !finite(i[key])) throw new ApiError(`Invalid value for argument. Property '${key}': Expected 'number'.`)
  }
  const { when, delayInMinutes, periodInMinutes } = i as AlarmInfo
  if (when !== undefined && delayInMinutes !== undefined) throw new ApiError('Cannot set both when and delayInMinutes.')
  if (when === undefined && delayInMinutes === undefined && periodInMinutes === undefined) {
    throw new ApiError('Must set at least one of when, delayInMinutes, or periodInMinutes.')
  }
  if (periodInMinutes !== undefined && periodInMinutes <= 0) throw new ApiError('Period must be greater than 0.')
  if (delayInMinutes !== undefined && delayInMinutes < 0) throw new ApiError('Delay cannot be negative.')

  const period = periodInMinutes === undefined ? undefined : Math.max(periodInMinutes * 60_000, MIN_ALARM_MS)
  const firstDelay = when !== undefined ? when - now : delayInMinutes !== undefined ? delayInMinutes * 60_000 : (period ?? 0)
  return {
    name,
    scheduledTime: now + Math.max(firstDelay, MIN_ALARM_MS),
    ...(period !== undefined && { periodInMinutes: period / 60_000 })
  }
}

/**
 * Next time a repeating alarm fires after having fired (or been missed) at `now`: the next slot of its period that is
 * still in the future, so a long sleep fires once instead of replaying every missed tick.
 */
export function nextOccurrence(alarm: Alarm, now: number): number {
  const period = (alarm.periodInMinutes ?? 0) * 60_000
  if (period <= 0) return alarm.scheduledTime
  return alarm.scheduledTime + period * (Math.floor((now - alarm.scheduledTime) / period) + 1)
}

/** `setTimeout` cannot wait longer than a signed 32-bit number of milliseconds. */
const MAX_TIMEOUT = 2 ** 31 - 1

export interface SchedulerHooks {
  /** Delivers `onAlarm` to the extension (waking its worker). */
  fire(extensionId: string, alarm: Alarm): void
  /** The set of alarms changed: persist it. */
  changed(): void
  now?: () => number
}

/**
 * Alarms of all extensions with their timers. Alarms are kept per extension id; timers only run for extensions that
 * are loaded (`activate`). An alarm whose time passed while the app was closed fires once on activation, like Chrome.
 */
export class AlarmScheduler {
  private alarms = new Map<string, Map<string, Alarm>>()
  private timers = new Map<string, ReturnType<typeof setTimeout>>()
  private now: () => number

  constructor(
    private hooks: SchedulerHooks,
    initial: Record<string, Alarm[]> = {}
  ) {
    this.now = hooks.now ?? Date.now
    for (const [id, list] of Object.entries(initial)) {
      const map = new Map<string, Alarm>()
      for (const a of list) if (typeof a?.name === 'string' && finite(a.scheduledTime)) map.set(a.name, a)
      if (map.size) this.alarms.set(id, map)
    }
  }

  private key = (id: string, name: string) => `${id}\0${name}`

  create(id: string, name: string, info: unknown): void {
    const alarm = buildAlarm(name, info, this.now())
    this.cancel(id, name)
    const map = this.alarms.get(id) ?? new Map<string, Alarm>()
    map.set(name, alarm)
    this.alarms.set(id, map)
    this.arm(id, alarm)
    this.hooks.changed()
  }

  get(id: string, name: string): Alarm | undefined {
    const a = this.alarms.get(id)?.get(name)
    return a && { ...a }
  }

  getAll(id: string): Alarm[] {
    return [...(this.alarms.get(id)?.values() ?? [])].map((a) => ({ ...a }))
  }

  clear(id: string, name: string): boolean {
    this.cancel(id, name)
    const removed = this.alarms.get(id)?.delete(name) ?? false
    if (removed) this.hooks.changed()
    return removed
  }

  clearAll(id: string): boolean {
    const had = (this.alarms.get(id)?.size ?? 0) > 0
    this.remove(id)
    return had
  }

  /** The extension is loaded: start its timers, firing the ones that came due while it was away. */
  activate(id: string): void {
    for (const alarm of this.alarms.get(id)?.values() ?? []) this.arm(id, alarm)
  }

  /** The extension is gone from every session: forget its alarms (Chrome also drops them on update and uninstall). */
  remove(id: string): void {
    const map = this.alarms.get(id)
    if (!map) return
    for (const name of map.keys()) this.cancel(id, name)
    this.alarms.delete(id)
    this.hooks.changed()
  }

  /** Stops every timer (shutdown, tests). */
  stop(): void {
    for (const t of this.timers.values()) clearTimeout(t)
    this.timers.clear()
  }

  serialize(): Record<string, Alarm[]> {
    return Object.fromEntries([...this.alarms].map(([id, map]) => [id, [...map.values()]]))
  }

  private cancel(id: string, name: string): void {
    const k = this.key(id, name)
    const t = this.timers.get(k)
    if (t) clearTimeout(t)
    this.timers.delete(k)
  }

  private arm(id: string, alarm: Alarm): void {
    this.cancel(id, alarm.name)
    const delay = Math.max(0, alarm.scheduledTime - this.now())
    const timer = setTimeout(
      () => {
        this.timers.delete(this.key(id, alarm.name))
        // Too far for one timer: wait again.
        if (this.now() < alarm.scheduledTime && delay > MAX_TIMEOUT) return this.arm(id, alarm)
        this.due(id, alarm)
      },
      Math.min(delay, MAX_TIMEOUT)
    )
    this.timers.set(this.key(id, alarm.name), timer)
  }

  private due(id: string, alarm: Alarm): void {
    const map = this.alarms.get(id)
    // Replaced or cleared while the timer was pending.
    if (!map || map.get(alarm.name) !== alarm) return
    this.hooks.fire(id, { ...alarm })
    if (alarm.periodInMinutes) {
      const next = { ...alarm, scheduledTime: nextOccurrence(alarm, this.now()) }
      map.set(alarm.name, next)
      this.arm(id, next)
    } else map.delete(alarm.name)
    this.hooks.changed()
  }
}

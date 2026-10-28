import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { AlarmScheduler, buildAlarm, MIN_ALARM_MS, nextOccurrence, type Alarm } from './schedule'

const T0 = Date.UTC(2026, 0, 1)

describe('buildAlarm', () => {
  it('rounds delays and periods up to 30 seconds', () => {
    expect(buildAlarm('a', { delayInMinutes: 0.1 }, T0).scheduledTime).toBe(T0 + MIN_ALARM_MS)
    expect(buildAlarm('a', { when: T0 - 5000 }, T0).scheduledTime).toBe(T0 + MIN_ALARM_MS)
    expect(buildAlarm('a', { periodInMinutes: 0.1 }, T0)).toEqual({ name: 'a', scheduledTime: T0 + MIN_ALARM_MS, periodInMinutes: 0.5 })
  })

  it('first run: when, delay, or one period', () => {
    expect(buildAlarm('a', { when: T0 + 120_000 }, T0).scheduledTime).toBe(T0 + 120_000)
    expect(buildAlarm('a', { delayInMinutes: 2, periodInMinutes: 5 }, T0)).toEqual({ name: 'a', scheduledTime: T0 + 120_000, periodInMinutes: 5 })
    expect(buildAlarm('a', { periodInMinutes: 3 }, T0).scheduledTime).toBe(T0 + 180_000)
  })

  it('rejects what Chrome rejects', () => {
    expect(() => buildAlarm('a', {}, T0)).toThrow(/at least one/)
    expect(() => buildAlarm('a', { when: 1, delayInMinutes: 1 }, T0)).toThrow(/both/)
    expect(() => buildAlarm('a', { periodInMinutes: 0 }, T0)).toThrow()
    expect(() => buildAlarm('a', { delayInMinutes: 'x' }, T0)).toThrow()
  })
})

describe('nextOccurrence', () => {
  it('skips the missed ticks of a long sleep', () => {
    const a: Alarm = { name: 'a', scheduledTime: T0, periodInMinutes: 1 }
    expect(nextOccurrence(a, T0)).toBe(T0 + 60_000)
    expect(nextOccurrence(a, T0 + 10 * 60_000 + 5)).toBe(T0 + 11 * 60_000)
  })
})

describe('AlarmScheduler', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    vi.setSystemTime(T0)
  })
  afterEach(() => vi.useRealTimers())

  const make = (initial: Record<string, Alarm[]> = {}) => {
    const fire = vi.fn()
    const changed = vi.fn()
    return { fire, changed, s: new AlarmScheduler({ fire, changed }, initial) }
  }

  it('fires a one-shot alarm once, then forgets it', () => {
    const { s, fire } = make()
    s.create('ext', 'once', { delayInMinutes: 1 })
    expect(s.get('ext', 'once')).toBeDefined()
    vi.advanceTimersByTime(59_999)
    expect(fire).not.toHaveBeenCalled()
    vi.advanceTimersByTime(1)
    expect(fire).toHaveBeenCalledWith('ext', { name: 'once', scheduledTime: T0 + 60_000 })
    expect(s.get('ext', 'once')).toBeUndefined()
    vi.advanceTimersByTime(10 * 60_000)
    expect(fire).toHaveBeenCalledOnce()
  })

  it('repeats a periodic alarm until cleared', () => {
    const { s, fire } = make()
    s.create('ext', 'tick', { periodInMinutes: 1 })
    vi.advanceTimersByTime(3 * 60_000)
    expect(fire).toHaveBeenCalledTimes(3)
    expect(s.clear('ext', 'tick')).toBe(true)
    expect(s.clear('ext', 'tick')).toBe(false)
    vi.advanceTimersByTime(5 * 60_000)
    expect(fire).toHaveBeenCalledTimes(3)
  })

  it('replaces an alarm of the same name and keeps extensions apart', () => {
    const { s, fire } = make()
    s.create('a', 'x', { delayInMinutes: 1 })
    s.create('a', 'x', { delayInMinutes: 2 })
    s.create('b', 'x', { delayInMinutes: 1 })
    vi.advanceTimersByTime(60_000)
    expect(fire).toHaveBeenCalledTimes(1)
    expect(fire).toHaveBeenCalledWith('b', expect.anything())
    expect(s.getAll('a')).toHaveLength(1)
    expect(s.clearAll('a')).toBe(true)
    expect(s.getAll('a')).toEqual([])
  })

  it('fires once on activation an alarm that came due while the app was closed', () => {
    const missed: Alarm = { name: 'late', scheduledTime: T0 - 3_600_000 }
    const periodic: Alarm = { name: 'p', scheduledTime: T0 - 3_600_000, periodInMinutes: 10 }
    const future: Alarm = { name: 'soon', scheduledTime: T0 + 60_000 }
    const { s, fire } = make({ ext: [missed, periodic, future] })
    vi.advanceTimersByTime(10_000)
    expect(fire).not.toHaveBeenCalled() // dormant until the extension loads
    s.activate('ext')
    vi.advanceTimersByTime(0)
    expect(fire.mock.calls.map((c) => c[1].name).sort()).toEqual(['late', 'p'])
    expect(s.get('ext', 'late')).toBeUndefined()
    expect(s.get('ext', 'p')!.scheduledTime).toBe(T0 + 10_000 + (10 * 60_000 - 10_000))
    vi.advanceTimersByTime(60_000)
    expect(fire.mock.calls.filter((c) => c[1].name === 'soon')).toHaveLength(1)
  })

  it('removes everything and stops timers when the extension goes away', () => {
    const { s, fire } = make()
    s.create('ext', 'x', { periodInMinutes: 1 })
    s.remove('ext')
    vi.advanceTimersByTime(10 * 60_000)
    expect(fire).not.toHaveBeenCalled()
    expect(s.serialize()).toEqual({})
  })

  it('waits several times for a delay beyond the timer limit', () => {
    const { s, fire } = make()
    const day = 24 * 60 * 60_000
    s.create('ext', 'far', { when: T0 + 40 * day })
    vi.advanceTimersByTime(39 * day)
    expect(fire).not.toHaveBeenCalled()
    vi.advanceTimersByTime(day + 1)
    expect(fire).toHaveBeenCalledOnce()
  })

  it('reports changes for persistence', () => {
    const { s, changed } = make()
    s.create('ext', 'x', { delayInMinutes: 1 })
    expect(changed).toHaveBeenCalled()
    expect(s.serialize()).toEqual({ ext: [{ name: 'x', scheduledTime: T0 + 60_000 }] })
  })
})

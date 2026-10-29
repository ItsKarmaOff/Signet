import { readFileSync } from 'node:fs'
import { AtomicWriter } from '../../../core/atomic'
import { ALARM_EVENT } from '@shared/extension-apis'
import { AlarmScheduler, type Alarm } from './schedule'
import type { ApiHandler, ExtensionEmitter } from './types'

/** `chrome.alarms`: timers live in main, persisted per extension so they survive a restart. */
export class Alarms {
  readonly scheduler: AlarmScheduler
  private writer: AtomicWriter

  constructor(file: string, emitter: ExtensionEmitter) {
    let initial: Record<string, Alarm[]> = {}
    try {
      const data = JSON.parse(readFileSync(file, 'utf8')) as { alarms?: Record<string, Alarm[]> }
      if (data.alarms && typeof data.alarms === 'object') initial = data.alarms
    } catch {
      // no file yet
    }
    this.scheduler = new AlarmScheduler(
      {
        // Waking the worker is the point of an alarm in MV3.
        fire: (id, alarm) => emitter.emit(id, ALARM_EVENT, [alarm], true),
        changed: () => this.writer.request()
      },
      initial
    )
    this.writer = new AtomicWriter(file, () => JSON.stringify({ version: 1, alarms: this.scheduler.serialize() }))
  }

  flush(): boolean {
    return this.writer.flushSync()
  }

  /** `create(name?, info)`: the name is optional and comes first. */
  methods(): Record<string, ApiHandler> {
    const s = this.scheduler
    const nameOf = (v: unknown) => (typeof v === 'string' ? v : '')
    return {
      'alarms.create': ({ extensionId }, a, b) => (typeof a === 'string' ? s.create(extensionId, a, b) : s.create(extensionId, '', a)),
      'alarms.get': ({ extensionId }, name) => s.get(extensionId, nameOf(name)),
      'alarms.getAll': ({ extensionId }) => s.getAll(extensionId),
      'alarms.clear': ({ extensionId }, name) => s.clear(extensionId, nameOf(name)),
      'alarms.clearAll': ({ extensionId }) => s.clearAll(extensionId)
    }
  }
}

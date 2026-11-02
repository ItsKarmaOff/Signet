import { describe, expect, it } from 'vitest'
import { ReloadScheduler, safeReload, sameSnapshot, type Snapshot } from './unpacked-reload'

const snap = (o: Record<string, string>): Snapshot => new Map(Object.entries(o))

function setup(initial: Record<string, string>) {
  let disk = snap(initial)
  const timers: (() => void)[] = []
  const log: string[] = []
  let releaseReload: (() => void) | undefined
  let hold = false
  const s = new ReloadScheduler({
    delay: 400,
    scan: async () => new Map(disk),
    reload: async () => {
      log.push('reload')
      if (hold) await new Promise<void>((r) => (releaseReload = r))
    },
    setTimer: (fn) => timers.push(fn) - 1,
    clearTimer: (h) => {
      if (typeof h === 'number') timers[h] = () => {}
    }
  })
  s.baseline(new Map(disk))
  return {
    s,
    log,
    write: (file: string, sig: string) => (disk = new Map(disk).set(file, sig)),
    fire: async () => {
      const t = timers.splice(0)
      t.forEach((f) => f())
      await new Promise((r) => setTimeout(r, 0))
    },
    hold: () => (hold = true),
    release: async () => {
      hold = false
      releaseReload?.()
      await new Promise((r) => setTimeout(r, 0))
    }
  }
}

describe('sameSnapshot', () => {
  it('compares files and signatures', () => {
    expect(sameSnapshot(snap({ a: '1' }), snap({ a: '1' }))).toBe(true)
    expect(sameSnapshot(snap({ a: '1' }), snap({ a: '2' }))).toBe(false)
    expect(sameSnapshot(snap({ a: '1' }), snap({ a: '1', b: '1' }))).toBe(false)
  })
})

describe('ReloadScheduler', () => {
  it('ignores events that change nothing (FSEvents replay)', async () => {
    const t = setup({ 'manifest.json': '1:1' })
    t.s.notify()
    t.s.notify()
    await t.fire()
    expect(t.log).toEqual([])
  })

  it('reloads once for a burst of real changes', async () => {
    const t = setup({ 'a.js': '1:1' })
    t.write('a.js', '2:2')
    t.s.notify()
    t.s.notify()
    t.s.notify()
    await t.fire()
    expect(t.log).toEqual(['reload'])
    t.s.notify()
    await t.fire()
    expect(t.log).toEqual(['reload'])
  })

  it('never reloads while a reload runs, then checks once more', async () => {
    const t = setup({ 'a.js': '1:1' })
    t.hold()
    t.write('a.js', '2:2')
    t.s.notify()
    await t.fire()
    expect(t.s.running).toBe(true)
    t.write('a.js', '3:3')
    t.s.notify()
    await t.fire()
    expect(t.log).toEqual(['reload'])
    await t.release()
    await new Promise((r) => setTimeout(r, 0))
    expect(t.log).toEqual(['reload', 'reload'])
  })

  it('does not reload after dispose', async () => {
    const t = setup({ 'a.js': '1:1' })
    t.write('a.js', '2:2')
    t.s.dispose()
    t.s.notify()
    await t.fire()
    expect(t.log).toEqual([])
  })
})

describe('safeReload', () => {
  it('unloads, waits for the worker, then loads', async () => {
    const order: string[] = []
    await safeReload({
      isLoaded: () => true,
      unload: async () => void order.push('unload'),
      workerStopped: async () => void order.push('worker-stopped'),
      load: async () => void order.push('load')
    })
    expect(order).toEqual(['unload', 'worker-stopped', 'load'])
  })
  it('loads directly when not loaded', async () => {
    const order: string[] = []
    await safeReload({
      isLoaded: () => false,
      unload: async () => void order.push('unload'),
      workerStopped: async () => void order.push('w'),
      load: async () => void order.push('load')
    })
    expect(order).toEqual(['load'])
  })
})

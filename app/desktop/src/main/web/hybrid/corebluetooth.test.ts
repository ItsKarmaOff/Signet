import { EventEmitter } from 'node:events'
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { startBleScan, type BleState, type ScanChild, type ScanDeps } from './corebluetooth'

const FIDO = '0000fff9-0000-1000-8000-00805f9b34fb'
const GOOGLE = '0000fde2-0000-1000-8000-00805f9b34fb'

class FakeChild extends EventEmitter {
  stdout = new EventEmitter() as unknown as NodeJS.ReadableStream
  stdin = { end: vi.fn() } as unknown as NodeJS.WritableStream
  exited = false
  signals: string[] = []
  kill = (signal: NodeJS.Signals): void => {
    this.signals.push(signal)
    if (signal === 'SIGKILL') this.exited = true
  }
  push(text: string): void {
    ;(this.stdout as unknown as EventEmitter).emit('data', Buffer.from(text))
  }
}

let dir: string
beforeEach(async () => {
  dir = await mkdtemp(join(tmpdir(), 'cable-scan-test-'))
  await writeFile(join(dir, 'a.swift'), 'source')
  await writeFile(join(dir, 'Info.plist'), 'plist')
  vi.spyOn(console, 'error').mockImplementation(() => {})
})
afterEach(async () => {
  vi.useRealTimers()
  vi.restoreAllMocks()
  await rm(dir, { recursive: true, force: true, maxRetries: 5, retryDelay: 50 })
})

function setup(opts: { compile?: ScanDeps['compile'] } = {}) {
  const child = new FakeChild()
  const quitFns: Array<() => void> = []
  const compile = vi.fn(opts.compile ?? (async (_s: string, _p: string, out: string) => void (await writeFile(out, 'bin'))))
  const spawn = vi.fn(() => child as unknown as ScanChild)
  const deps: ScanDeps = {
    sourcePath: join(dir, 'a.swift'),
    plistPath: join(dir, 'Info.plist'),
    cacheDir: join(dir, 'cache'),
    compile,
    spawn,
    onQuit: (fn) => {
      quitFns.push(fn)
      return () => {}
    }
  }
  const states: BleState[] = []
  const data: Array<[string, string]> = []
  const start = () =>
    startBleScan({
      uuids: [FIDO, GOOGLE],
      onServiceData: (u, b) => data.push([u, b.toString('hex')]),
      onState: (s) => states.push(s),
      deps
    })
  return { child, deps, compile, spawn, states, data, start, quitFns }
}

const spawned = (s: ReturnType<typeof setup>) => vi.waitFor(() => expect(s.spawn).toHaveBeenCalled())

describe('corebluetooth scanner', () => {
  it('maps states and forwards data with the 128-bit UUID', async () => {
    const s = setup()
    s.start()
    await spawned(s)
    s.child.push('{"type":"state","state":"poweredOn"}\n{"type":"data","uuid":"fff9","data":"0a0b"}\n')
    s.child.push('{"type":"data","uuid":"fde2","data":"ff"}\n{"type":"state","state":"poweredOff"}\n')
    s.child.push('{"type":"state","state":"unauthorized"}\n{"type":"state","state":"unsupported"}\n')
    s.child.push('{"type":"state","state":"resetting"}\n{"type":"state","state":"unknown"}\n')
    expect(s.states).toEqual(['scanning', 'off', 'denied', 'no-adapter'])
    expect(s.data).toEqual([
      [FIDO, '0a0b'],
      [GOOGLE, 'ff']
    ])
  })

  it('reassembles split lines and ignores garbage', async () => {
    const s = setup()
    s.start()
    await spawned(s)
    s.child.push('not json\n{"type":"data","uu')
    s.child.push('id":"fff9","data":"01"}\n{"type":"data","uuid":"beef","data":"01"}\n{"type":"data","uuid":"fff9","data":"zz"}\n[]\nnull\n')
    expect(s.data).toEqual([[FIDO, '01']])
    expect(s.states).toEqual([])
  })

  it('caps long lines', async () => {
    const s = setup()
    s.start()
    await spawned(s)
    s.child.push('x'.repeat(10_000))
    s.child.push('y'.repeat(10) + '\n{"type":"data","uuid":"fff9","data":"02"}\n')
    expect(s.data).toEqual([[FIDO, '02']])
    s.child.push(`{"type":"data","uuid":"fff9","data":"${'00'.repeat(3000)}"}\n`)
    expect(s.data).toHaveLength(1)
  })

  it('compiles when the binary is missing, skips when the hash matches, recompiles on change', async () => {
    const a = setup()
    a.start()
    await spawned(a)
    expect(a.compile).toHaveBeenCalledTimes(1)
    expect(await readFile(join(dir, 'cache', 'cable-scan.sha256'), 'utf8')).toMatch(/^[0-9a-f]{64}$/)

    const b = setup()
    b.deps.cacheDir = join(dir, 'cache')
    b.start()
    await spawned(b)
    expect(b.compile).not.toHaveBeenCalled()

    await writeFile(join(dir, 'a.swift'), 'changed')
    const c = setup()
    c.deps.cacheDir = join(dir, 'cache')
    c.start()
    await spawned(c)
    expect(c.compile).toHaveBeenCalledTimes(1)
  })

  it('reports an error when the compile fails', async () => {
    const s = setup({ compile: async () => Promise.reject(new Error('boom')) })
    s.start()
    await vi.waitFor(() => expect(s.states).toEqual(['error']))
    expect(s.spawn).not.toHaveBeenCalled()
  })

  it('reports an error when the child exits or fails to spawn', async () => {
    const s = setup()
    s.start()
    await spawned(s)
    s.child.emit('exit')
    expect(s.states).toEqual(['error'])
    const t = setup()
    t.start()
    await spawned(t)
    t.child.emit('error', new Error('ENOENT'))
    expect(t.states).toEqual(['error'])
  })

  it('stop closes stdin, kills the child (SIGKILL if needed) and is idempotent', async () => {
    const s = setup()
    const stop = s.start()
    await spawned(s)
    vi.useFakeTimers()
    stop()
    stop()
    expect(s.child.stdin.end).toHaveBeenCalledTimes(1)
    expect(s.child.signals).toEqual(['SIGTERM'])
    vi.advanceTimersByTime(1100)
    expect(s.child.signals).toEqual(['SIGTERM', 'SIGKILL'])
    s.child.emit('exit')
    expect(s.states).toEqual([])
  })

  it('does not SIGKILL a child that exited after SIGTERM', async () => {
    const s = setup()
    const stop = s.start()
    await spawned(s)
    vi.useFakeTimers()
    stop()
    s.child.exited = true
    vi.advanceTimersByTime(1100)
    expect(s.child.signals).toEqual(['SIGTERM'])
  })

  it('kills the child when the app quits', async () => {
    const s = setup()
    s.start()
    await spawned(s)
    s.quitFns[0]()
    expect(s.child.signals).toEqual(['SIGTERM'])
  })

  it('stop during the compile prevents the spawn', async () => {
    let release: () => void = () => {}
    const s = setup({
      compile: (_s, _p, out) =>
        new Promise<void>((resolve) => {
          release = () => void writeFile(out, 'bin').then(resolve)
        })
    })
    const stop = s.start()
    await vi.waitFor(() => expect(s.compile).toHaveBeenCalled())
    stop()
    release()
    await new Promise((r) => setTimeout(r, 50))
    expect(s.spawn).not.toHaveBeenCalled()
    expect(s.states).toEqual([])
  })
})

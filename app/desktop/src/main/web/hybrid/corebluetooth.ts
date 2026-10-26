import { execFile, spawn as nodeSpawn, type ChildProcess } from 'node:child_process'
import { createHash } from 'node:crypto'
import { mkdir, readFile, writeFile } from 'node:fs/promises'
import { join } from 'node:path'
import type { BleScanOptions, BleState } from './bluez'

// BLE scan through CoreBluetooth on macOS, for development only (never used when the app is packaged). A small Swift
// helper (native/macos/cable-scan.swift) is compiled on demand and prints the adverts as JSON lines. Electron is only
// reached through `defaultDeps`, so the rest is testable.

export type { BleScanOptions, BleState }

export interface ScanChild {
  stdout: NodeJS.ReadableStream | null
  stdin: NodeJS.WritableStream | null
  /** True once the process has exited. */
  readonly exited: boolean
  kill: (signal: NodeJS.Signals) => void
  on: (event: 'exit' | 'error', listener: (arg?: unknown) => void) => void
}

export interface ScanDeps {
  /** Source files of the helper (Swift source and Info.plist). */
  sourcePath: string
  plistPath: string
  /** Where the compiled binary and its hash are kept. */
  cacheDir: string
  compile: (source: string, plist: string, out: string) => Promise<void>
  spawn: (bin: string) => ScanChild
  /** Registers a callback for the app quitting; returns the unregister function. */
  onQuit: (fn: () => void) => () => void
}

/** A line longer than this is dropped (the helper never prints more than a few hundred bytes). */
const MAX_LINE = 4096
const COMPILE_TIMEOUT = 120_000
/** Time given to the child to exit after SIGTERM before SIGKILL. */
const KILL_WAIT = 1000
const UUID_BASE = '-0000-1000-8000-00805f9b34fb'
const WANTED = new Set(['fff9', 'fde2'])

const STATES: Record<string, BleState | undefined> = {
  poweredOn: 'scanning',
  poweredOff: 'off',
  unauthorized: 'denied',
  unsupported: 'no-adapter'
}

const sha256 = (...parts: Buffer[]): string => {
  const h = createHash('sha256')
  for (const p of parts) h.update(createHash('sha256').update(p).digest())
  return h.digest('hex')
}

/** Compiles the helper when the binary is missing or the sources changed; returns the binary path. */
export async function ensureBinary(deps: ScanDeps): Promise<string> {
  const bin = join(deps.cacheDir, 'cable-scan')
  const stamp = `${bin}.sha256`
  const [source, plist] = await Promise.all([readFile(deps.sourcePath), readFile(deps.plistPath)])
  const hash = sha256(source, plist)
  const [built, previous] = await Promise.all([
    readFile(bin).then(
      () => true,
      () => false
    ),
    readFile(stamp, 'utf8').then(
      (s) => s.trim(),
      () => ''
    )
  ])
  if (built && previous === hash) return bin
  await mkdir(deps.cacheDir, { recursive: true })
  await deps.compile(deps.sourcePath, deps.plistPath, bin)
  await writeFile(stamp, hash)
  return bin
}

/** The Electron-dependent part: paths from the app and real processes. */
async function defaultDeps(): Promise<ScanDeps> {
  const { app } = await import('electron')
  const dir = join(app.getAppPath(), 'native', 'macos')
  return {
    sourcePath: join(dir, 'cable-scan.swift'),
    plistPath: join(dir, 'Info.plist'),
    cacheDir: join(app.getPath('userData'), 'dev-bin'),
    compile: (source, plist, out) =>
      new Promise((resolve, reject) => {
        const args = ['swiftc', '-O', '-o', out, source, '-Xlinker', '-sectcreate', '-Xlinker', '__TEXT', '-Xlinker', '__info_plist', '-Xlinker', plist]
        execFile('xcrun', args, { timeout: COMPILE_TIMEOUT }, (e, _out, stderr) => {
          if (e) reject(new Error(`${e.message}\n${String(stderr).slice(0, 500)}`.trim()))
          else resolve()
        })
      }),
    spawn: (bin) => {
      const child: ChildProcess = nodeSpawn(bin, [], { stdio: ['pipe', 'pipe', 'inherit'] })
      return {
        stdout: child.stdout,
        stdin: child.stdin,
        get exited() {
          return child.exitCode !== null || child.signalCode !== null
        },
        kill: (signal) => void child.kill(signal),
        on: (event, listener) => void child.on(event, listener)
      }
    },
    onQuit: (fn) => {
      app.on('will-quit', fn)
      return () => app.removeListener('will-quit', fn)
    }
  }
}

/** Starts scanning; returns the stop function (idempotent). Failures are reported through `onState`. */
export function startBleScan(options: BleScanOptions & { deps?: ScanDeps }): () => void {
  const wanted = new Set(options.uuids.map((u) => u.toLowerCase()))
  let stopped = false
  let child: ScanChild | null = null
  let last: BleState | null = null
  let unregister = (): void => {}

  const state = (s: BleState): void => {
    if (stopped || s === last) return
    last = s
    options.onState(s)
  }

  const kill = (): void => {
    const c = child
    child = null
    unregister()
    if (!c) return
    try {
      c.stdin?.end()
    } catch {
      // The pipe may already be closed.
    }
    if (c.exited) return
    c.kill('SIGTERM')
    setTimeout(() => {
      if (!c.exited) c.kill('SIGKILL')
    }, KILL_WAIT).unref()
  }

  const line = (text: string): void => {
    let msg: { type?: unknown; state?: unknown; uuid?: unknown; data?: unknown }
    try {
      msg = JSON.parse(text)
    } catch {
      return
    }
    if (!msg || typeof msg !== 'object') return
    if (msg.type === 'state' && typeof msg.state === 'string') {
      const s = STATES[msg.state]
      if (s) state(s)
    } else if (msg.type === 'data' && typeof msg.uuid === 'string' && typeof msg.data === 'string') {
      const short = msg.uuid.toLowerCase()
      const uuid = WANTED.has(short) ? `0000${short}${UUID_BASE}` : ''
      if (!uuid || !wanted.has(uuid) || msg.data.length % 2 || !/^[0-9a-f]*$/i.test(msg.data)) return
      options.onServiceData(uuid, Buffer.from(msg.data, 'hex'))
    }
  }

  const run = async (): Promise<void> => {
    const deps = options.deps ?? (await defaultDeps())
    const bin = await ensureBinary(deps)
    if (stopped) return
    const c = deps.spawn(bin)
    child = c
    const quit = deps.onQuit(kill)
    unregister = quit
    let buffer = ''
    let dropping = false
    c.stdout?.on('data', (chunk: Buffer | string) => {
      buffer += chunk.toString()
      for (let i = buffer.indexOf('\n'); i >= 0; i = buffer.indexOf('\n')) {
        const text = buffer.slice(0, i)
        buffer = buffer.slice(i + 1)
        if (dropping) dropping = false
        else if (text.length <= MAX_LINE) line(text)
      }
      // No newline within the cap: drop this line, including what comes next until its newline.
      if (buffer.length > MAX_LINE) {
        buffer = ''
        dropping = true
      }
    })
    c.on('error', (e) => {
      if (stopped || child !== c) return
      console.error('Bluetooth: scanner failed to start:', e instanceof Error ? e.message : e)
      state('error')
    })
    c.on('exit', () => {
      if (stopped || child !== c) return
      console.error('Bluetooth: scanner exited unexpectedly')
      state('error')
    })
  }

  run().catch((e) => {
    if (stopped) return
    console.error('Bluetooth: scanner unavailable:', e instanceof Error ? e.message : e)
    state('error')
  })

  return () => {
    if (stopped) return
    stopped = true
    kill()
  }
}

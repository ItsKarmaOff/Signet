import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { AtomicWriter, writeFileSyncDurable } from './atomic'

let dir: string
beforeEach(() => {
  dir = mkdtempSync(join(tmpdir(), 'signet-atomic-'))
})
afterEach(() => {
  rmSync(dir, { recursive: true, force: true, maxRetries: 5, retryDelay: 50 })
})

describe('writeFileSyncDurable', () => {
  it('writes the content', () => {
    const f = join(dir, 'a.txt')
    writeFileSyncDurable(f, 'salut')
    expect(readFileSync(f, 'utf8')).toBe('salut')
  })
})

describe('AtomicWriter', () => {
  it('writes asynchronously without leaving a .tmp', async () => {
    const f = join(dir, 'state.json')
    const w = new AtomicWriter(f, () => '{"a":1}')
    w.request()
    await w.whenIdle()
    expect(readFileSync(f, 'utf8')).toBe('{"a":1}')
    expect(existsSync(f + '.tmp')).toBe(false)
  })

  it('does nothing if build returns null', async () => {
    const f = join(dir, 'x.json')
    const w = new AtomicWriter(f, () => null)
    w.request()
    await w.whenIdle()
    expect(existsSync(f)).toBe(false)
    expect(w.flushSync()).toBe(true)
  })

  it('batches requests during a write and writes the last value', async () => {
    const f = join(dir, 'x.json')
    let v = 'un'
    const w = new AtomicWriter(f, () => v)
    w.request()
    v = 'deux'
    w.request()
    await w.whenIdle()
    expect(readFileSync(f, 'utf8')).toBe('deux')
  })

  it('flushSync writes and returns true, then does not rewrite identical content', () => {
    const f = join(dir, 'x.json')
    const w = new AtomicWriter(f, () => 'données')
    expect(w.flushSync()).toBe(true)
    expect(readFileSync(f, 'utf8')).toBe('données')
    writeFileSync(f, 'modifié ailleurs')
    expect(w.flushSync()).toBe(true) // identical to the last written: nothing to do
    expect(readFileSync(f, 'utf8')).toBe('modifié ailleurs')
  })

  it('flushSync reports a failure and calls onFail', () => {
    const onFail = vi.fn()
    const w = new AtomicWriter(join(dir, 'absent', 'x.json'), () => 'a', onFail)
    expect(w.flushSync()).toBe(false)
    expect(onFail).toHaveBeenCalledTimes(1)
  })

  it('request reports a failure via onFail', async () => {
    const onFail = vi.fn()
    const w = new AtomicWriter(join(dir, 'absent', 'x.json'), () => 'a', onFail)
    w.request()
    await w.whenIdle()
    expect(onFail).toHaveBeenCalled()
  })

  it('cancel prevents the rename of the write in progress', async () => {
    const f = join(dir, 'x.json')
    const w = new AtomicWriter(f, () => 'a')
    w.request()
    w.cancel()
    await w.whenIdle()
    expect(existsSync(f)).toBe(false)
    expect(existsSync(f + '.tmp')).toBe(false)
  })
})

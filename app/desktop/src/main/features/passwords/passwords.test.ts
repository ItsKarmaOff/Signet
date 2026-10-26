import { mkdtempSync, readFileSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const env = vi.hoisted(() => ({ dir: '', encryption: true }))
vi.mock('electron', () => ({
  app: { getPath: () => env.dir, getPreferredSystemLanguages: () => ['en-US'] },
  webContents: { getAllWebContents: () => [] },
  BrowserWindow: class {},
  clipboard: {},
  dialog: {},
  ipcMain: {},
  systemPreferences: { canPromptTouchID: () => false },
  safeStorage: {
    isEncryptionAvailable: () => env.encryption,
    // Fake reversible encryption, enough for the test.
    encryptString: (s: string) => Buffer.from('enc:' + s),
    decryptString: (b: Buffer) => b.toString().replace(/^enc:/, '')
  }
}))

import type { Browser } from '../../browser/browser'
import { isManager, normalizeOrigin, parseCsv, Passwords } from './passwords'

describe('parseCsv', () => {
  it('simple lines and fields, mixed line endings', () => {
    expect(parseCsv('a,b,c\n1,2,3\r\n4,5,6\r7,8,9')).toEqual([
      ['a', 'b', 'c'],
      ['1', '2', '3'],
      ['4', '5', '6'],
      ['7', '8', '9']
    ])
  })

  it('quotes: commas, doubled quotes, line breaks inside a field', () => {
    expect(parseCsv('name,pw\n"a,b","x""y"\n"ligne1\nligne2",z')).toEqual([
      ['name', 'pw'],
      ['a,b', 'x"y'],
      ['ligne1\nligne2', 'z']
    ])
  })

  it('strips the BOM, ignores empty lines, keeps empty fields', () => {
    expect(parseCsv('﻿a,b\n\n,\n1,\n   ,  \n')).toEqual([
      ['a', 'b'],
      ['1', '']
    ])
  })

  it('last line without line break; empty text', () => {
    expect(parseCsv('a,b')).toEqual([['a', 'b']])
    expect(parseCsv('')).toEqual([])
  })

  it('unclosed quote: the rest becomes a single field', () => {
    expect(parseCsv('a,"b,c\nd')).toEqual([['a', 'b,c\nd']])
  })
})

describe('normalizeOrigin', () => {
  it('adds https:// and keeps only the origin', () => {
    expect(normalizeOrigin('example.com')).toBe('https://example.com')
    expect(normalizeOrigin(' https://Example.com/login?x=1 ')).toBe('https://example.com')
    expect(normalizeOrigin('http://localhost:3000/a')).toBe('http://localhost:3000')
  })
  it('rejects empty, other schemes and nonsense', () => {
    expect(normalizeOrigin('')).toBeNull()
    expect(normalizeOrigin('ftp://example.com')).toBeNull()
    expect(normalizeOrigin('javascript:alert(1)')).toBeNull()
    expect(normalizeOrigin('http://')).toBeNull()
  })
})

describe('isManager', () => {
  it('signet, off and unknown', () => {
    expect(isManager('signet')).toBe(true)
    expect(isManager('off')).toBe(true)
    expect(isManager('nope')).toBe(false)
    expect(isManager(3)).toBe(false)
  })
})

describe('Passwords.addMany / list', () => {
  let dir: string
  beforeEach(() => {
    dir = mkdtempSync(join(tmpdir(), 'signet-pw-'))
    env.dir = dir
    env.encryption = true
  })
  afterEach(() => rmSync(dir, { recursive: true, force: true }))

  const make = () => new Passwords({ store: { state: { tabs: {}, settings: {} } } } as unknown as Browser)

  it('adds, normalizes the origin, ignores invalid entries and duplicates in the list', () => {
    const p = make()
    const r = p.addMany([
      { origin: 'example.com', username: ' me ', password: 'pw1' },
      { origin: 'https://example.com/other', username: 'me', password: 'pw2' }, // same origin + username: duplicate
      { origin: 'ftp://x.com', username: 'a', password: 'p' },
      { origin: 'x.com', username: 'a', password: '' },
      { origin: 'y.com', username: 'a', password: 'x'.repeat(1001) },
      { origin: 'z.com', username: '', password: 'pw3' }
    ])
    expect(r).toEqual({ added: 2, existing: 0 })
    const { logins } = p.list()
    expect(logins.map((l) => [l.origin, l.username])).toEqual([
      ['https://example.com', 'me'],
      ['https://z.com', '']
    ])
    expect(JSON.stringify(logins)).not.toContain('pw')
  })

  it('already saved logins are counted once and kept', () => {
    const p = make()
    p.addMany([{ origin: 'a.com', username: 'u', password: 'old' }])
    const r = p.addMany([
      { origin: 'a.com', username: 'u', password: 'new' },
      { origin: 'a.com', username: 'u', password: 'new2' },
      { origin: 'b.com', username: 'u', password: 'p' }
    ])
    expect(r).toEqual({ added: 1, existing: 1 })
    p.flush()
    const saved = JSON.parse(readFileSync(join(dir, 'passwords.json'), 'utf8')).logins as { origin: string; password: string }[]
    const a = saved.find((l) => l.origin === 'https://a.com')!
    expect(Buffer.from(a.password, 'base64').toString()).toBe('enc:old')
  })

  it('without available encryption: nothing is added', () => {
    env.encryption = false
    expect(make().addMany([{ origin: 'a.com', username: 'u', password: 'p' }])).toEqual({ added: 0, existing: 0 })
  })

  it('future or zero createdAt brought back to now', () => {
    // Frozen clock: "now" is exact, without depending on the millisecond the test runs at.
    const NOW = 1_800_000_000_000
    vi.useFakeTimers({ toFake: ['Date'] })
    vi.setSystemTime(NOW)
    try {
      const p = make()
      p.addMany([
        { origin: 'a.com', username: '1', password: 'p', createdAt: 1000 },
        { origin: 'b.com', username: '1', password: 'p', createdAt: NOW + 1e9 },
        { origin: 'c.com', username: '1', password: 'p', createdAt: 0 },
        { origin: 'd.com', username: '1', password: 'p', createdAt: NOW }
      ])
      const by = Object.fromEntries(p.list().logins.map((l) => [l.origin, l.createdAt]))
      expect(by).toEqual({ 'https://a.com': 1000, 'https://b.com': NOW, 'https://c.com': NOW, 'https://d.com': NOW })
    } finally {
      vi.useRealTimers()
    }
  })

  it('flush writes passwords.json with encrypted passwords, and a reload finds them', () => {
    const p = make()
    p.addMany([{ origin: 'a.com', username: 'u', password: 'secret' }])
    p.flush()
    const raw = readFileSync(join(dir, 'passwords.json'), 'utf8')
    expect(raw).not.toContain('"secret"')
    expect(JSON.parse(raw)).toMatchObject({ version: 1, logins: [{ origin: 'https://a.com', username: 'u' }] })
    expect(make().list().logins).toHaveLength(1)
  })
})

import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { LinkStore, hashToken, newToken, tokenMatches, type StoredLink, type Vault } from './store'

const vault = (available = true): Vault => ({
  available: () => available,
  encrypt: (s) => Buffer.from(s).toString('base64'),
  decrypt: (s) => Buffer.from(s, 'base64').toString()
})

const link = (id: string): StoredLink => ({
  linkId: id,
  peer: { id: 'phone-1', name: 'iPhone', platform: 'ios' },
  enabled: true,
  certPem: 'CERT',
  keyPem: 'SECRET-KEY',
  tokenHash: hashToken('x'.repeat(43)),
  createdAt: 1,
  lastSeen: null,
  peerPaused: false
})

let dir: string
let file: string
beforeEach(() => {
  dir = mkdtempSync(join(tmpdir(), 'signet-store-'))
  file = join(dir, 'link.json')
})
afterEach(() => rmSync(dir, { recursive: true, force: true }))

describe('token', () => {
  it('32 bytes in base64url; comparison by SHA-256', () => {
    const token = newToken()
    expect(token).toMatch(/^[A-Za-z0-9_-]{43}$/)
    expect(tokenMatches(token, hashToken(token))).toBe(true)
    expect(tokenMatches(newToken(), hashToken(token))).toBe(false)
    expect(tokenMatches(token, 'zz')).toBe(false)
  })
})

describe('LinkStore', () => {
  it('first launch: random id, master switch ON, no link', () => {
    const s = new LinkStore(file, vault())
    expect(s.desktopId).toMatch(/^[0-9a-f]{32}$/)
    expect(s.general).toBe(true)
    expect(s.links).toEqual([])
  })

  it('keeps id, links and switches; the private key is encrypted on disk', () => {
    const s = new LinkStore(file, vault())
    s.add(link('0123456789abcdef'))
    s.general = false
    s.get('0123456789abcdef')!.enabled = false
    s.get('0123456789abcdef')!.peerPaused = true
    s.flush()
    const text = readFileSync(file, 'utf8')
    expect(text).not.toContain('SECRET-KEY')
    const again = new LinkStore(file, vault())
    expect(again.desktopId).toBe(s.desktopId)
    expect(again.general).toBe(false)
    expect(again.links).toHaveLength(1)
    expect(again.links[0]).toMatchObject({ keyPem: 'SECRET-KEY', enabled: false, peerPaused: true, tokenHash: link('a').tokenHash })
  })

  it('without a system keychain, the key is kept in plain text rather than lost', () => {
    const s = new LinkStore(file, vault(false))
    s.add(link('0123456789abcdef'))
    s.flush()
    expect(new LinkStore(file, vault(false)).links[0]!.keyPem).toBe('SECRET-KEY')
  })

  it('a link whose key can no longer be decrypted is ignored', () => {
    const s = new LinkStore(file, vault())
    s.add(link('0123456789abcdef'))
    s.flush()
    const broken: Vault = { ...vault(), decrypt: () => null }
    expect(new LinkStore(file, broken).links).toEqual([])
  })

  it('deleting a link removes it from the file', () => {
    const s = new LinkStore(file, vault())
    s.add(link('0123456789abcdef'))
    expect(s.remove('0123456789abcdef')).toBe(true)
    expect(s.remove('0123456789abcdef')).toBe(false)
    s.flush()
    expect(new LinkStore(file, vault()).links).toEqual([])
  })

  it('unreadable file: fresh values, but the file is not overwritten', () => {
    writeFileSync(file, '{ pas du json')
    const s = new LinkStore(file, vault())
    s.add(link('0123456789abcdef'))
    s.flush()
    expect(readFileSync(file, 'utf8')).toBe('{ pas du json')
  })
})

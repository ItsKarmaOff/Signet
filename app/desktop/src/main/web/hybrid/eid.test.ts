import { createCipheriv, createHmac, randomBytes } from 'node:crypto'
import { describe, expect, it } from 'vitest'
import { ADVERT_SIZE, decryptAdvert, eidKeys, tunnelDomain } from './eid'

/** An advert as the phone builds it. */
function advert(secret: Uint8Array, plaintext: Buffer): Buffer {
  const { aes, hmac } = eidKeys(secret)
  const c = createCipheriv('aes-256-ecb', aes, null)
  c.setAutoPadding(false)
  const block = Buffer.concat([c.update(plaintext), c.final()])
  return Buffer.concat([block, createHmac('sha256', hmac).update(block).digest().subarray(0, 4)])
}

function plain(domain: number, reserved = 0): Buffer {
  const p = Buffer.alloc(16)
  p[0] = reserved
  for (let i = 0; i < 10; i++) p[1 + i] = 0x10 + i
  p.set([0xa1, 0xa2, 0xa3], 11)
  p.writeUInt16LE(domain, 14)
  return p
}

const secret = Buffer.from('000102030405060708090a0b0c0d0e0f', 'hex')

describe('eidKeys', () => {
  it('derives 32 + 32 bytes, deterministic, different per secret', () => {
    const a = eidKeys(secret)
    expect(a.aes).toHaveLength(32)
    expect(a.hmac).toHaveLength(32)
    expect(a.aes.equals(a.hmac)).toBe(false)
    expect(eidKeys(secret).aes.equals(a.aes)).toBe(true)
    expect(eidKeys(randomBytes(16)).aes.equals(a.aes)).toBe(false)
  })
})

describe('decryptAdvert', () => {
  it('decrypts what the phone sent', () => {
    const eid = decryptAdvert(secret, advert(secret, plain(1)))!
    expect(eid.nonce.toString('hex')).toBe('101112131415161718' + '19')
    expect(eid.routingId.toString('hex')).toBe('a1a2a3')
    expect(eid.tunnelDomainId).toBe(1)
    expect(eid.tunnelDomain).toBe('cable.auth.com')
    expect(eid.plaintext.equals(plain(1))).toBe(true)
  })

  it('accepts a hashed domain', () => {
    expect(decryptAdvert(secret, advert(secret, plain(266)))!.tunnelDomain).toBe('cable.wufkweyy3uaxb.com')
  })

  it('rejects a wrong secret, an altered tag or block, a wrong size', () => {
    const ok = advert(secret, plain(0))
    expect(decryptAdvert(secret, ok)).not.toBeNull()
    expect(decryptAdvert(randomBytes(16), ok)).toBeNull()
    const tag = Buffer.from(ok)
    tag[19]! ^= 1
    expect(decryptAdvert(secret, tag)).toBeNull()
    const block = Buffer.from(ok)
    block[0]! ^= 1
    expect(decryptAdvert(secret, block)).toBeNull()
    expect(decryptAdvert(secret, ok.subarray(0, ADVERT_SIZE - 1))).toBeNull()
    expect(decryptAdvert(secret, Buffer.concat([ok, Buffer.alloc(1)]))).toBeNull()
    expect(decryptAdvert(secret, Buffer.alloc(0))).toBeNull()
  })

  it('rejects a set reserved byte or an invalid domain even with a good tag', () => {
    expect(decryptAdvert(secret, advert(secret, plain(0, 1)))).toBeNull()
    expect(decryptAdvert(secret, advert(secret, plain(2)))).toBeNull()
    expect(decryptAdvert(secret, advert(secret, plain(255)))).toBeNull()
  })
})

describe('tunnelDomain', () => {
  it('maps the assigned ids', () => {
    expect(tunnelDomain(0)).toBe('cable.ua5v.com')
    expect(tunnelDomain(1)).toBe('cable.auth.com')
  })

  it('rejects ids between the assigned ones and 256, and out of range values', () => {
    for (const id of [2, 100, 255, -1, 65536, 1.5, NaN]) expect(tunnelDomain(id)).toBeNull()
  })

  it('hashes ids from 256 up (Chromium vector: 266)', () => {
    expect(tunnelDomain(266)).toBe('cable.wufkweyy3uaxb.com')
    expect(tunnelDomain(256)).toMatch(/^cable\.[a-z2-7]+\.(com|org|net|info)$/)
    expect(tunnelDomain(65535)).toMatch(/^cable\.[a-z2-7]+\.(com|org|net|info)$/)
  })
})

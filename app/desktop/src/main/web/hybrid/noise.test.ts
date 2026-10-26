import { createCipheriv, createECDH, createHmac } from 'node:crypto'
import { describe, expect, it } from 'vitest'
import { Crypter, HandshakeInitiator, Noise, qrPsk, RESPONSE_SIZE, tunnelId, tunnelUrl } from './noise'
import { respondToHandshake } from './phone.testing'

const seq = (start: number, n = 16): Buffer => Buffer.from(Array.from({ length: n }, (_, i) => start + i))

/** HKDF-SHA256 from RFC 5869 with plain HMAC, independent of node's hkdfSync. */
function hkdf(ikm: Buffer, salt: Buffer, info: Buffer, length: number): Buffer {
  const prk = createHmac('sha256', salt.length ? salt : Buffer.alloc(32))
    .update(ikm)
    .digest()
  const out: Buffer[] = []
  let t = Buffer.alloc(0)
  for (let i = 1; Buffer.concat(out).length < length; i++) {
    t = createHmac('sha256', prk)
      .update(Buffer.concat([t, info, Buffer.of(i)]))
      .digest()
    out.push(t)
  }
  return Buffer.concat(out).subarray(0, length)
}

const identity = () => {
  const ecdh = createECDH('prime256v1')
  ecdh.generateKeys()
  return ecdh
}

describe('derivations', () => {
  const secret = seq(0)
  const eid = seq(16)

  it('tunnel id: HKDF(secret, no salt, info = uint32 LE 2), 16 bytes', () => {
    expect(tunnelId(secret).toString('hex')).toBe('de1d8150b45f03349b2b00a52e2e893e')
    expect(tunnelId(secret).equals(hkdf(secret, Buffer.alloc(0), Buffer.of(2, 0, 0, 0), 16))).toBe(true)
  })

  it('psk: HKDF(secret, salt = advert plaintext, info = uint32 LE 3), 32 bytes', () => {
    expect(qrPsk(secret, eid).toString('hex')).toBe('2a1db389ed56313efb6b4278d02b62d3ca0e95c434b1c83188d550dd62e3afeb')
    expect(qrPsk(secret, eid).equals(hkdf(secret, eid, Buffer.of(3, 0, 0, 0), 32))).toBe(true)
  })

  it('connect URL with uppercase hex (Chromium TunnelServerURLs: domain 266, zero tunnel id)', () => {
    const url = tunnelUrl('cable.wufkweyy3uaxb.com', Buffer.from([0xaa, 0xbb, 0x0c]), Buffer.alloc(16))
    expect(url).toBe(`wss://cable.wufkweyy3uaxb.com/cable/connect/AABB0C/${'00'.repeat(16)}`)
    expect(tunnelUrl('cable.ua5v.com', Buffer.from([0xab, 0xcd, 0xef]), seq(0xa0))).toBe(
      'wss://cable.ua5v.com/cable/connect/ABCDEF/A0A1A2A3A4A5A6A7A8A9AAABACADAEAF'
    )
  })
})

describe('Noise', () => {
  it('starts from the zero-padded protocol name', () => {
    const n = new Noise()
    const name = Buffer.alloc(32)
    name.write('Noise_KNpsk0_P256_AESGCM_SHA256')
    expect(n.handshakeHash.equals(name)).toBe(true)
  })

  it('two states that mix the same things agree, and refuse a tampered ciphertext', () => {
    const a = new Noise()
    const b = new Noise()
    for (const n of [a, b]) {
      n.mixHash(Buffer.of(1, 2, 3))
      n.mixKeyAndHash(seq(0, 32))
      n.mixKey(seq(9, 32))
    }
    const c = a.encryptAndHash(Buffer.from('hello'))
    const bad = Buffer.from(c)
    bad[0]! ^= 1
    expect(new Noise().decryptAndHash(c)).toBeNull()
    expect(b.decryptAndHash(bad)).toBeNull()
  })

  it('decrypts what the other side encrypted and keeps the hashes equal', () => {
    const a = new Noise()
    const b = new Noise()
    for (const n of [a, b]) n.mixKey(seq(1, 32))
    expect(b.decryptAndHash(a.encryptAndHash(Buffer.from('payload')))?.toString()).toBe('payload')
    expect(a.handshakeHash.equals(b.handshakeHash)).toBe(true)
    expect(a.trafficKeys()[0].equals(b.trafficKeys()[0])).toBe(true)
  })
})

describe('Crypter', () => {
  const pair = (): [Crypter, Crypter] => {
    const k1 = seq(1, 32)
    const k2 = seq(100, 32)
    return [new Crypter(k1, k2), new Crypter(k2, k1)]
  }

  it('pads to a multiple of 32 bytes with the last byte counting the padding', () => {
    const [a, b] = pair()
    for (const [len, total] of [
      [0, 32],
      [1, 32],
      [30, 32],
      [31, 32],
      [32, 64],
      [100, 128]
    ] as const) {
      const message = Buffer.alloc(len, 7)
      const sealed = a.encrypt(message)
      expect(sealed.length).toBe(total + 16)
      expect(b.decrypt(sealed)?.equals(message)).toBe(true)
    }
  })

  it('uses a counter per direction (the same message encrypts differently each time)', () => {
    const [a, b] = pair()
    const one = a.encrypt(Buffer.from('x'))
    const two = a.encrypt(Buffer.from('x'))
    expect(one.equals(two)).toBe(false)
    // Out of order: the counter does not match.
    expect(b.decrypt(two)).toBeNull()
    expect(b.decrypt(one)?.toString()).toBe('x')
    expect(b.decrypt(two)?.toString()).toBe('x')
    // The other direction has its own counter.
    expect(a.decrypt(b.encrypt(Buffer.from('y')))?.toString()).toBe('y')
  })

  it('builds the nonce like Chromium: 8 zero bytes then the counter, big-endian', () => {
    const [a] = pair()
    const padded = Buffer.alloc(32)
    padded.write('abc')
    padded[31] = 28
    expect(a.encrypt(Buffer.from('abc')).equals(rawSeal(seq(100, 32), 0, padded))).toBe(true)
    expect(a.encrypt(Buffer.from('abc')).equals(rawSeal(seq(100, 32), 1, padded))).toBe(true)
  })

  it('rejects tampered, truncated and badly padded messages without moving the counter', () => {
    const [a, b] = pair()
    const sealed = a.encrypt(Buffer.from('data'))
    const bad = Buffer.from(sealed)
    bad[3]! ^= 0x80
    expect(b.decrypt(bad)).toBeNull()
    expect(b.decrypt(sealed.subarray(0, 10))).toBeNull()
    expect(b.decrypt(Buffer.alloc(0))).toBeNull()
    expect(b.decrypt(sealed)?.toString()).toBe('data')
    // A valid ciphertext whose padding byte claims more than the message.
    const evil = new Crypter(seq(1, 32), seq(100, 32))
    const plain = Buffer.alloc(32, 0)
    plain[31] = 32
    expect(evil.decrypt(rawSeal(seq(100, 32), 0, plain))).toBeNull()
  })
})

function rawSeal(key: Buffer, seqNum: number, plain: Buffer): Buffer {
  const nonce = Buffer.alloc(12)
  nonce.writeUInt32BE(seqNum, 8)
  const c = createCipheriv('aes-256-gcm', key, nonce)
  return Buffer.concat([c.update(plain), c.final(), c.getAuthTag()])
}

describe('KNpsk0 handshake', () => {
  const psk = qrPsk(seq(0), seq(16))

  function run(phonePsk = psk) {
    const id = identity()
    const initiator = new HandshakeInitiator(psk, id)
    const message = initiator.buildInitialMessage()
    const answered = respondToHandshake(phonePsk, id.getPublicKey(null, 'uncompressed'), message)
    return { id, initiator, message, answered }
  }

  it('the first message is the ephemeral key (65 bytes) and a 16-byte tag', () => {
    const { message } = run()
    expect(message.length).toBe(65 + 16)
    expect(message[0]).toBe(4)
  })

  it('both sides get the same keys and handshake hash, and talk in both directions', () => {
    const { initiator, answered } = run()
    expect(answered).not.toBeNull()
    expect(answered!.response.length).toBe(RESPONSE_SIZE)
    const result = initiator.processResponse(answered!.response)
    expect(result).not.toBeNull()
    expect(result!.handshakeHash.equals(answered!.handshakeHash)).toBe(true)
    expect(answered!.crypter.decrypt(result!.crypter.encrypt(Buffer.from('to phone')))?.toString()).toBe('to phone')
    expect(result!.crypter.decrypt(answered!.crypter.encrypt(Buffer.from('to browser')))?.toString()).toBe('to browser')
  })

  it('a phone with another psk cannot answer', () => {
    expect(run(seq(1, 32)).answered).toBeNull()
  })

  it('a phone that knows another identity key cannot answer', () => {
    const id = identity()
    const message = new HandshakeInitiator(psk, id).buildInitialMessage()
    expect(respondToHandshake(psk, identity().getPublicKey(null, 'uncompressed'), message)).toBeNull()
  })

  it('refuses a tampered response', () => {
    for (const at of [0, 10, 64, 65, 80]) {
      const { initiator, answered } = run()
      const bad = Buffer.from(answered!.response)
      bad[at]! ^= 1
      expect(initiator.processResponse(bad)).toBeNull()
    }
  })

  it('refuses responses of the wrong size or with a point off the curve', () => {
    const { initiator, answered } = run()
    expect(initiator.processResponse(answered!.response.subarray(1))).toBeNull()
    expect(initiator.processResponse(Buffer.concat([answered!.response, Buffer.of(0)]))).toBeNull()
    const offCurve = Buffer.from(answered!.response)
    offCurve.fill(1, 1, 65)
    expect(initiator.processResponse(offCurve)).toBeNull()
  })

  it('refuses a replayed response from another handshake', () => {
    const first = run()
    const second = run()
    expect(first.initiator.processResponse(second.answered!.response)).toBeNull()
  })
})

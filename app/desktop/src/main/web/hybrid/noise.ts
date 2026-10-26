import { createCipheriv, createDecipheriv, createECDH, createHash, hkdfSync, type ECDH } from 'node:crypto'

// Noise handshake and transport encryption of the caBLE v2 ("hybrid") tunnel, initiator side (the browser). Pure module
// (no Electron), tested. Checked against Chromium (device/fido/cable/), base:
// https://raw.githubusercontent.com/chromium/chromium/main/device/fido/cable/
//   noise.cc (Noise_KNpsk0_P256_AESGCM_SHA256, zero-padded name, HKDF2 / 96-byte HKDF in MixKeyAndHash, nonce, traffic keys)
//   v2_handshake.cc (HandshakeInitiator::BuildInitialMessage / ProcessResponse, RespondToHandshake, Crypter, Derive)
//   v2_handshake.h (DerivedValueType, kResponseSize)
// Keys, psk and nonces are never logged.

/** DerivedValueType::kTunnelID and kPSK. */
const TUNNEL_ID_TYPE = 2
const PSK_TYPE = 3

/** Uncompressed X9.62 P-256 point (kP256X962Length). */
export const P256_POINT_SIZE = 65
/** Handshake response: ephemeral point + empty AES-GCM ciphertext (HandshakeInitiator::kResponseSize). */
export const RESPONSE_SIZE = P256_POINT_SIZE + 16
const TAG_SIZE = 16
/** Sequence numbers use 24 bits (kMaxSequence). */
const MAX_SEQUENCE = (1 << 24) - 1
/** Messages are padded to a multiple of this size. */
const PADDING_GRANULARITY = 32

/** Derive(): HKDF-SHA256(secret, salt, info = uint32 little-endian of the type). */
function derive(secret: Uint8Array, salt: Uint8Array, type: number, size: number): Buffer {
  const info = Buffer.alloc(4)
  info.writeUInt32LE(type)
  return Buffer.from(hkdfSync('sha256', secret, salt, info, size))
}

/** Tunnel id (16 bytes) from the QR secret: it names the tunnel next to the routing id. */
export function tunnelId(secret: Uint8Array): Buffer {
  return derive(secret, Buffer.alloc(0), TUNNEL_ID_TYPE, 16)
}

/** Pre-shared key (32 bytes) of a QR handshake: the QR secret salted with the decrypted advert (16 bytes). */
export function qrPsk(secret: Uint8Array, eidPlaintext: Uint8Array): Buffer {
  return derive(secret, eidPlaintext, PSK_TYPE, 32)
}

/** Connect URL of the tunnel (tunnelserver::GetConnectURL: base::HexEncode is uppercase). */
export function tunnelUrl(domain: string, routingId: Uint8Array, id: Uint8Array): string {
  return `wss://${domain}/cable/connect/${Buffer.from(routingId).toString('hex').toUpperCase()}/${Buffer.from(id).toString('hex').toUpperCase()}`
}

/** HKDF-SHA256 with the chaining key as salt and no info (the Noise HKDF of Chromium's noise.cc). */
function hkdf(ck: Uint8Array, ikm: Uint8Array, size: number): Buffer {
  return Buffer.from(hkdfSync('sha256', ikm, ck, Buffer.alloc(0), size))
}

const sha256 = (...parts: Uint8Array[]): Buffer => {
  const h = createHash('sha256')
  for (const p of parts) h.update(p)
  return h.digest()
}

/** Noise SymmetricState for the caBLE handshake (Chromium's device::Noise). */
export class Noise {
  private ck: Buffer = Buffer.alloc(32)
  private h: Buffer = Buffer.alloc(32)
  private key: Buffer = Buffer.alloc(32)
  private counter = 0

  /** The protocol name is shorter than 32 bytes: zero-padded, not hashed. */
  constructor(name = 'Noise_KNpsk0_P256_AESGCM_SHA256') {
    this.ck.write(name, 'ascii')
    this.h = Buffer.from(this.ck)
  }

  get handshakeHash(): Buffer {
    return Buffer.from(this.h)
  }

  mixHash(data: Uint8Array): void {
    this.h = sha256(this.h, data)
  }

  private setKey(key: Uint8Array): void {
    this.key = Buffer.from(key)
    this.counter = 0
  }

  mixKey(ikm: Uint8Array): void {
    const out = hkdf(this.ck, ikm, 64)
    this.ck = out.subarray(0, 32)
    this.setKey(out.subarray(32, 64))
  }

  mixKeyAndHash(ikm: Uint8Array): void {
    const out = hkdf(this.ck, ikm, 96)
    this.ck = out.subarray(0, 32)
    this.mixHash(out.subarray(32, 64))
    this.setKey(out.subarray(64, 96))
  }

  /** 12-byte nonce: the counter as a big-endian uint32 in the first 4 bytes. */
  private nonce(): Buffer {
    const n = Buffer.alloc(12)
    n.writeUInt32BE(this.counter++, 0)
    return n
  }

  encryptAndHash(plaintext: Uint8Array): Buffer {
    const c = createCipheriv('aes-256-gcm', this.key, this.nonce())
    c.setAAD(this.h)
    const out = Buffer.concat([c.update(plaintext), c.final(), c.getAuthTag()])
    this.mixHash(out)
    return out
  }

  /** The plaintext, or null if the tag does not match (the hash is only updated on success). */
  decryptAndHash(ciphertext: Uint8Array): Buffer | null {
    const nonce = this.nonce()
    if (ciphertext.length < TAG_SIZE) return null
    const buf = Buffer.from(ciphertext)
    try {
      const d = createDecipheriv('aes-256-gcm', this.key, nonce)
      d.setAAD(this.h)
      d.setAuthTag(buf.subarray(buf.length - TAG_SIZE))
      const out = Buffer.concat([d.update(buf.subarray(0, buf.length - TAG_SIZE)), d.final()])
      this.mixHash(buf)
      return out
    } catch {
      return null
    }
  }

  /** Keys for the two directions: the first is the initiator's write key (and the responder's read key). */
  trafficKeys(): [Buffer, Buffer] {
    const out = hkdf(this.ck, Buffer.alloc(0), 64)
    return [out.subarray(0, 32), out.subarray(32, 64)]
  }
}

/**
 * Transport encryption after the handshake: AES-256-GCM without additional data, one counter per direction (big-endian in
 * the last 4 bytes of the nonce), messages padded to a multiple of 32 bytes (zeros, the last byte counts them).
 */
export class Crypter {
  private readSeq = 0
  private writeSeq = 0

  constructor(
    private readKey: Buffer,
    private writeKey: Buffer
  ) {}

  private static nonce(seq: number): Buffer {
    const n = Buffer.alloc(12)
    n.writeUInt32BE(seq, 8)
    return n
  }

  /** Throws when the sequence space is exhausted. */
  encrypt(message: Uint8Array): Buffer {
    if (this.writeSeq > MAX_SEQUENCE) throw new Error('Crypter: sequence number exhausted')
    const padded = Math.ceil((message.length + 1) / PADDING_GRANULARITY) * PADDING_GRANULARITY
    const plain = Buffer.alloc(padded)
    plain.set(message)
    plain[padded - 1] = padded - message.length - 1
    const c = createCipheriv('aes-256-gcm', this.writeKey, Crypter.nonce(this.writeSeq++))
    return Buffer.concat([c.update(plain), c.final(), c.getAuthTag()])
  }

  /** The unpadded plaintext, or null (bad tag, bad padding, empty); the read counter only moves on success. */
  decrypt(ciphertext: Uint8Array): Buffer | null {
    if (this.readSeq > MAX_SEQUENCE || ciphertext.length < TAG_SIZE) return null
    const buf = Buffer.from(ciphertext)
    let plain: Buffer
    try {
      const d = createDecipheriv('aes-256-gcm', this.readKey, Crypter.nonce(this.readSeq))
      d.setAuthTag(buf.subarray(buf.length - TAG_SIZE))
      plain = Buffer.concat([d.update(buf.subarray(0, buf.length - TAG_SIZE)), d.final()])
    } catch {
      return null
    }
    this.readSeq++
    if (plain.length === 0) return null
    const padding = plain[plain.length - 1]!
    if (padding + 1 > plain.length) return null
    return plain.subarray(0, plain.length - padding - 1)
  }
}

export interface HandshakeResult {
  crypter: Crypter
  handshakeHash: Buffer
}

/** Uncompressed point of the identity key (what both sides mix into the hash). */
const identityPoint = (identity: ECDH): Buffer => identity.getPublicKey(null, 'uncompressed')

/**
 * Initiator of the QR handshake, Noise KNpsk0: the phone knows our identity key from the QR code, both share the psk.
 * Message 1: ephemeral key (65 bytes) + AEAD of nothing. Message 2 (81 bytes): the phone's ephemeral key + AEAD of nothing.
 */
export class HandshakeInitiator {
  private noise = new Noise()
  private ephemeral = createECDH('prime256v1')

  constructor(
    private psk: Uint8Array,
    private identity: ECDH
  ) {}

  buildInitialMessage(): Buffer {
    this.noise.mixHash(Buffer.of(1)) // prologue: 1 for a QR (KN) handshake
    this.noise.mixHash(identityPoint(this.identity))
    this.noise.mixKeyAndHash(this.psk)
    this.ephemeral.generateKeys()
    const e = this.ephemeral.getPublicKey(null, 'uncompressed')
    this.noise.mixHash(e)
    this.noise.mixKey(e)
    return Buffer.concat([e, this.noise.encryptAndHash(Buffer.alloc(0))])
  }

  /** null for anything that is not a valid response (size, point, tag, non-empty payload). */
  processResponse(response: Uint8Array): HandshakeResult | null {
    if (response.length !== RESPONSE_SIZE) return null
    const peer = Buffer.from(response.subarray(0, P256_POINT_SIZE))
    const ciphertext = response.subarray(P256_POINT_SIZE)
    let ee: Buffer
    let se: Buffer
    try {
      ee = this.ephemeral.computeSecret(peer)
      se = this.identity.computeSecret(peer)
    } catch {
      return null
    }
    this.noise.mixHash(peer)
    this.noise.mixKey(peer)
    this.noise.mixKey(ee)
    this.noise.mixKey(se)
    const payload = this.noise.decryptAndHash(ciphertext)
    if (!payload || payload.length !== 0) return null
    const [write, read] = this.noise.trafficKeys()
    return { crypter: new Crypter(read, write), handshakeHash: this.noise.handshakeHash }
  }
}

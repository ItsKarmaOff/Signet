import { randomBytes, createECDH } from 'node:crypto'
import { encodeCbor } from './cbor'

// Contents of the caBLE v2 QR code ("hybrid" transport), as Chromium's device/fido/cable/v2_handshake.cc builds it.
// Pure module (no Electron), tested.

/** Number of tunnel server domains this client knows (cable.ua5v.com and cable.auth.com). */
const KNOWN_DOMAINS = 2
const SECRET_SIZE = 16

export interface QrKeys {
  /** Identity key pair of the session; the private key never leaves the main process. */
  ecdh: ReturnType<typeof createECDH>
  /** Compressed public key (33 bytes). */
  publicKey: Buffer
  secret: Buffer
}

/** Fresh identity key (P-256) and QR secret. */
export function generateQrKeys(): QrKeys {
  const ecdh = createECDH('prime256v1')
  ecdh.generateKeys()
  return { ecdh, publicKey: ecdh.getPublicKey(null, 'compressed'), secret: randomBytes(SECRET_SIZE) }
}

/** The CBOR map carried by the code. Key 4: supports linking (state-assisted transactions), not supported. Key 5: request type hint. */
export function qrCbor(keys: Pick<QrKeys, 'publicKey' | 'secret'>, type: 'get' | 'create', now: number = Date.now()): Uint8Array {
  return encodeCbor(
    new Map<number, Uint8Array | number | boolean | string>([
      [0, new Uint8Array(keys.publicKey)],
      [1, new Uint8Array(keys.secret)],
      [2, KNOWN_DOMAINS],
      [3, Math.floor(now / 1000)],
      [4, false],
      [5, type === 'get' ? 'ga' : 'mc']
    ])
  )
}

/** Digits per partial chunk of 0 to 6 bytes. */
const PARTIAL_DIGITS = [0, 3, 5, 8, 10, 13, 15]

/** 7-byte chunks read as little-endian integers, 17 decimal digits each; the last partial chunk gets fewer digits. */
export function encodeQrDigits(bytes: Uint8Array): string {
  let out = ''
  for (let i = 0; i < bytes.length; i += 7) {
    const chunk = bytes.subarray(i, i + 7)
    let n = 0n
    for (let j = chunk.length - 1; j >= 0; j--) n = (n << 8n) | BigInt(chunk[j]!)
    out += n.toString().padStart(chunk.length === 7 ? 17 : PARTIAL_DIGITS[chunk.length]!, '0')
  }
  return out
}

/** Text to encode in the QR code. */
export function qrText(keys: Pick<QrKeys, 'publicKey' | 'secret'>, type: 'get' | 'create', now?: number): string {
  return `FIDO:/${encodeQrDigits(qrCbor(keys, type, now))}`
}

import { createDecipheriv, createHash, createHmac, hkdfSync, timingSafeEqual } from 'node:crypto'

// Phone's BLE advert of the caBLE v2 ("hybrid") transport: decryption with the QR secret. Pure module (no Electron), tested.
// Checked against Chromium (device/fido/cable/): v2_handshake.cc (eid::Decrypt, tunnelserver::DecodeDomain, qr::Encode),
// v2_handshake.h (DerivedValueType), v2_discovery.cc (KeysFromQRGeneratorKey, advert size), fido_ble_uuids.cc, and the
// TunnelServerURLs test (domain 266). Base: https://raw.githubusercontent.com/chromium/chromium/main/device/fido/cable/

/** The two 16-bit service UUIDs the phone may use: FIDO (0xFFF9) and Google (0xFDE2), in 128-bit form. */
export const CABLE_UUIDS = ['0000fff9-0000-1000-8000-00805f9b34fb', '0000fde2-0000-1000-8000-00805f9b34fb'] as const

/** 16-byte encrypted block + 4-byte tag. */
export const ADVERT_SIZE = 20
const BLOCK = 16
const TAG = 4
/** DerivedValueType::kEIDKey. */
const EID_KEY_TYPE = 1

/** Assigned tunnel server domains: ids 0 and 1. */
const ASSIGNED_DOMAINS = ['cable.ua5v.com', 'cable.auth.com']
const DOMAIN_PREFIX = 'caBLEv2 tunnel server domain'
const BASE32 = 'abcdefghijklmnopqrstuvwxyz234567'
const TLDS = ['com', 'org', 'net', 'info']

export interface Eid {
  /** The 16 decrypted bytes. */
  plaintext: Buffer
  /** Connection nonce (10 bytes). */
  nonce: Buffer
  /** Routing id (3 bytes), sent to the tunnel server. */
  routingId: Buffer
  tunnelDomainId: number
  tunnelDomain: string
}

/** AES-256 and HMAC-SHA256 keys protecting the advert: HKDF-SHA256(secret, no salt, info = uint32 LE 1), 64 bytes. */
export function eidKeys(secret: Uint8Array): { aes: Buffer; hmac: Buffer } {
  const info = Buffer.alloc(4)
  info.writeUInt32LE(EID_KEY_TYPE)
  const okm = Buffer.from(hkdfSync('sha256', secret, Buffer.alloc(0), info, 64))
  return { aes: okm.subarray(0, 32), hmac: okm.subarray(32, 64) }
}

/** Domain of a tunnel server id: 0 and 1 are assigned, 256 and above are hashed into a generated name, others are invalid. */
export function tunnelDomain(id: number): string | null {
  if (!Number.isInteger(id) || id < 0 || id > 0xffff) return null
  if (id < ASSIGNED_DOMAINS.length) return ASSIGNED_DOMAINS[id]!
  if (id < 256) return null
  const input = Buffer.alloc(DOMAIN_PREFIX.length + 3)
  input.write(DOMAIN_PREFIX, 0, 'ascii')
  input.writeUInt16LE(id, DOMAIN_PREFIX.length)
  // First 8 bytes of the digest as a little-endian integer: 2 bits pick the TLD, the rest is written in base 32.
  let v = createHash('sha256').update(input).digest().readBigUInt64LE(0)
  const tld = TLDS[Number(v & 3n)]!
  v >>= 2n
  let label = ''
  while (v !== 0n) {
    label += BASE32[Number(v & 31n)]
    v >>= 5n
  }
  return `cable.${label}.${tld}`
}

/** Decrypts the service data of the phone's advert; null if it is not ours (size, tag, reserved byte or domain). */
export function decryptAdvert(secret: Uint8Array, serviceData: Uint8Array): Eid | null {
  if (serviceData.length !== ADVERT_SIZE) return null
  const advert = Buffer.from(serviceData)
  const block = advert.subarray(0, BLOCK)
  const { aes, hmac } = eidKeys(secret)
  const tag = createHmac('sha256', hmac).update(block).digest().subarray(0, TAG)
  if (!timingSafeEqual(tag, advert.subarray(BLOCK))) return null
  const d = createDecipheriv('aes-256-ecb', aes, null)
  d.setAutoPadding(false)
  const plaintext = Buffer.concat([d.update(block), d.final()])
  if (plaintext[0] !== 0) return null
  const tunnelDomainId = plaintext.readUInt16LE(14)
  const domain = tunnelDomain(tunnelDomainId)
  if (!domain) return null
  return {
    plaintext,
    nonce: plaintext.subarray(1, 11),
    routingId: plaintext.subarray(11, 14),
    tunnelDomainId,
    tunnelDomain: domain
  }
}

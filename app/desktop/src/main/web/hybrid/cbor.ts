// Minimal CBOR encoder (RFC 8949, canonical form as CTAP2 uses it): shortest-form lengths, map keys sorted. Pure module,
// tested. No decoder yet: the tunnel step adds one.

export type CborValue = number | bigint | boolean | null | string | Uint8Array | CborValue[] | ReadonlyMap<CborValue, CborValue>

function head(major: number, value: bigint): Uint8Array {
  const m = major << 5
  if (value < 24n) return Uint8Array.of(m | Number(value))
  if (value < 0x100n) return Uint8Array.of(m | 24, Number(value))
  if (value < 0x10000n) return Uint8Array.of(m | 25, Number(value >> 8n), Number(value & 0xffn))
  const size = value < 0x100000000n ? 4 : 8
  const out = new Uint8Array(1 + size)
  out[0] = m | (size === 4 ? 26 : 27)
  for (let i = 0, v = value; i < size; i++, v >>= 8n) out[size - i] = Number(v & 0xffn)
  return out
}

function concat(parts: Uint8Array[]): Uint8Array {
  const out = new Uint8Array(parts.reduce((n, p) => n + p.length, 0))
  let at = 0
  for (const p of parts) {
    out.set(p, at)
    at += p.length
  }
  return out
}

/** CTAP2 canonical order: shorter encoded keys first, then bytewise. */
function compareKeys(a: Uint8Array, b: Uint8Array): number {
  if (a.length !== b.length) return a.length - b.length
  for (let i = 0; i < a.length; i++) if (a[i] !== b[i]) return a[i]! - b[i]!
  return 0
}

export function encodeCbor(value: CborValue): Uint8Array {
  if (value === null) return Uint8Array.of(0xf6)
  if (value === true) return Uint8Array.of(0xf5)
  if (value === false) return Uint8Array.of(0xf4)
  if (typeof value === 'number') {
    if (!Number.isSafeInteger(value)) throw new RangeError('CBOR: integer expected')
    return encodeCbor(BigInt(value))
  }
  if (typeof value === 'bigint') {
    if (value >= 0n) {
      if (value > 0xffffffffffffffffn) throw new RangeError('CBOR: integer out of range')
      return head(0, value)
    }
    if (value < -0x10000000000000000n) throw new RangeError('CBOR: integer out of range')
    return head(1, -1n - value)
  }
  if (typeof value === 'string') {
    const bytes = new TextEncoder().encode(value)
    return concat([head(3, BigInt(bytes.length)), bytes])
  }
  if (value instanceof Uint8Array) return concat([head(2, BigInt(value.length)), value])
  if (Array.isArray(value)) return concat([head(4, BigInt(value.length)), ...value.map(encodeCbor)])
  if (value instanceof Map) {
    const entries = [...value].map(([k, v]) => [encodeCbor(k), encodeCbor(v)] as const).sort(([a], [b]) => compareKeys(a, b))
    for (let i = 1; i < entries.length; i++) if (compareKeys(entries[i - 1]![0], entries[i]![0]) === 0) throw new Error('CBOR: duplicate map key')
    return concat([head(5, BigInt(entries.length)), ...entries.flat()])
  }
  throw new TypeError('CBOR: unsupported value')
}

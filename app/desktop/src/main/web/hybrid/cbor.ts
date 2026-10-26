// Minimal CBOR (RFC 8949) as CTAP2 uses it. The encoder writes the canonical form (shortest-form lengths, map keys sorted).
// The decoder is strict like Chromium's components/cbor reader: definite lengths only, shortest-form integers and lengths,
// no tags, no floats, no duplicate map keys, no trailing bytes. Pure module, tested.

export type CborValue = number | bigint | boolean | null | undefined | string | Uint8Array | CborValue[] | ReadonlyMap<CborValue, CborValue>

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
  if (value === undefined) return Uint8Array.of(0xf7)
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

export class CborError extends Error {}

/** Nesting limit (arrays and maps). */
const MAX_DEPTH = 16

const fail = (message: string): never => {
  throw new CborError(message)
}

/** Integers beyond the safe range stay bigint, the others are numbers (so that equal keys are equal Map keys). */
function normalizeInt(v: bigint): number | bigint {
  return v >= BigInt(Number.MIN_SAFE_INTEGER) && v <= BigInt(Number.MAX_SAFE_INTEGER) ? Number(v) : v
}

class Reader {
  at = 0
  constructor(readonly bytes: Uint8Array) {}

  get left(): number {
    return this.bytes.length - this.at
  }

  take(n: number): Uint8Array {
    if (n > this.left) fail('CBOR: truncated')
    const out = this.bytes.subarray(this.at, this.at + n)
    this.at += n
    return out
  }

  /** The argument of an initial byte; rejects indefinite lengths and non-shortest forms. */
  argument(info: number): bigint {
    if (info < 24) return BigInt(info)
    if (info > 27) return fail('CBOR: indefinite length or reserved value')
    const size = 1 << (info - 24)
    const b = this.take(size)
    let v = 0n
    for (const x of b) v = (v << 8n) | BigInt(x)
    const min = info === 24 ? 24n : info === 25 ? 0x100n : info === 26 ? 0x10000n : 0x100000000n
    if (v < min) fail('CBOR: not the shortest form')
    return v
  }

  /** A length that must fit in what is left (`unit`: minimum bytes of each element). */
  length(info: number, unit: number): number {
    const n = this.argument(info)
    if (n * BigInt(unit) > BigInt(this.left)) fail('CBOR: length beyond the input')
    return Number(n)
  }

  value(depth: number): CborValue {
    const initial = this.take(1)[0]!
    const major = initial >> 5
    const info = initial & 31
    switch (major) {
      case 0:
        return normalizeInt(this.argument(info))
      case 1:
        return normalizeInt(-1n - this.argument(info))
      case 2:
        return new Uint8Array(this.take(this.length(info, 1)))
      case 3: {
        const bytes = this.take(this.length(info, 1))
        try {
          return new TextDecoder('utf-8', { fatal: true }).decode(bytes)
        } catch {
          return fail('CBOR: invalid UTF-8')
        }
      }
      case 4: {
        if (depth >= MAX_DEPTH) fail('CBOR: too deeply nested')
        const n = this.length(info, 1)
        const out: CborValue[] = []
        for (let i = 0; i < n; i++) out.push(this.value(depth + 1))
        return out
      }
      case 5: {
        if (depth >= MAX_DEPTH) fail('CBOR: too deeply nested')
        const n = this.length(info, 2)
        const out = new Map<CborValue, CborValue>()
        for (let i = 0; i < n; i++) {
          const key = this.value(depth + 1)
          if (typeof key !== 'number' && typeof key !== 'bigint' && typeof key !== 'string') fail('CBOR: unsupported map key')
          if (out.has(key)) fail('CBOR: duplicate map key')
          out.set(key, this.value(depth + 1))
        }
        return out
      }
      case 6:
        return fail('CBOR: tags are not supported')
      default:
        if (info === 20) return false
        if (info === 21) return true
        if (info === 22) return null
        if (info === 23) return undefined
        return fail('CBOR: unsupported simple value or float')
    }
  }
}

/** Decodes the first value of `input` and says how many bytes it took (COSE keys sit in front of other data). */
export function decodeCborPrefix(input: Uint8Array): { value: CborValue; length: number } {
  const r = new Reader(input)
  const value = r.value(0)
  return { value, length: r.at }
}

/** Decodes exactly one value; throws CborError on malformed input or trailing bytes. */
export function decodeCbor(input: Uint8Array): CborValue {
  const { value, length } = decodeCborPrefix(input)
  if (length !== input.length) fail('CBOR: trailing bytes')
  return value
}

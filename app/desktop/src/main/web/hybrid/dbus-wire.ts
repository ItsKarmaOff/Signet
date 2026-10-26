// D-Bus wire format (https://dbus.freedesktop.org/doc/dbus-specification.html#message-protocol-marshaling): marshalling of
// values driven by signature strings, and of whole messages. Pure module (no Electron, no socket), tested.
// Values: y n q i u d number, x t bigint, b boolean, s o g string, ay Buffer, a{kv} Map, other arrays Array, struct Array,
// variant `Variant`. No unix fds.

/** Largest message accepted or sent (the spec allows 128 MiB). */
export const MAX_MESSAGE = 16 * 1024 * 1024
const MAX_ARRAY = 64 * 1024 * 1024
const MAX_DEPTH = 32

export class DBusWireError extends Error {}

export class Variant {
  constructor(
    public sig: string,
    public value: unknown
  ) {}
}

export const MessageType = { MethodCall: 1, MethodReturn: 2, Error: 3, Signal: 4 } as const
export const Flags = { NoReplyExpected: 1, NoAutoStart: 2 } as const

export interface Message {
  type: number
  flags: number
  serial: number
  path?: string
  interface?: string
  member?: string
  errorName?: string
  replySerial?: number
  destination?: string
  sender?: string
  signature?: string
  body: unknown[]
}

// ───────────────────────── Signatures ─────────────────────────

interface Type {
  code: string
  /** Array element. */
  elem?: Type
  /** Struct members, or key and value of a dict entry. */
  fields?: Type[]
}

const BASIC = 'ybnqiuxtdsog'
const ALIGN: Record<string, number> = { y: 1, b: 4, n: 2, q: 2, i: 4, u: 4, x: 8, t: 8, d: 8, s: 4, o: 4, g: 1, a: 4, '(': 8, '{': 8, v: 1 }

function parseOne(sig: string, pos: { i: number }, depth: number): Type {
  if (depth > MAX_DEPTH) throw new DBusWireError('signature too deep')
  const c = sig[pos.i++]
  if (c === undefined) throw new DBusWireError('truncated signature')
  if (BASIC.includes(c) || c === 'v') return { code: c }
  if (c === 'a') {
    const elem = parseOne(sig, pos, depth + 1)
    return { code: 'a', elem }
  }
  if (c === '(') {
    const fields: Type[] = []
    while (sig[pos.i] !== ')') {
      if (pos.i >= sig.length) throw new DBusWireError('unclosed struct')
      fields.push(parseOne(sig, pos, depth + 1))
    }
    pos.i++
    if (!fields.length) throw new DBusWireError('empty struct')
    return { code: '(', fields }
  }
  if (c === '{') {
    // Only as the element of an array, with a basic key.
    const key = parseOne(sig, pos, depth + 1)
    if (!BASIC.includes(key.code)) throw new DBusWireError('bad dict key')
    const value = parseOne(sig, pos, depth + 1)
    if (sig[pos.i++] !== '}') throw new DBusWireError('bad dict entry')
    return { code: '{', fields: [key, value] }
  }
  throw new DBusWireError(`bad signature character ${JSON.stringify(c)}`)
}

function parseSignature(sig: string): Type[] {
  if (sig.length > 255) throw new DBusWireError('signature too long')
  const pos = { i: 0 }
  const out: Type[] = []
  while (pos.i < sig.length) out.push(parseOne(sig, pos, 0))
  // A dict entry is only valid directly inside an array.
  const check = (t: Type, inArray: boolean): void => {
    if (t.code === '{' && !inArray) throw new DBusWireError('dict entry outside an array')
    if (t.elem) check(t.elem, true)
    t.fields?.forEach((f) => check(f, false))
  }
  out.forEach((t) => check(t, false))
  return out
}

// ───────────────────────── Writer ─────────────────────────

class Writer {
  private buf = Buffer.alloc(256)
  private len = 0
  constructor(private le: boolean) {}

  get length(): number {
    return this.len
  }

  private push(b: Uint8Array): void {
    const need = this.len + b.length
    if (need > MAX_MESSAGE) throw new DBusWireError('message too large')
    if (need > this.buf.length) {
      const grown = Buffer.alloc(Math.max(need, this.buf.length * 2))
      this.buf.copy(grown, 0, 0, this.len)
      this.buf = grown
    }
    this.buf.set(b, this.len)
    this.len = need
  }

  align(n: number): void {
    const pad = (n - (this.len % n)) % n
    if (pad) this.push(Buffer.alloc(pad))
  }

  private num(size: number, fn: (b: Buffer) => void): void {
    this.align(size)
    const b = Buffer.alloc(size)
    fn(b)
    this.push(b)
  }

  u32(v: number): void {
    this.num(4, (b) => (this.le ? b.writeUInt32LE(v) : b.writeUInt32BE(v)))
  }

  /** Patches a u32 written earlier (array length). */
  patchU32(at: number, v: number): void {
    if (this.le) this.buf.writeUInt32LE(v, at)
    else this.buf.writeUInt32BE(v, at)
  }

  write(t: Type, v: unknown, depth = 0): void {
    if (depth > MAX_DEPTH) throw new DBusWireError('value too deep')
    const bad = (): never => {
      throw new DBusWireError(`value does not match type ${t.code}`)
    }
    const int = (min: number, max: number): number => {
      if (typeof v !== 'number' || !Number.isInteger(v) || v < min || v > max) bad()
      return v as number
    }
    const big = (signed: boolean): bigint => {
      const n = typeof v === 'number' && Number.isInteger(v) ? BigInt(v) : v
      if (typeof n !== 'bigint') return bad()
      const bits = 64n
      if (signed ? n < -(1n << (bits - 1n)) || n >= 1n << (bits - 1n) : n < 0n || n >= 1n << bits) bad()
      return n
    }
    switch (t.code) {
      case 'y':
        return this.push(Buffer.from([int(0, 255)]))
      case 'b':
        if (typeof v !== 'boolean') bad()
        return this.u32(v ? 1 : 0)
      case 'n':
        return this.num(2, (b) => (this.le ? b.writeInt16LE(int(-32768, 32767)) : b.writeInt16BE(int(-32768, 32767))))
      case 'q':
        return this.num(2, (b) => (this.le ? b.writeUInt16LE(int(0, 65535)) : b.writeUInt16BE(int(0, 65535))))
      case 'i':
        return this.num(4, (b) => (this.le ? b.writeInt32LE(int(-(2 ** 31), 2 ** 31 - 1)) : b.writeInt32BE(int(-(2 ** 31), 2 ** 31 - 1))))
      case 'u':
        return this.u32(int(0, 2 ** 32 - 1))
      case 'x':
        return this.num(8, (b) => (this.le ? b.writeBigInt64LE(big(true)) : b.writeBigInt64BE(big(true))))
      case 't':
        return this.num(8, (b) => (this.le ? b.writeBigUInt64LE(big(false)) : b.writeBigUInt64BE(big(false))))
      case 'd':
        if (typeof v !== 'number') bad()
        return this.num(8, (b) => (this.le ? b.writeDoubleLE(v as number) : b.writeDoubleBE(v as number)))
      case 's':
      case 'o': {
        if (typeof v !== 'string' || v.includes('\0')) bad()
        const b = Buffer.from(v as string, 'utf8')
        this.u32(b.length)
        this.push(b)
        return this.push(Buffer.alloc(1))
      }
      case 'g': {
        if (typeof v !== 'string' || v.includes('\0')) bad()
        const b = Buffer.from(v as string, 'ascii')
        if (b.length > 255) bad()
        this.push(Buffer.from([b.length]))
        this.push(b)
        return this.push(Buffer.alloc(1))
      }
      case 'v': {
        if (!(v instanceof Variant)) return bad()
        const types = parseSignature(v.sig)
        if (types.length !== 1) bad()
        this.write({ code: 'g' }, v.sig, depth + 1)
        return this.write(types[0]!, v.value, depth + 1)
      }
      case '(': {
        if (!Array.isArray(v) || v.length !== t.fields!.length) return bad()
        this.align(8)
        t.fields!.forEach((f, i) => this.write(f, v[i], depth + 1))
        return
      }
      case '{': {
        if (!Array.isArray(v) || v.length !== 2) return bad()
        this.align(8)
        this.write(t.fields![0]!, v[0], depth + 1)
        return this.write(t.fields![1]!, v[1], depth + 1)
      }
      case 'a': {
        const elem = t.elem!
        this.u32(0)
        const lenAt = this.len - 4
        this.align(ALIGN[elem.code]!)
        const start = this.len
        if (elem.code === 'y' && v instanceof Uint8Array) {
          this.push(v)
        } else if (elem.code === '{' && v instanceof Map) {
          for (const e of v) this.write(elem, e, depth + 1)
        } else if (Array.isArray(v)) {
          for (const e of v) this.write(elem, e, depth + 1)
        } else bad()
        const n = this.len - start
        if (n > MAX_ARRAY) bad()
        return this.patchU32(lenAt, n)
      }
    }
    bad()
  }

  done(): Buffer {
    return Buffer.from(this.buf.subarray(0, this.len))
  }
}

/** Marshals `values` according to `signature` (as the body of a message: alignment starts at 0). */
export function marshal(signature: string, values: unknown[], littleEndian = true): Buffer {
  const types = parseSignature(signature)
  if (types.length !== values.length) throw new DBusWireError('values do not match the signature')
  const w = new Writer(littleEndian)
  types.forEach((t, i) => w.write(t, values[i]))
  return w.done()
}

// ───────────────────────── Reader ─────────────────────────

class Reader {
  constructor(
    private buf: Buffer,
    public pos: number,
    private end: number,
    private le: boolean
  ) {}

  align(n: number): void {
    const next = this.pos + ((n - (this.pos % n)) % n)
    // Padding must lie inside the data and be zero.
    if (next > this.end) throw new DBusWireError('truncated padding')
    for (let i = this.pos; i < next; i++) if (this.buf[i] !== 0) throw new DBusWireError('non-zero padding')
    this.pos = next
  }

  private take(n: number): number {
    if (n < 0 || this.pos + n > this.end) throw new DBusWireError('truncated data')
    const at = this.pos
    this.pos += n
    return at
  }

  u32(): number {
    this.align(4)
    const at = this.take(4)
    return this.le ? this.buf.readUInt32LE(at) : this.buf.readUInt32BE(at)
  }

  read(t: Type, depth = 0): unknown {
    if (depth > MAX_DEPTH) throw new DBusWireError('value too deep')
    const b = this.buf
    const num = <T>(size: number, le: (at: number) => T, be: (at: number) => T): T => {
      this.align(size)
      const at = this.take(size)
      return this.le ? le(at) : be(at)
    }
    switch (t.code) {
      case 'y':
        return b[this.take(1)]!
      case 'b': {
        const v = this.u32()
        if (v > 1) throw new DBusWireError('bad boolean')
        return v === 1
      }
      case 'n':
        return num(
          2,
          (a) => b.readInt16LE(a),
          (a) => b.readInt16BE(a)
        )
      case 'q':
        return num(
          2,
          (a) => b.readUInt16LE(a),
          (a) => b.readUInt16BE(a)
        )
      case 'i':
        return num(
          4,
          (a) => b.readInt32LE(a),
          (a) => b.readInt32BE(a)
        )
      case 'u':
        return this.u32()
      case 'x':
        return num(
          8,
          (a) => b.readBigInt64LE(a),
          (a) => b.readBigInt64BE(a)
        )
      case 't':
        return num(
          8,
          (a) => b.readBigUInt64LE(a),
          (a) => b.readBigUInt64BE(a)
        )
      case 'd':
        return num(
          8,
          (a) => b.readDoubleLE(a),
          (a) => b.readDoubleBE(a)
        )
      case 's':
      case 'o': {
        const n = this.u32()
        const at = this.take(n)
        if (b[this.take(1)] !== 0) throw new DBusWireError('string not terminated')
        return b.toString('utf8', at, at + n)
      }
      case 'g': {
        const n = b[this.take(1)]!
        const at = this.take(n)
        if (b[this.take(1)] !== 0) throw new DBusWireError('signature not terminated')
        return b.toString('ascii', at, at + n)
      }
      case 'v': {
        const sig = this.read({ code: 'g' }) as string
        const types = parseSignature(sig)
        if (types.length !== 1) throw new DBusWireError('bad variant signature')
        return new Variant(sig, this.read(types[0]!, depth + 1))
      }
      case '(': {
        this.align(8)
        return t.fields!.map((f) => this.read(f, depth + 1))
      }
      case '{': {
        this.align(8)
        return t.fields!.map((f) => this.read(f, depth + 1))
      }
      case 'a': {
        const n = this.u32()
        if (n > MAX_ARRAY) throw new DBusWireError('array too long')
        const elem = t.elem!
        this.align(ALIGN[elem.code]!)
        const end = this.pos + n
        if (end > this.end) throw new DBusWireError('array overruns the message')
        if (elem.code === 'y') return Buffer.from(b.subarray(this.take(n), end))
        const items: unknown[] = []
        const saved = this.end
        this.end = end
        try {
          while (this.pos < end) items.push(this.read(elem, depth + 1))
        } finally {
          this.end = saved
        }
        if (this.pos !== end) throw new DBusWireError('array length mismatch')
        return elem.code === '{' ? new Map(items as [unknown, unknown][]) : items
      }
    }
    throw new DBusWireError('bad type')
  }
}

/** Unmarshals the values of `signature` from `buf` (body of a message: alignment starts at 0). Trailing bytes are an error. */
export function unmarshal(signature: string, buf: Uint8Array, littleEndian = true): unknown[] {
  const types = parseSignature(signature)
  const data = Buffer.from(buf.buffer, buf.byteOffset, buf.byteLength)
  const r = new Reader(data, 0, data.length, littleEndian)
  const out = types.map((t) => r.read(t))
  if (r.pos !== data.length) throw new DBusWireError('trailing bytes')
  return out
}

// ───────────────────────── Messages ─────────────────────────

const FIELD_CODES = { path: 1, interface: 2, member: 3, errorName: 4, replySerial: 5, destination: 6, sender: 7, signature: 8 } as const
const FIELD_TYPES = ['', 'o', 's', 's', 's', 'u', 's', 's', 'g'] as const

/** Encodes a message (little-endian). */
export function encodeMessage(m: Message): Buffer {
  const signature = m.signature ?? ''
  const body = marshal(signature, m.body)
  const fields: [number, Variant][] = []
  for (const [key, code] of Object.entries(FIELD_CODES)) {
    const v = m[key as keyof typeof FIELD_CODES]
    if (v !== undefined && !(key === 'signature' && v === '')) fields.push([code, new Variant(FIELD_TYPES[code]!, v)])
  }
  const w = new Writer(true)
  const head = parseSignature('yyyyuua(yv)')
  const values = [0x6c, m.type, m.flags, 1, body.length, m.serial, fields]
  head.forEach((t, i) => w.write(t, values[i]))
  w.align(8)
  const out = Buffer.concat([w.done(), body])
  if (out.length > MAX_MESSAGE) throw new DBusWireError('message too large')
  return out
}

/**
 * Length of the message starting at the beginning of `buf`, or null while the 16-byte fixed header is incomplete.
 * Throws if the header is invalid or the message is over the size limit.
 */
export function messageLength(buf: Uint8Array): number | null {
  if (buf.length < 16) return null
  const b = Buffer.from(buf.buffer, buf.byteOffset, 16)
  const le = b[0] === 0x6c
  if (!le && b[0] !== 0x42) throw new DBusWireError('bad endianness')
  if (b[3] !== 1) throw new DBusWireError('unsupported protocol version')
  const bodyLen = le ? b.readUInt32LE(4) : b.readUInt32BE(4)
  const fieldsLen = le ? b.readUInt32LE(12) : b.readUInt32BE(12)
  if (fieldsLen > MAX_ARRAY) throw new DBusWireError('header too long')
  const total = 16 + fieldsLen + ((8 - ((16 + fieldsLen) % 8)) % 8) + bodyLen
  if (total > MAX_MESSAGE) throw new DBusWireError('message too large')
  return total
}

/** Decodes one complete message (either endianness). Throws `DBusWireError` on anything invalid. */
export function decodeMessage(buf: Uint8Array): Message {
  const data = Buffer.from(buf.buffer, buf.byteOffset, buf.byteLength)
  const total = messageLength(data)
  if (total === null || total !== data.length) throw new DBusWireError('bad message length')
  const le = data[0] === 0x6c
  const type = data[1]!
  if (type < 1 || type > 4) throw new DBusWireError('unknown message type')
  const bodyLen = le ? data.readUInt32LE(4) : data.readUInt32BE(4)
  const serial = le ? data.readUInt32LE(8) : data.readUInt32BE(8)
  if (serial === 0) throw new DBusWireError('serial is zero')
  const r = new Reader(data, 12, data.length - bodyLen, le)
  const fields = r.read(parseSignature('a(yv)')[0]!) as [number, Variant][]
  r.align(8)
  if (r.pos !== data.length - bodyLen) throw new DBusWireError('bad header length')
  const m: Message = { type, flags: data[2]!, serial, body: [] }
  for (const [code, v] of fields) {
    const key = (Object.keys(FIELD_CODES) as (keyof typeof FIELD_CODES)[]).find((k) => FIELD_CODES[k] === code)
    if (!key) continue // unknown (or fds): ignored, as the spec asks
    if (v.sig !== FIELD_TYPES[code]) throw new DBusWireError('bad header field type')
    ;(m as unknown as Record<string, unknown>)[key] = v.value
  }
  const sig = m.signature ?? ''
  if (type === MessageType.MethodCall && (!m.path || !m.member)) throw new DBusWireError('method call without path or member')
  if (type === MessageType.Signal && (!m.path || !m.interface || !m.member)) throw new DBusWireError('signal without path, interface or member')
  if (type === MessageType.Error && (!m.errorName || m.replySerial === undefined)) throw new DBusWireError('error without name or reply serial')
  if (type === MessageType.MethodReturn && m.replySerial === undefined) throw new DBusWireError('return without reply serial')
  if (sig) {
    const br = new Reader(data, r.pos, data.length, le)
    m.body = parseSignature(sig).map((t) => br.read(t))
    if (br.pos !== data.length) throw new DBusWireError('trailing bytes')
  } else if (bodyLen !== 0) throw new DBusWireError('body without signature')
  return m
}

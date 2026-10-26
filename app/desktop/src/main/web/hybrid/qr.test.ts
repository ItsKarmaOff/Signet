import { describe, expect, it } from 'vitest'
import { encodeQrDigits, generateQrKeys, qrCbor, qrText } from './qr'

/** Inverse of encodeQrDigits, written independently. */
function decodeDigits(digits: string): Uint8Array {
  const partial = new Map([
    [3, 1],
    [5, 2],
    [8, 3],
    [10, 4],
    [13, 5],
    [15, 6]
  ])
  const out: number[] = []
  for (let i = 0; i < digits.length;) {
    const size = Math.min(17, digits.length - i)
    const bytes = size === 17 ? 7 : partial.get(size)!
    let n = BigInt(digits.slice(i, i + size))
    for (let j = 0; j < bytes; j++) {
      out.push(Number(n & 0xffn))
      n >>= 8n
    }
    i += size
  }
  return Uint8Array.from(out)
}

describe('encodeQrDigits', () => {
  it('known vectors', () => {
    expect(encodeQrDigits(Uint8Array.of(0x01))).toBe('001')
    expect(encodeQrDigits(Uint8Array.of(1, 2, 3, 4, 5, 6, 7))).toBe('01976943448883713')
    expect(encodeQrDigits(new Uint8Array())).toBe('')
  })
  it('digit counts for 1 to 14 bytes', () => {
    const expected = [3, 5, 8, 10, 13, 15, 17, 20, 22, 25, 27, 30, 32, 34]
    for (let n = 1; n <= 14; n++) {
      const bytes = Uint8Array.from({ length: n }, (_v, i) => 255 - i)
      const digits = encodeQrDigits(bytes)
      expect(digits.length, `${n} bytes`).toBe(expected[n - 1])
      expect(decodeDigits(digits)).toEqual(bytes)
    }
  })
})

describe('QR contents', () => {
  const keys = generateQrKeys()
  const now = 1_700_000_123_456

  it('keys: compressed P-256 key and 16-byte secret, fresh each time', () => {
    expect(keys.publicKey).toHaveLength(33)
    expect([2, 3]).toContain(keys.publicKey[0])
    expect(keys.secret).toHaveLength(16)
    const other = generateQrKeys()
    expect(other.publicKey.equals(keys.publicKey)).toBe(false)
    expect(other.secret.equals(keys.secret)).toBe(false)
  })

  it.each([
    ['get', 'ga'],
    ['create', 'mc']
  ] as const)('%s: text decodes to the expected CBOR map', (type, hint) => {
    const text = qrText(keys, type, now)
    expect(text.startsWith('FIDO:/')).toBe(true)
    expect(text.slice(6)).toMatch(/^\d+$/)
    const bytes = decodeDigits(text.slice(6))
    expect(Buffer.from(bytes).equals(Buffer.from(qrCbor(keys, type, now)))).toBe(true)
    const expected = Buffer.concat([
      Buffer.from('a6', 'hex'),
      Buffer.from('00' + '5821', 'hex'),
      keys.publicKey,
      Buffer.from('01' + '50', 'hex'),
      keys.secret,
      Buffer.from('02' + '02', 'hex'),
      Buffer.from('03' + '1a' + (1_700_000_123).toString(16).padStart(8, '0') + '04' + 'f4' + '05' + '62', 'hex'),
      Buffer.from(hint)
    ])
    expect(Buffer.from(bytes).toString('hex')).toBe(expected.toString('hex'))
  })
})

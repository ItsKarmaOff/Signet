import { describe, expect, it } from 'vitest'
import { CborError, decodeCbor, decodeCborPrefix, encodeCbor, type CborValue } from './cbor'

const hex = (v: CborValue): string => Buffer.from(encodeCbor(v)).toString('hex')

describe('encodeCbor', () => {
  it.each([
    [0, '00'],
    [23, '17'],
    [24, '1818'],
    [255, '18ff'],
    [256, '190100'],
    [65535, '19ffff'],
    [65536, '1a00010000'],
    [2 ** 32, '1b0000000100000000'],
    [0xffffffffffffffffn, '1bffffffffffffffff'],
    [-1, '20'],
    [-24, '37'],
    [-25, '3818'],
    [-256, '38ff'],
    [-257, '390100']
  ])('integer %s', (value, expected) => {
    expect(hex(value)).toBe(expected)
  })

  it('byte strings', () => {
    expect(hex(new Uint8Array())).toBe('40')
    expect(hex(Uint8Array.of(1, 2, 3))).toBe('43010203')
    expect(hex(new Uint8Array(24))).toBe('5818' + '00'.repeat(24))
    expect(hex(new Uint8Array(256))).toBe('590100' + '00'.repeat(256))
  })

  it('text strings (UTF-8 length)', () => {
    expect(hex('')).toBe('60')
    expect(hex('ga')).toBe('626761')
    expect(hex('é')).toBe('62c3a9')
    expect(hex('a'.repeat(24))).toBe('7818' + '61'.repeat(24))
    expect(hex('a'.repeat(300))).toBe('79012c' + '61'.repeat(300))
  })

  it('arrays', () => {
    expect(hex([])).toBe('80')
    expect(hex([1, [2, 3]])).toBe('8201820203')
  })

  it('maps sort their keys (shorter first, then bytewise)', () => {
    expect(
      hex(
        new Map<CborValue, CborValue>([
          [2, 'b'],
          [0, 'a'],
          [1, true]
        ])
      )
    ).toBe('a3' + '00' + '6161' + '01' + 'f5' + '02' + '6162')
    expect(
      hex(
        new Map<CborValue, CborValue>([
          [24, 1],
          [23, 2]
        ])
      )
    ).toBe('a2' + '17' + '02' + '1818' + '01')
    expect(hex(new Map())).toBe('a0')
  })

  it('rejects duplicate keys', () => {
    expect(() =>
      encodeCbor(
        new Map<CborValue, CborValue>([
          [1, 1],
          [1n, 2]
        ])
      )
    ).toThrow()
  })

  it('booleans and null', () => {
    expect(hex(true)).toBe('f5')
    expect(hex(false)).toBe('f4')
    expect(hex(null)).toBe('f6')
  })

  it('rejects what it cannot encode', () => {
    expect(() => encodeCbor(1.5)).toThrow()
    expect(() => encodeCbor(2n ** 64n)).toThrow()
  })
})

const dec = (hexString: string): CborValue => decodeCbor(Buffer.from(hexString, 'hex'))

describe('decodeCbor', () => {
  it.each([
    ['00', 0],
    ['17', 23],
    ['1818', 24],
    ['1903e8', 1000],
    ['1a000f4240', 1000000],
    ['20', -1],
    ['3863', -100],
    ['3903e7', -1000],
    ['1b0000000100000000', 2 ** 32],
    ['1b001fffffffffffff', Number.MAX_SAFE_INTEGER],
    ['f4', false],
    ['f5', true],
    ['f6', null],
    ['f7', undefined]
  ])('simple value %s', (input, expected) => {
    expect(dec(input)).toBe(expected)
  })

  it('64-bit integers beyond the safe range are bigint', () => {
    expect(dec('1bffffffffffffffff')).toBe(0xffffffffffffffffn)
    expect(dec('1b0020000000000000')).toBe(2n ** 53n)
    expect(dec('3bffffffffffffffff')).toBe(-0x10000000000000000n)
    expect(dec('3b001ffffffffffffe')).toBe(Number.MIN_SAFE_INTEGER)
    expect(dec('3b001fffffffffffff')).toBe(-(2n ** 53n))
  })

  it('byte and text strings', () => {
    expect(dec('40')).toEqual(new Uint8Array())
    expect(dec('4401020304')).toEqual(Uint8Array.of(1, 2, 3, 4))
    expect(dec('60')).toBe('')
    expect(dec('62c3a9')).toBe('é')
    expect(dec('7818' + '61'.repeat(24))).toBe('a'.repeat(24))
  })

  it('arrays and maps with integer or text keys', () => {
    expect(dec('83010203')).toEqual([1, 2, 3])
    expect(dec('8301820203820405')).toEqual([1, [2, 3], [4, 5]])
    expect(dec('a0')).toEqual(new Map())
    const m = dec('a2200102' + '03') as Map<CborValue, CborValue>
    expect(m.get(-1)).toBe(1)
    expect(m.get(2)).toBe(3)
    const t = dec('a2616101' + '626263' + '02') as Map<CborValue, CborValue>
    expect(t.get('a')).toBe(1)
    expect(t.get('bc')).toBe(2)
  })

  it('round trips with the encoder', () => {
    const value = new Map<CborValue, CborValue>([
      [1, Uint8Array.of(1, 2)],
      [
        2,
        new Map<CborValue, CborValue>([
          ['rk', true],
          ['x', [1, -1, 'é', null]]
        ])
      ],
      [-3, 0xffffffffffffffffn],
      ['fmt', 'none']
    ])
    expect(dec(Buffer.from(encodeCbor(value)).toString('hex'))).toEqual(value)
  })

  it('decodes a prefix and says how long it was', () => {
    const { value, length } = decodeCborPrefix(Buffer.from('a10102ff', 'hex'))
    expect(value).toEqual(new Map([[1, 2]]))
    expect(length).toBe(3)
  })

  it.each([
    ['empty input', ''],
    ['truncated integer', '19ff'],
    ['truncated byte string', '4401'],
    ['byte string longer than the input', '5b00000000ffffffff'],
    ['array longer than the input', '9b00000000ffffffff'],
    ['indefinite byte string', '5f4101ff'],
    ['indefinite array', '9f01ff'],
    ['indefinite map', 'bf0102ff'],
    ['reserved additional information', '1c'],
    ['non-shortest integer', '1800'],
    ['non-shortest 16-bit integer', '190017'],
    ['non-shortest length', '780161'],
    ['tag', 'c101'],
    ['float 16', 'f93c00'],
    ['float 64', 'fb3ff0000000000000'],
    ['simple value 16', 'f0'],
    ['simple value in the next byte', 'f820'],
    ['break outside of anything', 'ff'],
    ['invalid UTF-8', '62c328'],
    ['duplicate map keys', 'a201010102'],
    ['duplicate text map keys', 'a2616101616102'],
    ['byte string map key', 'a1410101'],
    ['array map key', 'a1810101'],
    ['trailing bytes', '0000'],
    ['map missing a value', 'a101']
  ])('rejects %s', (_name, input) => {
    expect(() => dec(input)).toThrow(CborError)
  })

  it('rejects nesting deeper than 16', () => {
    expect(dec('81'.repeat(16) + '00')).toBeDefined()
    expect(() => dec('81'.repeat(17) + '00')).toThrow(CborError)
  })

  it('does not allocate for huge declared sizes', () => {
    expect(() => dec('9a7fffffff')).toThrow(CborError)
    expect(() => dec('ba7fffffff')).toThrow(CborError)
  })
})

import { describe, expect, it } from 'vitest'
import { encodeCbor, type CborValue } from './cbor'

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

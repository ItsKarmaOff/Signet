import { describe, expect, it } from 'vitest'
import { cardDigits, detectNetwork, formatExpiry, formatYear, fullYear, groupCardNumber, isExpired, luhn, parseExpiry } from './card'

describe('Luhn', () => {
  it('accepts valid test numbers', () => {
    for (const n of ['4242424242424242', '5555555555554444', '378282246310005', '6011111111111117', '3530111333300000']) expect(luhn(n)).toBe(true)
  })
  it('rejects a changed digit, short numbers and text', () => {
    expect(luhn('4242424242424241')).toBe(false)
    expect(luhn('424242')).toBe(false)
    expect(luhn('abcdefghijklmnop')).toBe(false)
  })
  it('cardDigits drops spaces and dashes only', () => {
    expect(cardDigits('4242 4242-4242 4242')).toBe('4242424242424242')
    expect(cardDigits('4242 4242 x')).toBeNull()
    expect(cardDigits('1234')).toBeNull()
  })
})

describe('network', () => {
  it.each([
    ['4242424242424242', 'visa'],
    ['5555555555554444', 'mastercard'],
    ['2223003122003222', 'mastercard'],
    ['378282246310005', 'amex'],
    ['6011111111111117', 'discover'],
    ['6221260000000000', 'discover'],
    ['6200000000000005', 'unionpay'],
    ['3530111333300000', 'jcb'],
    ['30569309025904', 'diners'],
    ['6759649826438453', 'maestro'],
    ['9999999999999995', 'unknown']
  ])('%s is %s', (n, network) => {
    expect(detectNetwork(n)).toBe(network)
  })

  it('groups digits like the card (4-6-5 for Amex)', () => {
    expect(groupCardNumber('4242424242424242')).toBe('4242 4242 4242 4242')
    expect(groupCardNumber('378282246310005')).toBe('3782 822463 10005')
  })
})

describe('expiry', () => {
  it('parses the usual shapes', () => {
    expect(parseExpiry('08/27')).toEqual({ month: 8, year: 2027 })
    expect(parseExpiry('8/2027')).toEqual({ month: 8, year: 2027 })
    expect(parseExpiry('0827')).toEqual({ month: 8, year: 2027 })
    expect(parseExpiry('08 / 27')).toEqual({ month: 8, year: 2027 })
    expect(parseExpiry('12-2030')).toEqual({ month: 12, year: 2030 })
    expect(parseExpiry('1227')).toEqual({ month: 12, year: 2027 })
  })
  it('refuses nonsense', () => {
    expect(parseExpiry('13/27')).toBeNull()
    expect(parseExpiry('00/27')).toBeNull()
    expect(parseExpiry('ab/cd')).toBeNull()
    expect(parseExpiry('')).toBeNull()
  })
  it('two-digit years are 20xx', () => {
    expect(fullYear(27)).toBe(2027)
    expect(fullYear(2027)).toBe(2027)
    expect(fullYear(1999)).toBeNull()
  })
  it('expired once the month is over', () => {
    const now = new Date(2026, 10, 1)
    expect(isExpired(10, 2026, now)).toBe(true)
    expect(isExpired(11, 2026, now)).toBe(false)
    expect(isExpired(1, 2027, now)).toBe(false)
    expect(isExpired(12, 2025, now)).toBe(true)
    expect(isExpired(13, 2030, now)).toBe(true)
  })
  it('formats a combined field after its hints', () => {
    expect(formatExpiry(8, 2027)).toBe('08/27')
    expect(formatExpiry(8, 2027, { placeholder: 'MM/YYYY' })).toBe('08/2027')
    expect(formatExpiry(8, 2027, { placeholder: 'MM / YY' })).toBe('08 / 27')
    expect(formatExpiry(8, 2027, { placeholder: 'MM-YY' })).toBe('08-27')
    expect(formatExpiry(8, 2027, { maxLength: 4 })).toBe('0827')
    expect(formatExpiry(8, 2027, { maxLength: 7 })).toBe('08/2027')
    expect(formatExpiry(8, 2027, { maxLength: 6 })).toBe('082027')
  })
  it('formats a year field after its hints', () => {
    expect(formatYear(2027)).toBe('2027')
    expect(formatYear(2027, { maxLength: 2 })).toBe('27')
    expect(formatYear(2027, { placeholder: 'YY' })).toBe('27')
    expect(formatYear(2027, { maxLength: 4 })).toBe('2027')
  })
})

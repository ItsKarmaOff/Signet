// Card helpers shared by main and the page script: Luhn, network, expiry formats. Pure, no DOM, no Electron.
import type { CardNetwork } from '../types/autofill'

/** Digits of a typed card number (spaces and dashes dropped); null if anything else is in it or the length is off. */
export function cardDigits(input: string): string | null {
  const digits = input.replace(/[\s-]/g, '')
  return /^\d{12,19}$/.test(digits) ? digits : null
}

/** Luhn checksum. */
export function luhn(digits: string): boolean {
  if (!/^\d{12,19}$/.test(digits)) return false
  let sum = 0
  for (let i = 0; i < digits.length; i++) {
    let d = Number(digits[digits.length - 1 - i])
    if (i % 2 === 1) {
      d *= 2
      if (d > 9) d -= 9
    }
    sum += d
  }
  return sum % 10 === 0
}

const inRange = (n: number, lo: number, hi: number) => n >= lo && n <= hi

/** Network from the number prefix. */
export function detectNetwork(digits: string): CardNetwork {
  const p = (n: number) => Number(digits.slice(0, n))
  if (/^(34|37)/.test(digits)) return 'amex'
  if (/^4/.test(digits)) return 'visa'
  if (inRange(p(2), 51, 55) || inRange(p(4), 2221, 2720)) return 'mastercard'
  if (/^6011/.test(digits) || /^65/.test(digits) || inRange(p(3), 644, 649) || inRange(p(6), 622126, 622925)) return 'discover'
  if (/^62/.test(digits)) return 'unionpay'
  if (inRange(p(4), 3528, 3589)) return 'jcb'
  if (inRange(p(3), 300, 305) || /^(36|38|39)/.test(digits)) return 'diners'
  if (/^(5018|5020|5038|5893|6304|6759|676[1-3])/.test(digits)) return 'maestro'
  return 'unknown'
}

export const NETWORK_NAMES: Record<CardNetwork, string> = {
  visa: 'Visa',
  mastercard: 'Mastercard',
  amex: 'American Express',
  discover: 'Discover',
  diners: 'Diners Club',
  jcb: 'JCB',
  unionpay: 'UnionPay',
  maestro: 'Maestro',
  unknown: ''
}

/** Number with spaces, the way it is printed on the card (4-6-5 for Amex). */
export function groupCardNumber(digits: string, network: CardNetwork = detectNetwork(digits)): string {
  if (network === 'amex' && digits.length === 15) return `${digits.slice(0, 4)} ${digits.slice(4, 10)} ${digits.slice(10)}`
  return digits.replace(/(\d{4})(?=\d)/g, '$1 ')
}

/** Four-digit year from a two or four-digit one; null if it makes no sense. */
export function fullYear(year: number): number | null {
  if (!Number.isInteger(year) || year < 0) return null
  const y = year < 100 ? 2000 + year : year
  return y >= 2000 && y <= 2100 ? y : null
}

/** Expiry typed in one field: `08/27`, `8/2027`, `0827`, `08 - 2027`. */
export function parseExpiry(text: string): { month: number; year: number } | null {
  const m = /^\s*(\d{1,2})\s*[/\-. ]?\s*(\d{2}|\d{4})\s*$/.exec(text)
  if (!m) return null
  const month = Number(m[1])
  const year = fullYear(Number(m[2]))
  if (month < 1 || month > 12 || year === null) return null
  return { month, year }
}

/** True once the last day of the expiry month is over. */
export function isExpired(month: number, year: number, now = new Date()): boolean {
  const y = fullYear(year)
  if (y === null || month < 1 || month > 12) return true
  return y < now.getFullYear() || (y === now.getFullYear() && month < now.getMonth() + 1)
}

export interface ExpiryHint {
  maxLength?: number
  placeholder?: string
}

/** Combined expiry field: `MM/YY` by default, `MM/YYYY`, `MMYY`, `MM-YY`… following the placeholder or the max length. */
export function formatExpiry(month: number, year: number, hint: ExpiryHint = {}): string {
  const mm = String(month).padStart(2, '0')
  const full = String(fullYear(year) ?? year)
  const ph = hint.placeholder ?? ''
  const max = hint.maxLength && hint.maxLength > 0 ? hint.maxLength : 0
  const longYear = /y{4}|a{4}|\d{2}\s*[/-]\s*\d{4}/i.test(ph) || max === 7 || max === 6
  const sep = max === 4 || max === 6 ? '' : /\s\/\s/.test(ph) ? ' / ' : /-/.test(ph) ? '-' : '/'
  return `${mm}${sep}${longYear ? full : full.slice(-2)}`
}

/** Year for its own field: two digits when the field only holds two, four otherwise. */
export function formatYear(year: number, hint: ExpiryHint = {}): string {
  const full = String(fullYear(year) ?? year)
  const short = hint.maxLength === 2 || /^(y{2}|a{2})$/i.test((hint.placeholder ?? '').trim())
  return short ? full.slice(-2) : full
}

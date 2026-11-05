import { describe, expect, it } from 'vitest'
import { hasToken, messageOrigin, newPageToken, parseAutofillMessage, parseMessage } from './autofill-messages'

const T = 'abc123'

describe('newPageToken', () => {
  it('is 32 hex characters and changes every time', () => {
    const a = newPageToken()
    expect(a).toMatch(/^[0-9a-f]{32}$/)
    expect(newPageToken()).not.toBe(a)
  })
})

describe('parseMessage / hasToken', () => {
  it('reads objects only', () => {
    expect(parseMessage('{"a":1}')).toEqual({ a: 1 })
    expect(parseMessage('[1]')).toBeNull()
    expect(parseMessage('nope')).toBeNull()
  })
  it('needs the exact token', () => {
    expect(hasToken({ k: T }, T)).toBe(true)
    expect(hasToken({ k: 'x' }, T)).toBe(false)
    expect(hasToken({}, T)).toBe(false)
    expect(hasToken(null, T)).toBe(false)
    expect(hasToken({ k: '' }, '')).toBe(false)
  })
})

describe('messageOrigin', () => {
  it('accepts http(s) origins only', () => {
    expect(messageOrigin('https://shop.example')).toBe('https://shop.example')
    expect(messageOrigin('http://localhost:3000')).toBe('http://localhost:3000')
    expect(messageOrigin('https://shop.example/path')).toBeNull()
    expect(messageOrigin('null')).toBeNull()
    expect(messageOrigin('javascript:1')).toBeNull()
    expect(messageOrigin(12)).toBeNull()
  })
})

describe('parseAutofillMessage', () => {
  it('drops a message with a missing or wrong token', () => {
    expect(parseAutofillMessage({ type: 'af:blur' }, T)).toBeNull()
    expect(parseAutofillMessage({ type: 'af:blur', k: 'x' }, T)).toBeNull()
  })
  it('reads focus, blur and filled', () => {
    expect(parseAutofillMessage({ type: 'af:focus', k: T, kind: 'card', origin: 'https://a.example' }, T)).toEqual({
      type: 'af:focus',
      kind: 'card',
      origin: 'https://a.example'
    })
    expect(parseAutofillMessage({ type: 'af:focus', k: T, kind: 'other', origin: 'https://a.example' }, T)).toBeNull()
    expect(parseAutofillMessage({ type: 'af:focus', k: T, kind: 'card', origin: 'ftp://a' }, T)).toBeNull()
    expect(parseAutofillMessage({ type: 'af:blur', k: T }, T)).toEqual({ type: 'af:blur' })
    expect(parseAutofillMessage({ type: 'af:filled', k: T, ok: false }, T)).toEqual({ type: 'af:filled', ok: false })
    expect(parseAutofillMessage({ type: 'af:filled', k: T, ok: 'yes' }, T)).toBeNull()
    expect(parseAutofillMessage({ type: 'af:unknown', k: T }, T)).toBeNull()
  })
  it('keeps known address fields, clamped, and drops the rest', () => {
    const m = parseAutofillMessage(
      { type: 'af:submit', k: T, origin: 'https://a.example', address: { city: 'Paris', street: 'x'.repeat(5000), evil: 'y', email: 3 } },
      T
    )
    expect(m?.type).toBe('af:submit')
    if (m?.type !== 'af:submit') return
    expect(m.address?.city).toBe('Paris')
    expect(m.address?.street).toHaveLength(600)
    expect(m.address).not.toHaveProperty('evil')
    expect(m.address).not.toHaveProperty('email')
  })
  it('reads a card with a number, never other keys', () => {
    const m = parseAutofillMessage(
      { type: 'af:submit', k: T, origin: 'https://a.example', card: { holder: 'A B', number: '4242 4242 4242 4242', exp: '08/27', cvc: '123' } },
      T
    )
    if (m?.type !== 'af:submit') throw new Error('expected a submit')
    expect(m.card).toEqual({ holder: 'A B', number: '4242 4242 4242 4242', exp: '08/27', expMonth: '', expYear: '' })
    expect(m.card).not.toHaveProperty('cvc')
  })
  it('drops a submit with nothing usable or a bad origin', () => {
    expect(parseAutofillMessage({ type: 'af:submit', k: T, origin: 'https://a.example', card: { holder: 'A' } }, T)).toBeNull()
    expect(parseAutofillMessage({ type: 'af:submit', k: T, origin: 'x', address: { city: 'Paris' } }, T)).toBeNull()
  })
})

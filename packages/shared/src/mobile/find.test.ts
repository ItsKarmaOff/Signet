import { describe, expect, it } from 'vitest'
import { escapeRegExp, findClearScript, findScript, findStepScript, parseFindMessage } from './find'

describe('escapeRegExp', () => {
  it('escapes special characters', () => {
    const input = 'a.b*c+d?e^f$g{h}i(j)k|l[m]n\\o/p-q'
    const rx = new RegExp(escapeRegExp(input), 'i')
    expect(rx.test(input)).toBe(true)
    expect(rx.test('aXb*c')).toBe(false)
  })
  it('leaves ordinary text intact', () => {
    expect(escapeRegExp('bonjour le monde')).toBe('bonjour le monde')
  })
})

describe('scripts', () => {
  it('are valid JavaScript', () => {
    for (const js of [findScript('café "x" \\ </script>'), findStepScript(1), findStepScript(-1), findClearScript()]) {
      expect(() => new Function(js)).not.toThrow()
    }
  })
  it('pass the query through JSON (no string breakout)', () => {
    const js = findScript('a"b')
    expect(js).toContain('"a\\"b"')
  })
})

describe('parseFindMessage', () => {
  it('reads a result', () => {
    expect(parseFindMessage('{"type":"find","count":3,"index":0}')).toEqual({ type: 'find', count: 3, index: 0 })
  })
  it('ignores the rest', () => {
    expect(parseFindMessage('{"type":"meta"}')).toBeNull()
    expect(parseFindMessage('pas du json')).toBeNull()
    expect(parseFindMessage('{"type":"find","count":"3"}')).toBeNull()
  })
})

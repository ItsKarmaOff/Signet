import { mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { DNR_ENUMS } from '@shared/extension-apis'
import { DnrStore, validateRule } from './dnr'

let dir: string
beforeEach(() => {
  dir = mkdtempSync(join(tmpdir(), 'signet-dnr-'))
})
afterEach(() => rmSync(dir, { recursive: true, force: true, maxRetries: 5, retryDelay: 50 }))

const rule = (id: number) => ({
  id,
  priority: 1,
  action: { type: 'modifyHeaders', requestHeaders: [{ header: 'User-Agent', operation: 'set', value: 'x' }] },
  condition: { urlFilter: '|https://a.example/*', resourceTypes: ['xmlhttprequest'] }
})
const A = 'a'.repeat(32)

describe('validateRule', () => {
  it('accepts a header rule and rejects malformed ones', () => {
    expect(validateRule(rule(1)).id).toBe(1)
    for (const bad of [
      { ...rule(1), action: { type: 'MODIFY_HEADERS' } },
      { ...rule(1), action: { type: 'modifyHeaders', requestHeaders: [{ header: 'x', operation: 'SET' }] } },
      { ...rule(1), condition: { resourceTypes: ['XMLHTTPREQUEST'] } },
      { ...rule(1), condition: { requestMethods: ['GET'] } },
      { ...rule(1), condition: { domainType: 'THIRD_PARTY' } }
    ])
      expect(() => validateRule(bad)).toThrow()
    for (const bad of [null, { ...rule(0) }, { ...rule(1), action: { type: 'NOPE' } }, { ...rule(1), condition: null }, { ...rule(1), priority: 0 }]) {
      expect(() => validateRule(bad)).toThrow()
    }
  })
})

describe('DnrStore', () => {
  it('adds, removes first, filters by ruleIds, keeps extensions apart', () => {
    const s = new DnrStore(join(dir, 'd.json'))
    s.update('dynamic', A, { addRules: [rule(1), rule(2)] })
    s.update('dynamic', A, { removeRuleIds: [1], addRules: [rule(1), rule(3)] })
    expect(s.get('dynamic', A, undefined).map((r) => r.id)).toEqual([2, 1, 3])
    expect(s.get('dynamic', A, { ruleIds: [3] }).map((r) => r.id)).toEqual([3])
    expect(s.get('dynamic', 'b'.repeat(32), undefined)).toEqual([])
  })

  it('is all or nothing and refuses duplicate ids', () => {
    const s = new DnrStore(join(dir, 'd.json'))
    s.update('dynamic', A, { addRules: [rule(1)] })
    expect(() => s.update('dynamic', A, { addRules: [rule(2), rule(1)] })).toThrow(/unique/)
    expect(s.get('dynamic', A, undefined).map((r) => r.id)).toEqual([1])
    expect(() => s.update('dynamic', A, { addRules: [rule(5), rule(5)] })).toThrow(/unique/)
  })

  it('persists dynamic rules, not session rules, and drops them on removal', () => {
    const file = join(dir, 'd.json')
    const s = new DnrStore(file)
    s.update('dynamic', A, { addRules: [rule(1)] })
    s.update('session', A, { addRules: [rule(9)] })
    expect(s.flush()).toBe(true)
    const again = new DnrStore(file)
    expect(again.get('dynamic', A, undefined)).toHaveLength(1)
    expect(again.get('session', A, undefined)).toHaveLength(0)
    again.remove(A)
    expect(again.get('dynamic', A, undefined)).toHaveLength(0)
  })

  it('drops stored rules written with enum names instead of Chrome values, and says so', () => {
    const file = join(dir, 'old.json')
    writeFileSync(file, JSON.stringify({ dynamic: { [A]: [{ ...rule(1), action: { type: 'MODIFY_HEADERS' } }, rule(2)] } }))
    const log = vi.fn()
    const s = new DnrStore(file, log)
    expect(s.get('dynamic', A, undefined).map((r) => r.id)).toEqual([2])
    expect(log).toHaveBeenCalledWith(expect.stringMatching(/dropped 1/))
  })

  it('exposes Chrome enum values', () => {
    expect(DNR_ENUMS.RuleActionType.MODIFY_HEADERS).toBe('modifyHeaders')
    expect(DNR_ENUMS.ResourceType.XMLHTTPREQUEST).toBe('xmlhttprequest')
    expect(DNR_ENUMS.HeaderOperation.SET).toBe('set')
    expect(DNR_ENUMS.DomainType.THIRD_PARTY).toBe('thirdParty')
  })

  it('logs once per extension that rules are not enforced', () => {
    const log = vi.fn()
    const s = new DnrStore(join(dir, 'd.json'), log)
    s.update('dynamic', A, { addRules: [rule(1)] })
    s.update('dynamic', A, { addRules: [rule(2)] })
    expect(log).toHaveBeenCalledOnce()
    expect(log.mock.calls[0][0]).toMatch(/NOT enforced/)
  })
})

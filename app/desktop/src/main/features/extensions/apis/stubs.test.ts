import { describe, expect, it } from 'vitest'
import { StubRegistry } from './stubs'

describe('StubRegistry', () => {
  it('reports first only once per extension version and merges contexts', () => {
    const r = new StubRegistry()
    expect(r.record('a', '1.0', ['tabs.group'])).toBe(true)
    expect(r.record('a', '1.0', ['userScripts', 'tabs.group'])).toBe(false)
    expect(r.record('a', '2.0', [])).toBe(true)
    expect(r.of('a', '1.0')).toEqual(['tabs.group', 'userScripts'])
    expect(r.of('a')).toEqual(['tabs.group', 'userScripts'])
    expect(r.of('b')).toEqual([])
  })
})

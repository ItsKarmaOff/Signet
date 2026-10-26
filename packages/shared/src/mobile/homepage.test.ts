import { describe, expect, it } from 'vitest'
import { parseHomepageInput } from './settings'

describe('parseHomepageInput', () => {
  it('accepts web addresses and adds https', () => {
    expect(parseHomepageInput('example.com')).toBe('https://example.com')
    expect(parseHomepageInput(' https://example.com/a?b=1 ')).toBe('https://example.com/a?b=1')
    expect(parseHomepageInput('http://localhost:3000')).toBe('http://localhost:3000')
  })

  it('rejects searches, other schemes and empty input', () => {
    for (const bad of ['', '   ', 'hello world', 'javascript:alert(1)', 'about:blank', 'file:///etc/passwd', 'ftp://x.test', 'justaword']) {
      expect(parseHomepageInput(bad)).toBeNull()
    }
  })
})

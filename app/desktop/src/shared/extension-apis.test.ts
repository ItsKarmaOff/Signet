import { describe, expect, it } from 'vitest'
import { isExtensionContext } from './extension-apis'

const id = 'a'.repeat(32)
describe('isExtensionContext', () => {
  it('extension service worker: yes', () => {
    expect(isExtensionContext({ processType: 'service-worker', url: `chrome-extension://${id}/sw.js`, sandboxed: false })).toBe(true)
  })
  it('web site service worker: no', () => {
    expect(isExtensionContext({ processType: 'service-worker', url: 'https://example.com/sw.js', sandboxed: false })).toBe(false)
    expect(isExtensionContext({ processType: 'service-worker', url: null, sandboxed: false })).toBe(false)
  })
  it('extension page or extension subframe: yes, sandboxed page: no', () => {
    expect(isExtensionContext({ processType: 'renderer', url: `chrome-extension://${id}/popup.html`, sandboxed: false })).toBe(true)
    expect(isExtensionContext({ processType: 'renderer', url: `chrome-extension://${id}/popup.html`, sandboxed: true })).toBe(false)
  })
  it('web page: no', () => {
    expect(isExtensionContext({ processType: 'renderer', url: 'https://example.com/', sandboxed: false })).toBe(false)
    expect(isExtensionContext({ processType: 'renderer', url: undefined, sandboxed: false })).toBe(false)
  })
})

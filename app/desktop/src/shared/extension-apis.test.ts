import { describe, expect, it } from 'vitest'
import { isExtensionContext } from './extension-apis'

const id = 'a'.repeat(32)
describe('isExtensionContext', () => {
  it('extension service worker: yes', () => {
    expect(isExtensionContext({ processType: 'service-worker', url: `chrome-extension://${id}/sw.js`, isTopFrame: true })).toBe(true)
  })
  it('web site service worker: no', () => {
    expect(isExtensionContext({ processType: 'service-worker', url: 'https://example.com/sw.js', isTopFrame: true })).toBe(false)
    expect(isExtensionContext({ processType: 'service-worker', url: null, isTopFrame: true })).toBe(false)
  })
  it('extension page: top frame yes, subframe no', () => {
    expect(isExtensionContext({ processType: 'renderer', url: `chrome-extension://${id}/popup.html`, isTopFrame: true })).toBe(true)
    expect(isExtensionContext({ processType: 'renderer', url: `chrome-extension://${id}/popup.html`, isTopFrame: false })).toBe(false)
  })
  it('web page: no', () => {
    expect(isExtensionContext({ processType: 'renderer', url: 'https://example.com/', isTopFrame: true })).toBe(false)
    expect(isExtensionContext({ processType: 'renderer', url: undefined, isTopFrame: true })).toBe(false)
  })
})

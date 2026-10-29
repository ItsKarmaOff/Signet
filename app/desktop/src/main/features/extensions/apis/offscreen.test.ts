import { describe, expect, it, vi } from 'vitest'
import { Offscreen, type OffscreenPage } from './offscreen'

const A = 'a'.repeat(32)
const params = (url = 'offscreen.html') => ({ url, reasons: ['LOCAL_STORAGE', 'DOM_PARSER'], justification: 'dom' })

function make(loadError?: Error) {
  const pages: (OffscreenPage & { closed: boolean; fireClosed: () => void })[] = []
  const off = new Offscreen(() => {
    let closedFn = () => {}
    const page = {
      id: 40 + pages.length,
      closed: false,
      load: vi.fn(() => (loadError ? Promise.reject(loadError) : Promise.resolve())),
      close() {
        page.closed = true
      },
      onClosed: (fn: () => void) => (closedFn = fn),
      fireClosed: () => closedFn()
    }
    pages.push(page)
    return page
  })
  return { off, pages }
}

describe('Offscreen', () => {
  it('creates one document, resolves its url against the extension and reports it as a context', async () => {
    const { off, pages } = make()
    await off.create({}, A, params())
    expect(pages[0].load).toHaveBeenCalledWith(`chrome-extension://${A}/offscreen.html`)
    expect(off.has(A)).toBe(true)
    expect(off.owns(40)).toBe(true)
    expect(off.contexts(A)).toMatchObject([
      { contextType: 'OFFSCREEN_DOCUMENT', documentUrl: `chrome-extension://${A}/offscreen.html`, documentOrigin: `chrome-extension://${A}` }
    ])
  })

  it('refuses a second document, a foreign origin and bad parameters', async () => {
    const { off } = make()
    await off.create({}, A, params())
    await expect(off.create({}, A, params())).rejects.toThrow(/single offscreen document/)
    await expect(make().off.create({}, A, params('https://evil.example/x.html'))).rejects.toThrow(/pages of the extension/)
    await expect(make().off.create({}, A, params(`chrome-extension://${'b'.repeat(32)}/x.html`))).rejects.toThrow(/pages of the extension/)
    await expect(make().off.create({}, A, { url: 'a.html', reasons: [], justification: 'x' })).rejects.toThrow(/reasons/)
    await expect(make().off.create({}, A, { url: 'a.html', reasons: ['NOPE'], justification: 'x' })).rejects.toThrow(/reasons/)
    await expect(make().off.create({}, A, { url: 'a.html', reasons: ['TESTING'] })).rejects.toThrow(/justification/)
  })

  it('close, closeFor, and the page closing by itself free the slot', async () => {
    const { off, pages } = make()
    await off.create({}, A, params())
    off.close(A)
    expect(pages[0].closed).toBe(true)
    expect(off.has(A)).toBe(false)
    expect(() => off.close(A)).toThrow(/No current offscreen document/)
    expect(() => off.closeFor(A)).not.toThrow()
    await off.create({}, A, params())
    pages[1].fireClosed()
    expect(off.has(A)).toBe(false)
  })

  it('a failed load frees the slot and closes the page', async () => {
    const { off, pages } = make(new Error('ERR_FILE_NOT_FOUND'))
    await expect(off.create({}, A, params())).rejects.toThrow(/ERR_FILE_NOT_FOUND/)
    expect(off.has(A)).toBe(false)
    expect(pages[0].closed).toBe(true)
  })
})

import { describe, expect, it, vi } from 'vitest'
import { CssRegistry, cssSource, parseCaptureArgs, parseCssDetails, primaryLanguage, TabsExtras, type TabLike } from './tabs'
import type { ApiCaller } from './types'

const ID = 'a'.repeat(32)
const caller = { extensionId: ID } as ApiCaller

describe('parseCaptureArgs', () => {
  it('defaults to jpeg quality 92 and takes the window id and options in either position', () => {
    expect(parseCaptureArgs(undefined, undefined)).toEqual({ windowId: undefined, format: 'jpeg', quality: 92 })
    expect(parseCaptureArgs(5, { format: 'png' })).toEqual({ windowId: 5, format: 'png', quality: 92 })
    expect(parseCaptureArgs({ format: 'jpeg', quality: 40 }, undefined)).toEqual({ windowId: undefined, format: 'jpeg', quality: 40 })
  })
  it('clamps quality, ignores the current-window constant and rejects an unknown format', () => {
    expect(parseCaptureArgs(-2, { quality: 250 })).toMatchObject({ windowId: undefined, quality: 100 })
    expect(parseCaptureArgs(1, { quality: -5.4 }).quality).toBe(0)
    expect(parseCaptureArgs(1, { quality: Number.NaN }).quality).toBe(92)
    expect(() => parseCaptureArgs(1, { format: 'gif' })).toThrow(/format/)
  })
})

describe('primaryLanguage', () => {
  it('keeps the primary tag, Chinese keeps its script, anything else is und', () => {
    expect(primaryLanguage('en-US')).toBe('en')
    expect(primaryLanguage(' FR_ca ')).toBe('fr')
    expect(primaryLanguage('zh-Hant')).toBe('zh-TW')
    expect(primaryLanguage('zh')).toBe('zh-CN')
    expect(primaryLanguage('')).toBe('und')
    expect(primaryLanguage(undefined)).toBe('und')
    expect(primaryLanguage('x-default')).toBe('und')
  })
})

describe('CSS bookkeeping', () => {
  it('requires exactly one of code and file and tells the origins apart', () => {
    expect(() => parseCssDetails({})).toThrow()
    expect(() => parseCssDetails({ code: 'a', file: 'b' })).toThrow()
    expect(cssSource(parseCssDetails({ code: 'a{}' }))).not.toBe(cssSource(parseCssDetails({ code: 'a{}', cssOrigin: 'user' })))
    expect(cssSource(parseCssDetails({ file: 'a.css' }))).not.toBe(cssSource(parseCssDetails({ code: 'a.css' })))
  })
  it('keys are kept per extension, tab and source, and taken once', () => {
    const r = new CssRegistry()
    r.add('e1', 1, 's', 'k1')
    r.add('e1', 1, 's', 'k2')
    r.add('e2', 1, 's', 'k3')
    r.add('e1', 2, 's', 'k4')
    expect(r.take('e1', 1, 's')).toEqual(['k1', 'k2'])
    expect(r.take('e1', 1, 's')).toEqual([])
    r.forgetTab(1)
    expect(r.take('e2', 1, 's')).toEqual([])
    r.forgetExtension('e1')
    expect(r.take('e1', 2, 's')).toEqual([])
  })
})

function setup(opts: { url?: string; access?: { permissions: string[]; origins: string[] } } = {}) {
  let n = 0
  const tab = {
    id: 7,
    getURL: () => opts.url ?? 'https://a.test/page',
    isDestroyed: () => false,
    capturePage: vi.fn(async () => ({ toJPEG: (q: number) => Buffer.from(`jpeg${q}`), toDataURL: () => 'data:image/png;base64,PNG' })),
    insertCSS: vi.fn(async () => `key${++n}`),
    removeInsertedCSS: vi.fn(async () => {}),
    executeJavaScript: vi.fn(async () => 'fr-CA')
  } satisfies TabLike
  const activateAt = vi.fn((i: number) => (i < 3 ? { id: 1 } : undefined))
  const extras = new TabsExtras({
    contents: (id) => (id === 7 ? tab : undefined),
    active: () => tab,
    access: () => opts.access ?? { permissions: [], origins: ['<all_urls>'] },
    readFile: async (_c, file) => `/* ${file} */`,
    activateAt
  })
  return { extras, tab, activateAt }
}

describe('TabsExtras', () => {
  it('captureVisibleTab returns a jpeg data URL by default and png on request', async () => {
    const { extras } = setup()
    expect(await extras.capture(caller, undefined, { quality: 10 })).toBe(`data:image/jpeg;base64,${Buffer.from('jpeg10').toString('base64')}`)
    expect(await extras.capture(caller, 1, { format: 'png' })).toBe('data:image/png;base64,PNG')
  })

  it('captureVisibleTab needs <all_urls>, activeTab or a host pattern of the page', async () => {
    await expect(setup({ access: { permissions: ['tabs'], origins: [] } }).extras.capture(caller, undefined, undefined)).rejects.toThrow(/permission/)
    await expect(setup({ access: { permissions: ['activeTab'], origins: [] } }).extras.capture(caller, undefined, undefined)).resolves.toMatch(
      /^data:image\/jpeg/
    )
    await expect(setup({ access: { permissions: [], origins: ['https://a.test/*'] } }).extras.capture(caller, undefined, undefined)).resolves.toMatch(/^data:/)
  })

  it('insertCSS then removeCSS removes the keys of the same source only', async () => {
    const { extras, tab } = setup()
    await extras.insertCSS(caller, 7, { code: 'a{}' })
    await extras.insertCSS(caller, 7, { file: 'x.css' })
    await extras.removeCSS(caller, 7, { code: 'a{}' })
    expect(tab.insertCSS).toHaveBeenNthCalledWith(2, '/* x.css */', { cssOrigin: 'author' })
    expect(tab.removeInsertedCSS).toHaveBeenCalledTimes(1)
    expect(tab.removeInsertedCSS).toHaveBeenCalledWith('key1')
    await extras.removeCSS(caller, { file: 'x.css' }, undefined)
    expect(tab.removeInsertedCSS).toHaveBeenLastCalledWith('key2')
  })

  it('insertCSS is refused on a page the extension cannot access', async () => {
    await expect(setup({ access: { permissions: [], origins: [] } }).extras.insertCSS(caller, 7, { code: 'a{}' })).rejects.toThrow(/Cannot access/)
  })

  it('detectLanguage reads the page language', async () => {
    expect(await setup().extras.detectLanguage(caller, 7)).toBe('fr')
    const { extras, tab } = setup()
    tab.executeJavaScript.mockRejectedValueOnce(new Error('x'))
    expect(await extras.detectLanguage(caller, undefined)).toBe('und')
  })

  it('highlight activates the first index and fails when there is no such tab', () => {
    const { extras, activateAt } = setup()
    expect(extras.highlight(caller, { tabs: [1, 2] })).toEqual({ id: 1 })
    expect(activateAt).toHaveBeenCalledWith(1, undefined)
    expect(() => extras.highlight(caller, { tabs: 9 })).toThrow(/index/)
    expect(() => extras.highlight(caller, { tabs: [] })).toThrow()
  })
})

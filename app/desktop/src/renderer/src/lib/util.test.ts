import { afterEach, describe, expect, it, vi } from 'vitest'
import { barStyle, domainOf, isSecure, languageName, luminance, share, themeBackground } from './util'

describe('domainOf', () => {
  it('strips www. and keeps the hostname', () => {
    expect(domainOf('https://www.example.com/a?b')).toBe('example.com')
    expect(domainOf('http://sub.example.com:8080/')).toBe('sub.example.com')
  })

  it('shows internal addresses in full, without a trailing /', () => {
    expect(domainOf('signet://settings/')).toBe('signet://settings')
    expect(domainOf('SIGNET://note/abc')).toBe('SIGNET://note/abc')
  })

  it('returns the string as is if it is not an address', () => {
    expect(domainOf('pas une url')).toBe('pas une url')
    expect(domainOf('')).toBe('')
  })

  it('returns the address when there is no host', () => {
    expect(domainOf('file:///tmp/x.html')).toBe('file:///tmp/x.html')
  })
})

describe('isSecure', () => {
  it('vrai pour https seulement', () => {
    expect(isSecure('https://a.com')).toBe(true)
    expect(isSecure('http://a.com')).toBe(false)
    expect(isSecure('signet://x')).toBe(false)
  })
})

describe('themeBackground', () => {
  it('gives a solid color without gradient', () => {
    expect(themeBackground(undefined, 200)).toBe('hsl(200 70% 62%)')
    expect(themeBackground({ hues: [90] } as never, 200)).toBe('hsl(90 70% 62%)')
  })

  it('gives a gradient with at least two hues', () => {
    expect(themeBackground({ hues: [10, 20, 30] } as never, 0)).toBe('linear-gradient(135deg, hsl(10 70% 62%), hsl(20 70% 62%), hsl(30 70% 62%))')
  })
})

describe('luminance', () => {
  it('is 0 for black and 1 for white', () => {
    expect(luminance('#000000')).toBe(0)
    expect(luminance('#ffffff')).toBeCloseTo(1, 10)
  })

  it('suit les coefficients WCAG', () => {
    expect(luminance('#ff0000')).toBeCloseTo(0.2126, 4)
    expect(luminance('#00ff00')).toBeCloseTo(0.7152, 4)
    expect(luminance('#0000ff')).toBeCloseTo(0.0722, 4)
  })

  it('accepts any case and ignores an optional alpha', () => {
    expect(luminance('#FFFFFF')).toBeCloseTo(1, 10)
    expect(luminance('#ffffff80')).toBeCloseTo(1, 10)
  })

  it('returns null if the color cannot be parsed', () => {
    expect(luminance('red')).toBeNull()
    expect(luminance('#fff')).toBeNull()
    expect(luminance('')).toBeNull()
  })
})

describe('barStyle', () => {
  it('sets nothing without a readable color', () => {
    expect(barStyle(undefined)).toBeUndefined()
    expect(barStyle('')).toBeUndefined()
    expect(barStyle('rgb(0 0 0)')).toBeUndefined()
  })

  it('puts light text on a dark background', () => {
    const s = barStyle('#101010') as Record<string, string>
    expect(s['--bar-bg']).toBe('#101010')
    expect(s['--bar-fg']).toContain('255 255 255')
    expect(s['--bar-hover']).toBe('rgb(255 255 255 / .1)')
  })

  it('puts dark text on a light background', () => {
    const s = barStyle('#f5f5f5') as Record<string, string>
    expect(s['--bar-fg']).toContain('rgb(0 0 0')
    expect(s['--bar-fg-2']).toBe('rgb(0 0 0 / var(--bar-fg-2-alpha))')
  })
})

describe('languageName', () => {
  it('gives the name in the requested language', () => {
    expect(languageName('de', 'en')).toBe('German')
    expect(languageName('de', 'fr')).toBe('allemand')
  })

  it('returns the code as is if it is invalid', () => {
    expect(languageName('!!', 'en')).toBe('!!')
  })
})

describe('share (partage structurel)', () => {
  it('keeps the identity of prev when everything is equal', () => {
    const prev = { a: 1, b: [1, 2, { c: 3 }], d: { e: 'x' } }
    const next = JSON.parse(JSON.stringify(prev))
    expect(share(prev, next)).toBe(prev)
  })

  it('returns next for primitives and differing types', () => {
    expect(share(1, 2)).toBe(2)
    expect(share(undefined, 5)).toBe(5)
    expect(share(null, null)).toBeNull()
    const arr = [1]
    expect(share({ a: 1 }, arr)).toBe(arr)
    const obj = { a: 1 }
    expect(share([1], obj)).toBe(obj)
    expect(share('x', obj)).toBe(obj)
  })

  it('returns the same next if it is identical to prev', () => {
    const o = { a: 1 }
    expect(share(o, o)).toBe(o)
  })

  it('only changes the modified branch, the rest keeps its identity', () => {
    const prev = { tabs: { a: { t: 1 }, b: { t: 2 } }, list: [{ id: 1 }, { id: 2 }], same: { x: 1 } }
    const next = { tabs: { a: { t: 1 }, b: { t: 3 } }, list: [{ id: 1 }, { id: 2 }], same: { x: 1 } }
    const out = share(prev, next)
    expect(out).toEqual(next)
    expect(out).not.toBe(prev)
    expect(out.tabs).not.toBe(prev.tabs)
    expect(out.tabs.a).toBe(prev.tabs.a)
    expect(out.tabs.b).not.toBe(prev.tabs.b)
    expect(out.list).toBe(prev.list)
    expect(out.same).toBe(prev.same)
  })

  it('ne mute ni prev ni next', () => {
    const prev = { a: { x: 1 }, b: [1, 2] }
    const next = { a: { x: 2 }, b: [1, 2, 3] }
    const p = JSON.stringify(prev)
    const n = JSON.stringify(next)
    share(prev, next)
    expect(JSON.stringify(prev)).toBe(p)
    expect(JSON.stringify(next)).toBe(n)
  })

  describe('tableaux', () => {
    it('same content: keeps prev', () => {
      const prev = [{ a: 1 }, { a: 2 }]
      expect(share(prev, [{ a: 1 }, { a: 2 }])).toBe(prev)
    })

    it('changed element: new array, equal elements shared', () => {
      const prev = [{ a: 1 }, { a: 2 }, { a: 3 }]
      const out = share(prev, [{ a: 1 }, { a: 9 }, { a: 3 }])
      expect(out).toEqual([{ a: 1 }, { a: 9 }, { a: 3 }])
      expect(out).not.toBe(prev)
      expect(out[0]).toBe(prev[0])
      expect(out[2]).toBe(prev[2])
    })

    it('shorter array: equal prefix, returns a slice of prev', () => {
      const prev = [{ a: 1 }, { a: 2 }, { a: 3 }]
      const out = share(prev, [{ a: 1 }, { a: 2 }])
      expect(out).toEqual([{ a: 1 }, { a: 2 }])
      expect(out).not.toBe(prev)
      expect(out[0]).toBe(prev[0])
    })

    it('tableau plus long : ajoute la fin', () => {
      const prev = [{ a: 1 }]
      const out = share(prev, [{ a: 1 }, { a: 2 }])
      expect(out).toEqual([{ a: 1 }, { a: 2 }])
      expect(out[0]).toBe(prev[0])
    })

    it('reordering: the result equals next', () => {
      const prev = [1, 2, 3]
      expect(share(prev, [3, 2, 1])).toEqual([3, 2, 1])
    })

    it('tableau vide', () => {
      const prev: number[] = []
      expect(share(prev, [])).toBe(prev)
      expect(share([1], [])).toEqual([])
    })
  })

  describe('objets', () => {
    it('key added', () => {
      const prev = { a: { x: 1 } }
      const out = share(prev, { a: { x: 1 }, b: 2 })
      expect(out).toEqual({ a: { x: 1 }, b: 2 })
      expect(out.a).toBe(prev.a)
    })

    it('key removed', () => {
      const prev = { a: { x: 1 }, b: 2 }
      const next = { a: { x: 1 } }
      const out = share(prev, next)
      expect(out).toEqual({ a: { x: 1 } })
      expect(Object.keys(out)).toEqual(['a'])
      expect(out.a).toBe(prev.a)
    })

    it('key added with undefined value: the result does have the key', () => {
      const out = share({ a: 1 }, { a: 1, b: undefined }) as Record<string, unknown>
      expect('b' in out).toBe(true)
    })

    it('value going from null to object', () => {
      const out = share({ a: null }, { a: { x: 1 } })
      expect(out).toEqual({ a: { x: 1 } })
    })
  })
})

describe('shortcut / wantsNewTab depending on the system', () => {
  const proc = (globalThis as unknown as { process: object }).process
  const real = Object.getOwnPropertyDescriptor(proc, 'platform')!
  afterEach(() => {
    Object.defineProperty(proc, 'platform', real)
    vi.resetModules()
  })

  async function loadOn(platform: string) {
    Object.defineProperty(proc, 'platform', { value: platform, configurable: true })
    vi.resetModules()
    return import('./util')
  }

  it('Mac : symboles ⌥⇧⌘', async () => {
    const u = await loadOn('darwin')
    expect(u.IS_MAC).toBe(true)
    expect(u.shortcut('T')).toBe('⌘T')
    expect(u.shortcut('T', 'shift')).toBe('⇧⌘T')
    expect(u.shortcut('T', 'alt', 'shift')).toBe('⌥⇧⌘T')
  })

  it('Windows et Linux : Ctrl+Alt+Shift+touche', async () => {
    for (const p of ['win32', 'linux']) {
      const u = await loadOn(p)
      expect(u.IS_MAC).toBe(false)
      expect(u.shortcut('T')).toBe('Ctrl+T')
      expect(u.shortcut('T', 'shift', 'alt')).toBe('Ctrl+Alt+Shift+T')
    }
  })

  it('wantsNewTab : ⌘clic sur Mac, Ctrl+clic ailleurs, clic milieu partout', async () => {
    const mac = await loadOn('darwin')
    expect(mac.wantsNewTab({ metaKey: true, ctrlKey: false, button: 0 })).toBe(true)
    expect(mac.wantsNewTab({ metaKey: false, ctrlKey: true, button: 0 })).toBe(false)
    expect(mac.wantsNewTab({ metaKey: false, ctrlKey: false, button: 1 })).toBe(true)
    const win = await loadOn('win32')
    expect(win.wantsNewTab({ metaKey: false, ctrlKey: true, button: 0 })).toBe(true)
    expect(win.wantsNewTab({ metaKey: true, ctrlKey: false, button: 0 })).toBe(false)
    expect(win.wantsNewTab({ metaKey: false, ctrlKey: false, button: 1 })).toBe(true)
  })
})

import { describe, expect, it } from 'vitest'
import { cleanSpaceDraft, newSpace, newTab } from './model'

describe('newSpace / newTab / cleanSpaceDraft', () => {
  it('newSpace / newTab', () => {
    const s = newSpace({ name: 'N', icon: 'i', hue: 3 })
    expect(s).toMatchObject({ name: 'N', icon: 'i', hue: 3, pinned: [], today: [], activeTabId: null })
    expect(s.id).toMatch(/^[0-9a-f-]{36}$/)
    const t = newTab('https://a.com', 'A')
    expect(t).toMatchObject({ url: 'https://a.com', title: 'A', loading: false, loaded: false, muted: false })
    expect(newTab('x').title).toBe('')
  })

  it('cleanSpaceDraft: default values and bounds', () => {
    expect(cleanSpaceDraft({} as never, 'Personal')).toEqual({ name: 'Personal', icon: '✨', hue: 250 })
    expect(cleanSpaceDraft({ name: '  Perso  ', icon: ' 🏠 ', hue: -10 } as never, 'Personal')).toEqual({ name: 'Perso', icon: '🏠', hue: 350 })
    expect(cleanSpaceDraft({ name: 'x'.repeat(100), icon: 'i', hue: 725.6 } as never, 'Personal').name).toHaveLength(40)
    expect(cleanSpaceDraft({ name: 'a', icon: 'i', hue: 725.6 } as never, 'Personal').hue).toBe(6)
    expect(cleanSpaceDraft({ name: 'a', icon: 'i', hue: NaN } as never, 'Personal').hue).toBe(250)
  })

  it('cleanSpaceDraft: gradient of 2 or 3 hues only', () => {
    const d = cleanSpaceDraft({ name: 'a', icon: 'i', hue: 1, theme: { hues: [10, 370, 30, 40], grain: 5 } } as never, 'Personal')
    expect(d).toEqual({ name: 'a', icon: 'i', hue: 10, theme: { hues: [10, 10, 30], grain: 1 } })
    const one = cleanSpaceDraft({ name: 'a', icon: 'i', hue: 99, theme: { hues: [10], grain: 0.5 } } as never, 'Personal')
    expect(one).toEqual({ name: 'a', icon: 'i', hue: 99 })
    expect(cleanSpaceDraft({ name: 'a', icon: 'i', hue: 1, theme: { hues: [1, 2], grain: 'x' } } as never, 'Personal').theme!.grain).toBe(0)
  })
})

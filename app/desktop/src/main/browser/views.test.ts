import { describe, expect, it, vi } from 'vitest'

vi.mock('electron', () => ({}))
vi.mock('../web/profiles', () => ({ sessionFor: () => ({}) }))
vi.mock('../web/internal', () => ({ resolveInternal: (u: string) => u }))
vi.mock('../core/store', () => ({ trimHistory: () => null }))

import { Views } from './views'

function fakeView() {
  const wc = {
    id: Math.random(),
    destroyed: false,
    isDestroyed: () => wc.destroyed,
    close: () => (wc.destroyed = true),
    setAudioMuted: () => {},
    getURL: () => 'x',
    isLoading: () => false,
    loadURL: () => Promise.resolve()
  }
  return { webContents: wc }
}

function make(opts: { detached?: string[]; privates?: string[]; keep?: string[] } = {}) {
  const created: string[] = []
  const tabs = Object.fromEntries(['a', 'b', 'c', 'd'].map((id) => [id, { id, url: `https://${id}.com/`, loaded: false, muted: false }]))
  const store = { state: { tabs, spaces: [], favorites: [] }, histories: new Map(), detached: new Set(opts.detached ?? []) }
  const views = new Views({
    store: store as never,
    window: { win: null },
    create: (id) => {
      created.push(id)
      return fakeView() as never
    },
    extensions: { track: () => {}, whenReady: () => null } as never,
    changed: () => {},
    isPrivate: (id) => !!opts.privates?.includes(id),
    keep: (id) => !!opts.keep?.includes(id)
  })
  return { views, created }
}

describe('Views stale marking', () => {
  it('replaces a stale view when it is next asked for, others stay', () => {
    const { views, created } = make()
    const a1 = views.ensureView('a')
    const b1 = views.ensureView('b')
    expect(created).toEqual(['a', 'b'])
    views.markStale()
    // not asked for yet: nothing is recreated in the background
    expect(created).toEqual(['a', 'b'])
    expect(views.viewOf('a')).toBe(a1)
    // the displayed tab is asked for by the layout
    const a2 = views.ensureView('a')
    expect(a2).not.toBe(a1)
    expect(a1.webContents.isDestroyed()).toBe(true)
    expect(created).toEqual(['a', 'b', 'a'])
    expect(views.ensureView('a')).toBe(a2) // fresh now
    // the hidden one is replaced when it is finally shown
    expect(views.ensureView('b')).not.toBe(b1)
    expect(created).toEqual(['a', 'b', 'a', 'b'])
  })
  it('does not mark detached tabs (Little Signet, apps, Peek), but marks private window tabs', () => {
    const { views } = make({ detached: ['b', 'c'], privates: ['c'] })
    for (const id of ['a', 'b', 'c']) views.ensureView(id)
    expect(views.markStale().sort()).toEqual(['a', 'c'])
  })
  it('keeps a view that must not be replaced (mini player, developer tools) and replaces it later', () => {
    const keep = ['a']
    const { views, created } = make({ keep })
    const a1 = views.ensureView('a')
    views.markStale()
    expect(views.ensureView('a')).toBe(a1)
    keep.length = 0
    expect(views.ensureView('a')).not.toBe(a1)
    expect(created).toEqual(['a', 'a'])
  })
  it('a tab that sleeps while stale wakes with a plain new view and is no longer stale', () => {
    const { views, created } = make()
    views.ensureView('a')
    views.markStale()
    views.destroyView('a')
    const a2 = views.ensureView('a')
    expect(views.ensureView('a')).toBe(a2)
    expect(created).toEqual(['a', 'a'])
  })
})

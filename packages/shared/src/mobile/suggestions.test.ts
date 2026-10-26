import { describe, expect, it } from 'vitest'
import { newTab } from '../model'
import { addBookmark } from './bookmarks'
import { addVisit } from './history'
import { suggest, withRemoteSuggestions } from './suggestions'

const NOW = 1_000_000
const tabs = [{ ...newTab('https://docs.rs/serde', 'Serde docs'), lastActive: 5 }, newTab('', '')]
const bookmarks = addBookmark([], { title: 'Rust book', url: 'https://doc.rust-lang.org/book' })
const history = addVisit(addVisit([], { url: 'https://docs.rs/serde', title: 'Serde docs' }, NOW), { url: 'https://rustup.rs', title: 'Rustup' }, NOW)
const src = { tabs, bookmarks, history, engine: 'duckduckgo' as const, now: NOW }

describe('suggest', () => {
  it('empty input: non-empty open tabs only', () => {
    const out = suggest('', src)
    expect(out).toHaveLength(1)
    expect(out[0]).toMatchObject({ kind: 'tab', url: 'https://docs.rs/serde' })
  })
  it('a phrase: search first, then matches', () => {
    const out = suggest('rust', src)
    expect(out[0]).toMatchObject({ kind: 'search', url: 'https://duckduckgo.com/?q=rust' })
    expect(out.map((s) => s.kind)).toEqual(['search', 'bookmark', 'history'])
  })
  it('an open tab hides the same page from history', () => {
    const out = suggest('serde', src)
    expect(out.filter((s) => s.kind !== 'search').map((s) => s.kind)).toEqual(['tab'])
  })
  it('an address: the address first, search last', () => {
    const out = suggest('rustup.rs', src)
    expect(out[0]).toEqual({ kind: 'url', url: 'https://rustup.rs' })
    expect(out.at(-1)?.kind).toBe('search')
  })
  it('respects the limit', () => {
    expect(suggest('r', src, 2)).toHaveLength(2)
  })
})

describe('withRemoteSuggestions', () => {
  it('adds engine suggestions below local ones, no duplicates, at most max', () => {
    const local = suggest('rust', src)
    const out = withRemoteSuggestions(local, ['rust', 'Rust lang', 'rust book', 'rustup', 'rust game', 'rust ide', 'rust x'], 'duckduckgo', 3)
    expect(out.slice(0, local.length)).toEqual(local)
    const remote = out.slice(local.length)
    expect(remote.map((s) => (s.kind === 'search' ? s.query : ''))).toEqual(['Rust lang', 'rust book', 'rustup'])
    expect(remote.every((s) => s.kind === 'search' && s.remote && s.url.startsWith('https://duckduckgo.com/'))).toBe(true)
  })
  it('nothing to add: local as is', () => {
    const local = suggest('rust', src)
    expect(withRemoteSuggestions(local, [], 'google')).toEqual(local)
  })
})

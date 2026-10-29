import { existsSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const env = vi.hoisted(() => ({ dir: '' }))
vi.mock('electron', () => ({ app: { getPath: () => env.dir } }))

import { History } from './history'

let dir: string
beforeEach(() => {
  dir = mkdtempSync(join(tmpdir(), 'signet-history-'))
  env.dir = dir
})
afterEach(() => {
  vi.useRealTimers()
  rmSync(dir, { recursive: true, force: true, maxRetries: 5, retryDelay: 50 }) // a background write may still be finishing
})

const urls = (list: { url: string }[]) => list.map((e) => e.url)

describe('History.record', () => {
  it('ignores about:, data:, devtools: and the empty address', () => {
    const h = new History()
    h.record('', 't')
    h.record('about:blank', 't')
    h.record('data:text/html,hi', 't')
    h.record('devtools://devtools/x', 't')
    expect(h.size).toBe(0)
  })

  it('accumulates visits, keeps the old title and moves to the front', () => {
    const h = new History()
    h.record('https://a.com', 'A')
    h.record('https://b.com', 'B')
    h.record('https://a.com', '')
    const [first] = h.list('', 0, 10)
    expect(first).toMatchObject({ url: 'https://a.com', title: 'A', visits: 2 })
    expect(urls(h.list('', 0, 10))).toEqual(['https://a.com', 'https://b.com'])
  })

  it('default title = URL', () => {
    const h = new History()
    h.record('https://a.com', '')
    expect(h.list('', 0, 1)[0].title).toBe('https://a.com')
  })

  it('caps at 5000 entries by removing the oldest', () => {
    const h = new History()
    for (let i = 0; i < 5001; i++) h.record(`https://x.com/${i}`, '')
    expect(h.size).toBe(5000)
    expect(h.list('', 4999, 1)[0].url).toBe('https://x.com/1')
  })

  it('large data: favicons are not kept', () => {
    const h = new History()
    h.record('https://a.com', 'A', 'data:image/png;base64,' + 'x'.repeat(200_001))
    h.record('https://b.com', 'B', 'https://b.com/favicon.ico')
    expect(h.list('a.com', 0, 1)[0].favicon).toBeUndefined()
    expect(h.list('b.com', 0, 1)[0].favicon).toBe('https://b.com/favicon.ico')
  })
})

describe('History.list / search', () => {
  const fill = () => {
    const h = new History()
    h.record('https://cafe.com', 'Le Café')
    h.record('https://github.com/x', 'GitHub')
    h.record('https://example.org', 'Exemple')
    return h
  }

  it('list: most recent first, pagination', () => {
    const h = fill()
    expect(urls(h.list('', 0, 10))).toEqual(['https://example.org', 'https://github.com/x', 'https://cafe.com'])
    expect(urls(h.list('', 1, 1))).toEqual(['https://github.com/x'])
  })

  it('list: filter ignoring accents and case, on title and URL', () => {
    const h = fill()
    expect(urls(h.list('CAFE', 0, 10))).toEqual(['https://cafe.com'])
    expect(urls(h.list('le café', 0, 10))).toEqual(['https://cafe.com'])
    expect(urls(h.list('github.com', 0, 10))).toEqual(['https://github.com/x'])
    expect(h.list('zzz', 0, 10)).toEqual([])
  })

  it('search: exclusions and limit', () => {
    const h = fill()
    expect(urls(h.search('o', new Set(['https://cafe.com']), 10)).sort()).toEqual(['https://example.org', 'https://github.com/x'].sort())
    expect(h.search('o', new Set(), 1)).toHaveLength(1)
    expect(h.search('o', new Set(), 0)).toEqual([])
  })

  it('search: most visited pages first', () => {
    const h = new History()
    h.record('https://a.com/1', 'doc')
    h.record('https://a.com/2', 'doc')
    h.record('https://a.com/1', 'doc')
    h.record('https://a.com/1', 'doc')
    expect(urls(h.search('doc', new Set(), 5))).toEqual(['https://a.com/1', 'https://a.com/2'])
  })

  it('search: a host prefix beats a match in the path or title, even if less visited', () => {
    const h = new History()
    h.record('https://blog.com/why-youtube-is-big', 'Why youtube is big')
    h.record('https://blog.com/why-youtube-is-big', 'Why youtube is big')
    h.record('https://blog.com/why-youtube-is-big', 'Why youtube is big')
    h.record('https://www.youtube.com/', 'YouTube')
    expect(urls(h.search('yout', new Set(), 5))).toEqual(['https://www.youtube.com/', 'https://blog.com/why-youtube-is-big'])
  })

  it('search: frequency and recency both count', () => {
    const h = new History()
    h.importEntries([
      { url: 'https://b.com/stale', title: 'doc', visits: 1, lastVisit: Date.now() - 400 * 86_400_000 },
      { url: 'https://b.com/fresh', title: 'doc', visits: 1, lastVisit: Date.now() }
    ])
    expect(urls(h.search('doc', new Set(), 5))).toEqual(['https://b.com/fresh', 'https://b.com/stale'])
  })

  it('update changes the title (search follows) and remove removes', () => {
    const h = new History()
    h.record('https://a.com', 'Ancien')
    h.update('https://a.com', { title: 'Nouveau' })
    expect(h.search('nouveau', new Set(), 5)).toHaveLength(1)
    expect(h.search('ancien', new Set(), 5)).toHaveLength(0)
    h.remove('https://a.com')
    expect(h.size).toBe(0)
  })
})

describe('History.importEntries', () => {
  it('adds new pages, merges known ones, ignores invalid ones', () => {
    const h = new History()
    h.record('https://a.com', '')
    const known = h.list('', 0, 1)[0]
    const t = known.lastVisit
    const added = h.importEntries([
      { url: 'https://a.com', title: 'Titre A', visits: 3, lastVisit: t + 1000 },
      { url: 'https://b.com', title: '', visits: 0, lastVisit: 5 },
      { url: 'about:blank', title: 'x', visits: 1, lastVisit: 1 },
      { url: 'https://c.com', title: 'C', visits: 1, lastVisit: NaN }
    ])
    expect(added).toBe(1)
    expect(h.size).toBe(2)
    expect(h.list('a.com', 0, 1)[0]).toMatchObject({ title: 'Titre A', visits: 4, lastVisit: t + 1000 })
    expect(h.list('b.com', 0, 1)[0]).toMatchObject({ title: 'https://b.com', visits: 1, lastVisit: 5 })
    // Recency order rebuilt: b (5) is the oldest
    expect(urls(h.list('', 0, 10))).toEqual(['https://a.com', 'https://b.com'])
  })
})

describe('History: file', () => {
  const path = () => join(dir, 'history.json')

  it('flush writes, a new object reads back', () => {
    const h = new History()
    h.record('https://a.com', 'A')
    h.flush()
    const again = new History()
    expect(urls(again.list('', 0, 10))).toEqual(['https://a.com'])
  })

  it('clear empties the file', () => {
    const h = new History()
    h.record('https://a.com', 'A')
    h.flush()
    h.clear()
    expect(JSON.parse(readFileSync(path(), 'utf8'))).toEqual([])
  })

  it('corrupt file: copy aside, empty history', () => {
    writeFileSync(path(), '{oops')
    const h = new History()
    expect(h.size).toBe(0)
    expect(readdirSync(dir).filter((f) => f.startsWith('history.json.corrupt-'))).toHaveLength(1)
  })

  it('invalid entries in the file ignored', () => {
    writeFileSync(path(), JSON.stringify([{ url: 'https://a.com', title: 'A', visits: 1, lastVisit: 1 }, null, { title: 'sans url' }, 3]))
    expect(new History().size).toBe(1)
  })

  it('without prior load, flush does not create a file', () => {
    new History().flush()
    expect(existsSync(path())).toBe(false)
  })

  it('deferred write fires after the delay', async () => {
    vi.useFakeTimers()
    const h = new History()
    h.record('https://a.com', 'A')
    vi.advanceTimersByTime(2001)
    vi.useRealTimers()
    // The write itself is async: poll for the file instead of a fixed wait.
    await vi.waitFor(() => expect(existsSync(path())).toBe(true), { timeout: 5000 })
  })
})

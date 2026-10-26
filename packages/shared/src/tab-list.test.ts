import { describe, expect, it } from 'vitest'
import { activeTab, emptyTabList, parseTabList, serializeTabList, tabListReducer, type TabList } from './tab-list'

const open = (s: TabList, url: string) => tabListReducer(s, { type: 'open', url })

describe('tabListReducer', () => {
  it('open adds a tab and activates it', () => {
    const s = open(open(emptyTabList, 'https://a.com'), 'https://b.com')
    expect(s.tabs.map((t) => t.url)).toEqual(['https://a.com', 'https://b.com'])
    expect(s.activeId).toBe(s.tabs[1].id)
    expect(activeTab(s)?.url).toBe('https://b.com')
  })

  it('close: right neighbor, otherwise left, otherwise nothing', () => {
    let s = open(open(open(emptyTabList, 'a'), 'b'), 'c')
    const [a, b, c] = s.tabs.map((t) => t.id)
    s = tabListReducer(s, { type: 'activate', id: b })
    s = tabListReducer(s, { type: 'close', id: b })
    expect(s.activeId).toBe(c)
    s = tabListReducer(s, { type: 'close', id: c })
    expect(s.activeId).toBe(a)
    s = tabListReducer(s, { type: 'close', id: a })
    expect(s).toEqual(emptyTabList)
    expect(activeTab(s)).toBeNull()
  })

  it('closing an inactive tab keeps the active one; an unknown id changes nothing', () => {
    const s = open(open(emptyTabList, 'a'), 'b')
    const [a, b] = s.tabs.map((t) => t.id)
    expect(tabListReducer(s, { type: 'close', id: a }).activeId).toBe(b)
    expect(tabListReducer(s, { type: 'close', id: 'nope' })).toBe(s)
    expect(tabListReducer(s, { type: 'activate', id: 'nope' })).toBe(s)
  })

  it('update merges without touching the id', () => {
    const s = open(emptyTabList, 'a')
    const id = s.tabs[0].id
    const u = tabListReducer(s, { type: 'update', id, patch: { url: 'https://x.com', title: 'X', id: 'other' } })
    expect(u.tabs[0]).toMatchObject({ id, url: 'https://x.com', title: 'X' })
    expect(tabListReducer(s, { type: 'update', id: 'nope', patch: { title: 'X' } })).toBe(s)
  })

  it('closeAll and restore', () => {
    const s = open(emptyTabList, 'a')
    expect(tabListReducer(s, { type: 'closeAll' })).toEqual(emptyTabList)
    expect(tabListReducer(emptyTabList, { type: 'restore', list: s })).toBe(s)
  })
})

describe('serializeTabList / parseTabList', () => {
  it('aller-retour', () => {
    const s = open(open(emptyTabList, 'https://a.com'), 'https://b.com')
    const back = parseTabList(JSON.parse(JSON.stringify(serializeTabList(s))))
    expect(back.activeId).toBe(s.activeId)
    expect(back.tabs.map((t) => [t.id, t.url, t.loading])).toEqual(s.tabs.map((t) => [t.id, t.url, false]))
  })

  it('ignores malformed data', () => {
    expect(parseTabList(null)).toEqual(emptyTabList)
    expect(parseTabList({ tabs: 'x' })).toEqual(emptyTabList)
    const p = parseTabList({ tabs: [null, { id: 1 }, { id: 'a', url: 'u' }, { id: 'a', url: 'dup' }, { id: 'b', url: 'v', title: 3 }], activeId: 'zz' })
    expect(p.tabs.map((t) => t.id)).toEqual(['a', 'b'])
    expect(p.tabs[1].title).toBe('')
    expect(p.activeId).toBe('b')
  })
})

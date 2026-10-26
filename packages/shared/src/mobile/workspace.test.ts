import { describe, expect, it } from 'vitest'
import {
  activateTab,
  activeSpace,
  addSpace,
  adjacentSpace,
  closePrivateTabs,
  closeTab,
  closeTodayTabs,
  createWorkspace,
  currentTab,
  editSpace,
  favoriteTab,
  isPrivateTab,
  MAX_SPACES,
  moveSpace,
  moveTab,
  openTab,
  parseWorkspace,
  pinTab,
  removeSpace,
  reopenTab,
  serializeWorkspace,
  setActiveSpace,
  setPrivateMode,
  spaceTabIds,
  tabLocation,
  updateTab,
  visibleTabIds,
  type Workspace
} from './workspace'

const fresh = () => createWorkspace('Perso')
const urls = (ws: Workspace) => visibleTabIds(ws).map((id) => ws.tabs[id].url)

describe('workspace: tabs', () => {
  it('starts with one space and an active empty tab', () => {
    const ws = fresh()
    expect(ws.spaces).toHaveLength(1)
    expect(visibleTabIds(ws)).toHaveLength(1)
    expect(currentTab(ws)?.url).toBe('')
  })

  it('openTab appends to the end of the day tabs and activates', () => {
    const ws = openTab(openTab(fresh(), { url: 'https://a.com' }), { url: 'https://b.com' })
    expect(urls(ws)).toEqual(['', 'https://a.com', 'https://b.com'])
    expect(currentTab(ws)?.url).toBe('https://b.com')
    const bg = openTab(ws, { url: 'https://c.com', activate: false })
    expect(currentTab(bg)?.url).toBe('https://b.com')
    expect(urls(bg).at(-1)).toBe('https://c.com')
  })

  it('closeTab: right neighbor then left; closing the last opens an empty tab', () => {
    let ws = openTab(openTab(fresh(), { url: 'a' }), { url: 'b' })
    const [t0, ta, tb] = visibleTabIds(ws)
    ws = activateTab(ws, ta)
    ws = closeTab(ws, ta)
    expect(currentTab(ws)?.id).toBe(tb)
    ws = closeTab(ws, tb)
    expect(currentTab(ws)?.id).toBe(t0)
    ws = closeTab(ws, t0)
    expect(visibleTabIds(ws)).toHaveLength(1)
    expect(currentTab(ws)?.url).toBe('')
    expect(closeTab(ws, 'inconnu')).toBe(ws)
  })

  it('closing an inactive tab keeps the active one', () => {
    let ws = openTab(openTab(fresh(), { url: 'a' }), { url: 'b' })
    const [t0] = visibleTabIds(ws)
    const active = currentTab(ws)!.id
    ws = closeTab(ws, t0)
    expect(currentTab(ws)?.id).toBe(active)
    expect(Object.keys(ws.tabs)).not.toContain(t0)
  })

  it('updateTab does not change the id', () => {
    let ws = fresh()
    const id = currentTab(ws)!.id
    ws = updateTab(ws, id, { url: 'https://a.com', title: 'A', id: 'x' } as never)
    expect(ws.tabs[id]).toMatchObject({ id, url: 'https://a.com', title: 'A' })
    expect(updateTab(ws, 'nope', { title: 'x' })).toBe(ws)
  })

  it('pin, favorite, unpin', () => {
    let ws = openTab(fresh(), { url: 'https://a.com' })
    const id = currentTab(ws)!.id
    ws = pinTab(ws, id)
    expect(activeSpace(ws).pinned).toEqual([id])
    expect(ws.tabs[id].pinnedUrl).toBe('https://a.com')
    ws = favoriteTab(ws, id)
    expect(activeSpace(ws).favorites).toEqual([id])
    expect(activeSpace(ws).pinned).toEqual([])
    expect(spaceTabIds(activeSpace(ws))[0]).toBe(id)
    ws = pinTab(ws, id, false)
    expect(activeSpace(ws).today).toContain(id)
    expect(ws.tabs[id].pinnedUrl).toBeUndefined()
    expect('pinnedUrl' in ws.tabs[id]).toBe(false)
  })

  it('moveTab reorders and changes space', () => {
    let ws = openTab(openTab(fresh(), { url: 'a' }), { url: 'b' })
    const [t0, ta, tb] = visibleTabIds(ws)
    ws = moveTab(ws, tb, { zone: 'today', index: 0 })
    expect(activeSpace(ws).today).toEqual([tb, t0, ta])
    ws = addSpace(ws, { name: 'Travail', icon: '💼', hue: 10 }, 'x')
    const second = activeSpace(ws).id
    const first = ws.spaces[0].id
    ws = moveTab(ws, ta, { zone: 'pinned', spaceId: second })
    expect(tabLocation(ws, ta)).toEqual({ kind: 'space', spaceId: second, zone: 'pinned', index: 0 })
    expect(ws.spaces.find((s) => s.id === first)!.today).not.toContain(ta)
  })

  it('reopenTab puts the tab back in place (undo close)', () => {
    let ws = openTab(openTab(fresh(), { url: 'a' }), { url: 'b' })
    const [, ta] = visibleTabIds(ws)
    const tab = ws.tabs[ta]
    const at = tabLocation(ws, ta)!
    ws = closeTab(ws, ta)
    ws = reopenTab(ws, tab, at)
    expect(visibleTabIds(ws)[1]).toBe(ta)
    expect(currentTab(ws)?.id).toBe(ta)
  })

  it('closeTodayTabs keeps the pinned ones', () => {
    let ws = openTab(openTab(fresh(), { url: 'a' }), { url: 'b' })
    const [, ta] = visibleTabIds(ws)
    ws = pinTab(ws, ta)
    ws = closeTodayTabs(ws)
    expect(activeSpace(ws).pinned).toEqual([ta])
    expect(activeSpace(ws).today).toHaveLength(0)
    expect(currentTab(ws)?.id).toBe(ta)
  })
})

describe('workspace: private', () => {
  it('private tabs are separate, never saved', () => {
    let ws = setPrivateMode(fresh(), true)
    expect(ws.privateMode).toBe(true)
    expect(visibleTabIds(ws)).toHaveLength(1)
    ws = openTab(ws, { url: 'https://secret.com' })
    const id = currentTab(ws)!.id
    expect(isPrivateTab(ws, id)).toBe(true)
    expect(activeSpace(ws).today.includes(id)).toBe(false)
    const stored = JSON.stringify(serializeWorkspace(ws))
    expect(stored).not.toContain('secret.com')
    expect(stored).not.toContain(id)
  })

  it('a new public tab leaves private mode; leaving keeps the private tabs open', () => {
    let ws = setPrivateMode(fresh(), true)
    ws = openTab(ws, { url: 'https://x.com', private: false })
    expect(ws.privateMode).toBe(false)
    ws = setPrivateMode(ws, true)
    expect(ws.priv.tabIds.length).toBe(1)
    ws = setPrivateMode(ws, false)
    expect(ws.priv.tabIds.length).toBe(1)
  })

  it('closing the last private tab leaves private mode; closePrivateTabs clears everything', () => {
    let ws = setPrivateMode(fresh(), true)
    ws = closeTab(ws, ws.priv.tabIds[0])
    expect(ws.privateMode).toBe(false)
    ws = setPrivateMode(ws, true)
    ws = openTab(ws, { url: 'https://y.com' })
    const n = Object.keys(ws.tabs).length
    ws = closePrivateTabs(ws)
    expect(ws.privateMode).toBe(false)
    expect(Object.keys(ws.tabs).length).toBe(n - 2)
  })

  it('private tabs cannot be moved', () => {
    const ws = setPrivateMode(fresh(), true)
    const id = ws.priv.tabIds[0]
    expect(moveTab(ws, id, { zone: 'pinned' })).toBe(ws)
  })

  it('activating a public tab leaves private mode', () => {
    let ws = setPrivateMode(fresh(), true)
    const pub = ws.spaces[0].today[0]
    ws = activateTab(ws, pub)
    expect(ws.privateMode).toBe(false)
  })
})

describe('workspace: spaces', () => {
  it('addSpace creates, activates, caps the count; the draft is sanitized', () => {
    let ws = addSpace(fresh(), { name: '  ', icon: '', hue: 400 }, 'Nouveau')
    expect(ws.spaces).toHaveLength(2)
    expect(activeSpace(ws)).toMatchObject({ name: 'Nouveau', icon: '✨', hue: 40 })
    expect(visibleTabIds(ws)).toHaveLength(1)
    for (let i = 0; i < 20; i++) ws = addSpace(ws, { name: 'x', icon: 'y', hue: 1 }, 'n')
    expect(ws.spaces).toHaveLength(MAX_SPACES)
  })

  it('editSpace: gradient then back to solid color', () => {
    let ws = fresh()
    const id = ws.spaces[0].id
    ws = editSpace(ws, id, { name: 'Maison', icon: '🏠', hue: 10, theme: { hues: [10, 200], grain: 0.5 } })
    expect(ws.spaces[0].theme).toEqual({ hues: [10, 200], grain: 0.5 })
    ws = editSpace(ws, id, { name: 'Maison', icon: '🏠', hue: 10, theme: null })
    expect('theme' in ws.spaces[0]).toBe(false)
  })

  it('removeSpace: refuses the last, deletes the tabs, moves to the neighbor', () => {
    let ws = addSpace(fresh(), { name: 'B', icon: 'b', hue: 1 }, 'n')
    const [a, b] = ws.spaces.map((s) => s.id)
    const bTabs = spaceTabIds(ws.spaces[1])
    ws = removeSpace(ws, b)
    expect(ws.activeSpaceId).toBe(a)
    expect(bTabs.every((t) => !ws.tabs[t])).toBe(true)
    expect(removeSpace(ws, a)).toBe(ws)
  })

  it('setActiveSpace, moveSpace, adjacentSpace', () => {
    let ws = addSpace(addSpace(fresh(), { name: 'B', icon: 'b', hue: 1 }, 'n'), { name: 'C', icon: 'c', hue: 2 }, 'n')
    const [a, b, c] = ws.spaces.map((s) => s.id)
    ws = setActiveSpace(ws, b)
    expect(adjacentSpace(ws, 1)?.id).toBe(c)
    expect(adjacentSpace(ws, -1)?.id).toBe(a)
    expect(adjacentSpace(setActiveSpace(ws, c), 1)).toBeNull()
    ws = moveSpace(ws, c, 0)
    expect(ws.spaces.map((s) => s.id)).toEqual([c, a, b])
    expect(setActiveSpace(ws, 'nope')).toBe(ws)
  })

  it('activating a tab of another space switches space', () => {
    let ws = openTab(fresh(), { url: 'a' })
    const tabInFirst = currentTab(ws)!.id
    ws = addSpace(ws, { name: 'B', icon: 'b', hue: 1 }, 'n')
    expect(ws.activeSpaceId).not.toBe(ws.spaces[0].id)
    ws = activateTab(ws, tabInFirst)
    expect(ws.activeSpaceId).toBe(ws.spaces[0].id)
  })
})

describe('workspace : enregistrement', () => {
  it('aller-retour', () => {
    let ws = openTab(openTab(fresh(), { url: 'https://a.com', title: 'A' }), { url: 'https://b.com' })
    ws = pinTab(ws, currentTab(ws)!.id)
    ws = addSpace(ws, { name: 'B', icon: 'b', hue: 1 }, 'n')
    ws = openTab(ws, { url: 'https://c.com' })
    const back = parseWorkspace(JSON.parse(JSON.stringify(serializeWorkspace(ws))), 'Perso')
    expect(back.spaces.map((s) => spaceTabIds(s))).toEqual(ws.spaces.map((s) => spaceTabIds(s)))
    expect(back.activeSpaceId).toBe(ws.activeSpaceId)
    expect(currentTab(back)?.url).toBe('https://c.com')
    expect(back.tabs[spaceTabIds(back.spaces[0])[0]].pinnedUrl).toBe('https://b.com')
    expect(back.privateMode).toBe(false)
  })

  it('tolerates anything and guarantees a space and a tab', () => {
    for (const bad of [undefined, null, 5, 'x', [], {}, { spaces: 'x', tabs: 3 }]) {
      const ws = parseWorkspace(bad, 'Perso')
      expect(ws.spaces).toHaveLength(1)
      expect(visibleTabIds(ws)).toHaveLength(1)
    }
  })

  it('removes orphan tabs, duplicate ids, unknown ids; fixes the active ones', () => {
    const ws = parseWorkspace(
      {
        spaces: [
          { id: 's1', name: 'A', icon: 'a', hue: 5, favorites: ['t1', 'zz'], pinned: ['t1'], today: ['t2'], activeTabId: 'inconnu' },
          { id: 's1', name: 'doublon', icon: 'a', hue: 5, favorites: [], pinned: [], today: [], activeTabId: null },
          null
        ],
        activeSpaceId: 'nope',
        tabs: [
          { id: 't1', url: 'https://1.com', title: 1, lastActive: 'x' },
          { id: 't2', url: 'https://2.com', title: '2', lastActive: 5 },
          { id: 't3', url: 'https://orphan.com' },
          { id: 't1', url: 'https://dup.com' },
          { url: 'no-id' }
        ]
      },
      'Perso'
    )
    expect(ws.spaces).toHaveLength(1)
    expect(ws.activeSpaceId).toBe('s1')
    expect(Object.keys(ws.tabs).sort()).toEqual(['t1', 't2'])
    expect(ws.spaces[0].favorites).toEqual(['t1'])
    expect(ws.spaces[0].pinned).toEqual([])
    expect(ws.spaces[0].activeTabId).toBe('t2')
    expect(ws.tabs.t1.title).toBe('')
  })
})

describe('workspace: forced id', () => {
  it('openTab accepts the requested id', () => {
    const ws = openTab(fresh(), { url: 'a', id: 'mon-id' })
    expect(ws.tabs['mon-id'].url).toBe('a')
    expect(currentTab(ws)?.id).toBe('mon-id')
  })
})

import { describe, expect, it } from 'vitest'
import {
  addSpace,
  activeSpace,
  canAddSpace,
  canRemoveSpace,
  closeTodayTabsBatch,
  createWorkspace,
  currentTab,
  duplicateTab,
  MAX_SPACES,
  moveTabToSpace,
  openTab,
  pinTab,
  reopenTabs,
  setPrivateMode,
  spaceById,
  tabLocation,
  type Workspace
} from './workspace'

const fresh = () => createWorkspace('Perso')
const today = (ws: Workspace, spaceId = ws.activeSpaceId) => spaceById(ws, spaceId)!.today.map((id) => ws.tabs[id].url)

describe('duplicateTab', () => {
  it('places the copy right after the original without activating it', () => {
    let ws = openTab(fresh(), { url: 'https://a.test', id: 'a' })
    ws = openTab(ws, { url: 'https://b.test', id: 'b' })
    ws = openTab(ws, { url: 'https://c.test', id: 'c' })
    ws = duplicateTab(ws, 'b', 'b2')
    expect(today(ws)).toEqual(['', 'https://a.test', 'https://b.test', 'https://b.test', 'https://c.test'])
    expect(tabLocation(ws, 'b2')).toMatchObject({ zone: 'today', index: 3 })
    expect(currentTab(ws)?.id).toBe('c')
  })

  it('puts the copy of a pinned tab among the day tabs', () => {
    let ws = openTab(fresh(), { url: 'https://a.test', id: 'a' })
    ws = pinTab(ws, 'a')
    ws = duplicateTab(ws, 'a', 'a2')
    expect(tabLocation(ws, 'a2')).toMatchObject({ zone: 'today' })
    expect(ws.tabs.a2.pinnedUrl).toBeUndefined()
  })

  it('keeps a private tab private', () => {
    let ws = setPrivateMode(fresh(), true)
    const first = ws.priv.tabIds[0]
    ws = duplicateTab(ws, first, 'p2')
    expect(ws.priv.tabIds).toEqual([first, 'p2'])
    expect(ws.tabs.p2).toBeDefined()
  })

  it('ignores an unknown tab', () => {
    const ws = fresh()
    expect(duplicateTab(ws, 'nope', 'x')).toBe(ws)
  })
})

describe('moveTabToSpace', () => {
  it('sends a tab to another space keeping its zone', () => {
    let ws = openTab(fresh(), { url: 'https://a.test', id: 'a' })
    ws = pinTab(ws, 'a')
    const first = ws.activeSpaceId
    ws = addSpace(ws, { name: 'Travail', icon: '💼', hue: 10 }, 'x')
    const second = ws.activeSpaceId
    ws = moveTabToSpace(ws, 'a', second)
    expect(tabLocation(ws, 'a')).toMatchObject({ spaceId: second, zone: 'pinned' })
    expect(spaceById(ws, first)!.pinned).toEqual([])
  })

  it('does nothing for the same space or a private tab', () => {
    let ws = openTab(fresh(), { url: 'https://a.test', id: 'a' })
    expect(moveTabToSpace(ws, 'a', ws.activeSpaceId)).toBe(ws)
    ws = setPrivateMode(ws, true)
    expect(moveTabToSpace(ws, ws.priv.tabIds[0], ws.spaces[0].id)).toBe(ws)
  })
})

describe('closeTodayTabsBatch / reopenTabs', () => {
  const build = () => {
    let ws = openTab(fresh(), { url: 'https://a.test', id: 'a' })
    ws = openTab(ws, { url: 'https://b.test', id: 'b' })
    ws = openTab(ws, { url: 'https://p.test', id: 'p' })
    return pinTab(ws, 'p')
  }

  it('closes the day tabs and returns the batch', () => {
    const { workspace, batch } = closeTodayTabsBatch(build())
    expect(batch?.tabs.map((c) => c.tab.id)).toEqual([expect.any(String), 'a', 'b'])
    expect(workspace.tabs.a).toBeUndefined()
    expect(spaceById(workspace, workspace.activeSpaceId)!.pinned).toEqual(['p'])
  })

  it('reopens everything in the same order', () => {
    const before = build()
    const { workspace, batch } = closeTodayTabsBatch(before)
    const back = reopenTabs(workspace, batch!)
    expect(today(back)).toEqual(today(before))
    expect(currentTab(back)?.id).toBe(currentTab(before)?.id)
  })

  it('removes the empty tab opened in its place if it was not used', () => {
    let ws = openTab(fresh(), { url: 'https://a.test', id: 'a' })
    ws = openTab(ws, { url: 'https://b.test', id: 'b' })
    const first = ws.tabs[ws.spaces[0].today[0]]
    const { workspace, batch } = closeTodayTabsBatch(ws)
    expect(batch?.placeholderId).toBeDefined()
    expect(today(workspace)).toEqual([''])
    const back = reopenTabs(workspace, batch!)
    expect(today(back)).toEqual([first.url, 'https://a.test', 'https://b.test'])
    expect(currentTab(back)?.id).toBe('b')
    expect(Object.keys(back.tabs)).toHaveLength(3)
  })

  it('keeps the tab opened in its place if it was used', () => {
    let ws = openTab(fresh(), { url: 'https://a.test', id: 'a' })
    const { workspace, batch } = closeTodayTabsBatch(ws)
    const placeholder = batch!.placeholderId!
    ws = { ...workspace, tabs: { ...workspace.tabs, [placeholder]: { ...workspace.tabs[placeholder], url: 'https://z.test' } } }
    expect(today(reopenTabs(ws, batch!))).toEqual(['', 'https://a.test', 'https://z.test'])
  })

  it('returns null if there is nothing to close', () => {
    let ws = fresh()
    ws = pinTab(ws, ws.spaces[0].today[0])
    const { workspace, batch } = closeTodayTabsBatch(ws)
    expect(batch).toBeNull()
    expect(workspace).toBe(ws)
  })

  it('does not break if the space has disappeared', () => {
    const { workspace, batch } = closeTodayTabsBatch(build())
    expect(reopenTabs({ ...workspace, spaces: workspace.spaces.map((s) => ({ ...s, id: 'other' })) }, batch!).spaces[0].id).toBe('other')
  })
})

describe('space limits', () => {
  it('canRemoveSpace / canAddSpace', () => {
    let ws = fresh()
    expect(canRemoveSpace(ws)).toBe(false)
    expect(canAddSpace(ws)).toBe(true)
    while (ws.spaces.length < MAX_SPACES) ws = addSpace(ws, { name: 'S', icon: '✨', hue: 1 }, 'S')
    expect(canAddSpace(ws)).toBe(false)
    expect(canRemoveSpace(ws)).toBe(true)
    expect(activeSpace(ws).name).toBe('S')
  })
})

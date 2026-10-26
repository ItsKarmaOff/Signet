import { describe, expect, it } from 'vitest'
import { addBookmark } from './bookmarks'
import { addVisit } from './history'
import { defaultState, LEGACY_TABS_KEY, parsePersisted, PERSIST_VERSION, serializePersisted, STORAGE_KEY, toPersisted } from './persist'
import { DEFAULT_MOBILE_SETTINGS } from './settings'
import { currentTab, openTab, setPrivateMode, visibleTabIds } from './workspace'

const ctx = { defaultSpaceName: 'Perso' }

describe('persist', () => {
  it('keys and version', () => {
    expect(STORAGE_KEY).toContain('mobile')
    expect(LEGACY_TABS_KEY).toBe('signet.tabs.v1')
    expect(PERSIST_VERSION).toBe(2)
  })

  it('nothing / corrupted JSON / wrong type -> defaults, never an exception', () => {
    for (const text of [null, undefined, '', '{', 'null', '42', '[]', '"x"']) {
      const s = parsePersisted(text, ctx)
      expect(s.settings).toEqual(DEFAULT_MOBILE_SETTINGS)
      expect(s.workspace.spaces).toHaveLength(1)
      expect(s.ui.onboardingDone).toBe(false)
    }
  })

  it('full round trip, private tabs excluded', () => {
    let s = defaultState('Perso')
    s.workspace = openTab(s.workspace, { url: 'https://a.com', title: 'A' })
    s.workspace = openTab(setPrivateMode(s.workspace, true), { url: 'https://secret.com' })
    s.history = addVisit([], { url: 'https://a.com', title: 'A' }, 5)
    s.bookmarks = addBookmark([], { title: 'A', url: 'https://a.com' })
    s = { ...s, settings: { ...s.settings, theme: 'dark', searchEngine: 'brave' }, ui: { onboardingDone: true } }
    const text = serializePersisted(s)
    expect(text).not.toContain('secret.com')
    expect(JSON.parse(text).version).toBe(2)
    const back = parsePersisted(text, ctx)
    expect(back.settings).toMatchObject({ theme: 'dark', searchEngine: 'brave' })
    expect(back.history).toEqual(s.history)
    expect(back.bookmarks).toEqual(s.bookmarks)
    expect(back.ui.onboardingDone).toBe(true)
    expect(back.workspace.privateMode).toBe(false)
    expect(visibleTabIds(back.workspace).map((id) => back.workspace.tabs[id].url)).toEqual(['', 'https://a.com'])
    expect(toPersisted(back).workspace.tabs).toHaveLength(2)
  })

  it('migrates the skeleton format (flat tab list) to a space', () => {
    const legacy = JSON.stringify({
      tabs: [{ id: 'a', url: 'https://a.com', title: 'A', lastActive: 1 }, { id: 'b', url: 'https://b.com', title: 'B', lastActive: 2 }, { nope: 1 }],
      activeId: 'a'
    })
    const s = parsePersisted(null, { ...ctx, legacyTabs: legacy })
    expect(s.workspace.spaces).toHaveLength(1)
    expect(s.workspace.spaces[0].name).toBe('Perso')
    expect(s.workspace.spaces[0].today).toEqual(['a', 'b'])
    expect(currentTab(s.workspace)?.id).toBe('a')
    expect(s.ui.onboardingDone).toBe(true)
  })

  it('the current document takes precedence over the old format', () => {
    const s = defaultState('Perso')
    const text = serializePersisted(s)
    const back = parsePersisted(text, { ...ctx, legacyTabs: JSON.stringify({ tabs: [{ id: 'z', url: 'https://z.com' }] }) })
    expect(back.workspace.tabs.z).toBeUndefined()
  })

  it('a partial document or one from a future version is sanitized part by part', () => {
    const s = parsePersisted(
      JSON.stringify({
        version: 99,
        settings: { theme: 'light', searchEngine: 'x' },
        history: 'x',
        bookmarks: [{ kind: 'link', id: '1', url: 'https://a.com', title: 'A' }],
        ui: { onboardingDone: 'oui' }
      }),
      ctx
    )
    expect(s.settings.theme).toBe('light')
    expect(s.settings.searchEngine).toBe('google')
    expect(s.history).toEqual([])
    expect(s.bookmarks).toHaveLength(1)
    expect(s.ui.onboardingDone).toBe(false)
  })
})

// Flat tab list with an active tab: pure reducer (no platform), designed for mobile
// and reusable by a desktop <-> mobile sync. Tabs are `TabInfo`s of the common model.
import { newTab } from './model'
import type { TabInfo } from './types'

export interface TabList {
  tabs: TabInfo[]
  activeId: string | null
}

export type TabListAction =
  | { type: 'open'; url: string; title?: string; tab?: TabInfo }
  | { type: 'close'; id: string }
  | { type: 'activate'; id: string }
  | { type: 'update'; id: string; patch: Partial<TabInfo> }
  | { type: 'closeAll' }
  | { type: 'restore'; list: TabList }

export const emptyTabList: TabList = { tabs: [], activeId: null }

/** `TabInfo` fields kept from one launch to the next; the rest (loading, history…) starts from scratch. */
export interface StoredTab {
  id: string
  url: string
  title: string
  lastActive: number
}

export interface StoredTabList {
  tabs: StoredTab[]
  activeId: string | null
}

export function serializeTabList(list: TabList): StoredTabList {
  return {
    tabs: list.tabs.map(({ id, url, title, lastActive }) => ({ id, url, title, lastActive })),
    activeId: list.activeId
  }
}

/** Reads back a saved list ignoring anything malformed; an unknown `activeId` falls back to the last tab. */
export function parseTabList(raw: unknown): TabList {
  const stored = raw as Partial<StoredTabList> | null | undefined
  if (!stored || !Array.isArray(stored.tabs)) return emptyTabList
  const seen = new Set<string>()
  const tabs: TabInfo[] = []
  for (const item of stored.tabs as unknown[]) {
    const s = item as Partial<StoredTab> | null
    if (!s || typeof s.id !== 'string' || !s.id || typeof s.url !== 'string' || seen.has(s.id)) continue
    seen.add(s.id)
    const tab = newTab(s.url, typeof s.title === 'string' ? s.title : '')
    tabs.push({ ...tab, id: s.id, lastActive: typeof s.lastActive === 'number' ? s.lastActive : tab.lastActive })
  }
  const activeId = typeof stored.activeId === 'string' && seen.has(stored.activeId) ? stored.activeId : (tabs.at(-1)?.id ?? null)
  return { tabs, activeId }
}

export function tabListReducer(state: TabList, action: TabListAction): TabList {
  switch (action.type) {
    case 'open': {
      const tab = action.tab ?? newTab(action.url, action.title)
      return { tabs: [...state.tabs, tab], activeId: tab.id }
    }
    case 'close': {
      const index = state.tabs.findIndex((t) => t.id === action.id)
      if (index < 0) return state
      const tabs = state.tabs.filter((t) => t.id !== action.id)
      if (state.activeId !== action.id) return { ...state, tabs }
      // The active tab closes: move to the right neighbor, otherwise the left one.
      return { tabs, activeId: (tabs[index] ?? tabs[index - 1])?.id ?? null }
    }
    case 'activate': {
      if (state.activeId === action.id || !state.tabs.some((t) => t.id === action.id)) return state
      return {
        tabs: state.tabs.map((t) => (t.id === action.id ? { ...t, lastActive: Date.now() } : t)),
        activeId: action.id
      }
    }
    case 'update': {
      if (!state.tabs.some((t) => t.id === action.id)) return state
      return { ...state, tabs: state.tabs.map((t) => (t.id === action.id ? { ...t, ...action.patch, id: t.id } : t)) }
    }
    case 'closeAll':
      return emptyTabList
    case 'restore':
      return action.list
  }
}

export function activeTab(list: TabList): TabInfo | null {
  return list.tabs.find((t) => t.id === list.activeId) ?? null
}

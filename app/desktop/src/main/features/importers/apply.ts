// Writing imported items into the state (unloaded tabs, provisional favicon).
import { randomUUID } from 'node:crypto'
import type { Bookmark, Item, Space } from '@shared/types'
import type { State } from '../../core/store'
import { newSpace, newTab } from '../../core/store'
import { newFolder, tabItem } from '../../core/tree'
import { countTabs, type ImportedNode, type ImportedTab, type SpacesImport } from './types'

export const MAX_IMPORTED_TABS = 2000
const HUES = [250, 330, 150, 25, 190, 285, 45, 215, 90, 0]

export interface ImportCounts {
  spaces: number
  pinned: number
  favorites: number
  today: number
  bookmarks: number
  truncated: boolean
}

const emptyCounts = (): ImportCounts => ({ spaces: 0, pinned: 0, favorites: 0, today: 0, bookmarks: 0, truncated: false })

/** Creates the tabs of an import, within the limit of `MAX_IMPORTED_TABS`. */
class Builder {
  count = 0
  truncated = false
  constructor(private state: State) {}

  tab(t: ImportedTab, pinned: boolean): string | null {
    if (this.count >= MAX_IMPORTED_TABS) {
      this.truncated = true
      return null
    }
    const tab = newTab(t.url, t.title)
    try {
      tab.favicon = `https://icons.duckduckgo.com/ip3/${new URL(t.url).hostname}.ico`
    } catch {
      // invalid URL: no provisional favicon
    }
    if (pinned) tab.pinnedUrl = t.url
    this.state.tabs[tab.id] = tab
    this.count++
    return tab.id
  }

  items(nodes: ImportedNode[], pinned: boolean): Item[] {
    return nodes.flatMap((n): Item[] => {
      if (n.kind === 'tab') {
        const id = this.tab(n, pinned)
        return id ? [tabItem(id)] : []
      }
      if (n.kind === 'split') {
        const ids = n.tabs.map((t) => this.tab(t, pinned)).filter((x): x is string => !!x)
        if (ids.length < 2) return ids.map(tabItem)
        return [{ kind: 'split', id: randomUUID(), tabIds: ids }]
      }
      const folder = newFolder(n.name)
      folder.items = this.items(n.items, pinned)
      return [folder]
    })
  }
}

/** At most this many bookmarks per import (guard against an oversized file). */
const MAX_IMPORTED_BOOKMARKS = 20000

/**
 * Browser favorites -> bookmarks of the given space: directly in the bar if it is empty,
 * otherwise in a folder named after the browser, at the end of the bar. `inFolder` says which of the two.
 */
export function applyBookmarks(space: Space, folder: ImportedNode): ImportCounts & { inFolder: boolean } {
  let count = 0
  let truncated = false
  const convert = (nodes: ImportedNode[]): Bookmark[] =>
    nodes.flatMap((n): Bookmark[] => {
      if (n.kind === 'folder') return [{ kind: 'folder', id: randomUUID(), name: n.name, items: convert(n.items) }]
      const tabs = n.kind === 'tab' ? [n] : n.tabs
      return tabs.flatMap((t): Bookmark[] => {
        if (count >= MAX_IMPORTED_BOOKMARKS) {
          truncated = true
          return []
        }
        count++
        let favicon: string | undefined
        try {
          favicon = `https://icons.duckduckgo.com/ip3/${new URL(t.url).hostname}.ico`
        } catch {
          // invalid URL: no provisional favicon
        }
        return [{ kind: 'link', id: randomUUID(), title: t.title, url: t.url, ...(favicon ? { favicon } : {}) }]
      })
    })
  const bar = (space.bookmarks ??= [])
  const inFolder = bar.length > 0 || folder.kind !== 'folder'
  if (inFolder) bar.push(...convert([folder]))
  else bar.push(...convert(folder.items))
  return { ...emptyCounts(), bookmarks: count, truncated, inFolder }
}

/** Imported spaces and favorites. A state reduced to an empty space is replaced, otherwise we add. */
export function applySpaces(state: State, data: SpacesImport): ImportCounts {
  const b = new Builder(state)
  const counts = emptyCounts()
  const only = state.spaces[0]
  const blank =
    state.spaces.length === 1 && only.pinned.length === 0 && only.today.length === 0 && !only.bookmarks?.length && !Object.keys(only.groups ?? {}).length
  const base = blank ? 0 : state.spaces.length
  const created = data.spaces.map((sp, i) => {
    const space = newSpace({ name: sp.name, icon: sp.icon, hue: HUES[(base + i) % HUES.length] })
    const before = b.count
    space.pinned = b.items(sp.pinned, true)
    counts.pinned += b.count - before
    const mid = b.count
    space.today = b.items(sp.today, false)
    counts.today += b.count - mid
    return space
  })
  const known = new Set(
    Object.values(state.tabs)
      .filter((t) => t.pinnedUrl && state.favorites.includes(t.id))
      .map((t) => t.pinnedUrl)
  )
  for (const f of data.favorites) {
    if (known.has(f.url)) continue
    const id = b.tab(f, true)
    if (!id) break
    known.add(f.url)
    state.favorites.push(id)
    counts.favorites++
  }
  if (blank && created.length) {
    state.spaces = created
    state.activeSpaceId = created[0].id
  } else state.spaces.push(...created)
  counts.spaces = created.length
  counts.truncated = b.truncated
  return counts
}

export { countTabs }

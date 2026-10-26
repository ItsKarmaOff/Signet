// Spaces and tabs of the mobile app: a single state (`Workspace`) and pure functions that transform it.
// Each space has its favorites, pinned tabs and day tabs (lists of ids; the tabs themselves are in `tabs`).
// Private tabs live apart (`priv`), are never saved (see `serializeWorkspace`) nor added to history.
import { newId } from '../ids'
import { cleanSpaceDraft, newTab } from '../model'
import type { SpaceDraft, SpaceTheme, TabInfo } from '../types'

export type TabZone = 'favorites' | 'pinned' | 'today'
export const TAB_ZONES: readonly TabZone[] = ['favorites', 'pinned', 'today']

export interface MobileSpace {
  id: string
  name: string
  /** Un emoji. */
  icon: string
  /** Teinte 0-359 (voir `mobile/color.ts`). */
  hue: number
  theme?: SpaceTheme
  favorites: string[]
  pinned: string[]
  today: string[]
  /** Tab shown when this space is active; `null` = none (never the case after an action: `ensureTab` creates one). */
  activeTabId: string | null
}

export interface Workspace {
  spaces: MobileSpace[]
  activeSpaceId: string
  /** All tabs, by id (spaces and private). */
  tabs: Record<string, TabInfo>
  /** Private tabs, in memory only. */
  priv: { tabIds: string[]; activeId: string | null }
  /** True when browsing privately (the shown tabs are `priv`). Never saved. */
  privateMode: boolean
}

export type TabLocation = { kind: 'space'; spaceId: string; zone: TabZone; index: number } | { kind: 'private'; index: number }

/** Spaces offered at first launch (names aside, translated by the caller). */
export const DEFAULT_SPACE_ICON = '🌐'
export const DEFAULT_SPACE_HUE = 250
export const MAX_SPACES = 12

function blankSpace(draft: { name: string; icon: string; hue: number; theme?: SpaceTheme }): MobileSpace {
  return {
    id: newId(),
    name: draft.name,
    icon: draft.icon,
    hue: draft.hue,
    ...(draft.theme ? { theme: draft.theme } : {}),
    favorites: [],
    pinned: [],
    today: [],
    activeTabId: null
  }
}

/** Starting space with an empty tab. `name`: (translated) name of the first space. */
export function createWorkspace(name: string): Workspace {
  const space = blankSpace({ name, icon: DEFAULT_SPACE_ICON, hue: DEFAULT_SPACE_HUE })
  return ensureTab({ spaces: [space], activeSpaceId: space.id, tabs: {}, priv: { tabIds: [], activeId: null }, privateMode: false })
}

// ── Reading ──

export const spaceById = (ws: Workspace, id: string): MobileSpace | undefined => ws.spaces.find((s) => s.id === id)

export function activeSpace(ws: Workspace): MobileSpace {
  return spaceById(ws, ws.activeSpaceId) ?? ws.spaces[0]
}

/** Ids of the tabs of a space: favorites, pinned, then the day's. */
export const spaceTabIds = (space: MobileSpace): string[] => [...space.favorites, ...space.pinned, ...space.today]

/** Ids of the tabs shown in the switcher: private ones in private mode, otherwise those of the active space. */
export const visibleTabIds = (ws: Workspace): string[] => (ws.privateMode ? ws.priv.tabIds : spaceTabIds(activeSpace(ws)))

/** Tab shown in the browser. */
export function currentTab(ws: Workspace): TabInfo | null {
  const id = ws.privateMode ? ws.priv.activeId : activeSpace(ws).activeTabId
  return (id && ws.tabs[id]) || null
}

export const isPrivateTab = (ws: Workspace, id: string): boolean => ws.priv.tabIds.includes(id)

export function tabLocation(ws: Workspace, id: string): TabLocation | null {
  const p = ws.priv.tabIds.indexOf(id)
  if (p >= 0) return { kind: 'private', index: p }
  for (const space of ws.spaces) {
    for (const zone of TAB_ZONES) {
      const index = space[zone].indexOf(id)
      if (index >= 0) return { kind: 'space', spaceId: space.id, zone, index }
    }
  }
  return null
}

/** The tabs matching these ids (those that no longer exist are ignored). */
export const tabsOf = (ws: Workspace, ids: string[]): TabInfo[] => ids.map((id) => ws.tabs[id]).filter((t): t is TabInfo => !!t)

// ── Writing (internal tools) ──

const updateSpace = (ws: Workspace, id: string, fn: (s: MobileSpace) => MobileSpace): Workspace => ({
  ...ws,
  spaces: ws.spaces.map((s) => (s.id === id ? fn(s) : s))
})

function insertId(list: string[], id: string, index?: number): string[] {
  const out = list.filter((x) => x !== id)
  out.splice(index === undefined ? out.length : Math.min(Math.max(0, index), out.length), 0, id)
  return out
}

/** Guarantees a tab is shown: creates an empty one if the active space (or private browsing) has none left. Leaves private mode if it has no tab left. */
export function ensureTab(ws: Workspace): Workspace {
  let next = ws
  if (next.privateMode && next.priv.tabIds.length === 0) next = { ...next, privateMode: false, priv: { tabIds: [], activeId: null } }
  if (next.privateMode) {
    const ok = next.priv.activeId && next.priv.tabIds.includes(next.priv.activeId)
    return ok ? next : { ...next, priv: { ...next.priv, activeId: next.priv.tabIds.at(-1) ?? null } }
  }
  const space = activeSpace(next)
  if (space.activeTabId && spaceTabIds(space).includes(space.activeTabId) && next.tabs[space.activeTabId]) return next
  const ids = spaceTabIds(space).filter((id) => next.tabs[id])
  if (ids.length > 0) return updateSpace(next, space.id, (s) => ({ ...s, activeTabId: ids.at(-1) ?? null }))
  return openTab(next, { url: '' })
}

export interface OpenTabOptions {
  url: string
  title?: string
  /** Target space (default: the active one). Ignored for a private tab. */
  spaceId?: string
  zone?: TabZone
  /** Default: the current mode (in private browsing, new tabs are private). */
  private?: boolean
  /** Default: true. */
  activate?: boolean
  index?: number
  /** Forced id (to know it before opening); default: generated. */
  id?: string
}

/** Opens a tab (at the end of the `today` zone by default) and activates it. */
export function openTab(ws: Workspace, opts: OpenTabOptions): Workspace {
  const tab = newTab(opts.url, opts.title ?? '')
  if (opts.id) tab.id = opts.id
  const activate = opts.activate ?? true
  const isPrivate = opts.private ?? ws.privateMode
  const tabs = { ...ws.tabs, [tab.id]: tab }
  if (isPrivate) {
    const priv = { tabIds: insertId(ws.priv.tabIds, tab.id, opts.index), activeId: activate ? tab.id : (ws.priv.activeId ?? tab.id) }
    return { ...ws, tabs, priv, privateMode: activate ? true : ws.privateMode }
  }
  const spaceId = opts.spaceId && spaceById(ws, opts.spaceId) ? opts.spaceId : ws.activeSpaceId
  const zone = opts.zone ?? 'today'
  if (zone !== 'today') tab.pinnedUrl = opts.url
  const next = updateSpace({ ...ws, tabs }, spaceId, (s) => ({
    ...s,
    [zone]: insertId(s[zone], tab.id, opts.index),
    activeTabId: activate ? tab.id : (s.activeTabId ?? tab.id)
  }))
  return activate ? { ...next, activeSpaceId: spaceId, privateMode: false } : next
}

/** Reopens a closed tab at its former location (undo close). */
export function reopenTab(ws: Workspace, tab: TabInfo, at: TabLocation): Workspace {
  if (ws.tabs[tab.id]) return activateTab(ws, tab.id)
  const tabs = { ...ws.tabs, [tab.id]: tab }
  if (at.kind === 'private') return { ...ws, tabs, priv: { tabIds: insertId(ws.priv.tabIds, tab.id, at.index), activeId: tab.id }, privateMode: true }
  const spaceId = spaceById(ws, at.spaceId) ? at.spaceId : ws.activeSpaceId
  return {
    ...updateSpace({ ...ws, tabs }, spaceId, (s) => ({ ...s, [at.zone]: insertId(s[at.zone], tab.id, at.index), activeTabId: tab.id })),
    activeSpaceId: spaceId,
    privateMode: false
  }
}

/** Neighbor tab to activate when `id` (active) closes: the next one, otherwise the previous one, in the flat list. */
function neighbour(flat: string[], id: string): string | null {
  const i = flat.indexOf(id)
  const rest = flat.filter((x) => x !== id)
  return rest[i] ?? rest[i - 1] ?? null
}

/** Closes a tab. If it was the active one, the neighbor takes over; if none is left, an empty tab is opened (in private: leave private mode). */
export function closeTab(ws: Workspace, id: string): Workspace {
  const loc = tabLocation(ws, id)
  if (!loc) return ws
  const tabs = { ...ws.tabs }
  delete tabs[id]
  if (loc.kind === 'private') {
    const ids = ws.priv.tabIds.filter((x) => x !== id)
    const activeId = ws.priv.activeId === id ? neighbour(ws.priv.tabIds, id) : ws.priv.activeId
    return ensureTab({ ...ws, tabs, priv: { tabIds: ids, activeId } })
  }
  const next = updateSpace({ ...ws, tabs }, loc.spaceId, (s) => ({
    ...s,
    [loc.zone]: s[loc.zone].filter((x) => x !== id),
    activeTabId: s.activeTabId === id ? neighbour(spaceTabIds(s), id) : s.activeTabId
  }))
  return ensureTab(next)
}

/** Closes the day tabs of a space (pinned and favorites stay). */
export function closeTodayTabs(ws: Workspace, spaceId: string = ws.activeSpaceId): Workspace {
  const space = spaceById(ws, spaceId)
  if (!space) return ws
  const tabs = { ...ws.tabs }
  for (const id of space.today) delete tabs[id]
  const keep = spaceTabIds({ ...space, today: [] })
  const activeTabId = space.activeTabId && keep.includes(space.activeTabId) ? space.activeTabId : (keep.at(-1) ?? null)
  return ensureTab(updateSpace({ ...ws, tabs }, spaceId, (s) => ({ ...s, today: [], activeTabId })))
}

/** Closes all private tabs and leaves private mode. */
export function closePrivateTabs(ws: Workspace): Workspace {
  const tabs = { ...ws.tabs }
  for (const id of ws.priv.tabIds) delete tabs[id]
  return ensureTab({ ...ws, tabs, priv: { tabIds: [], activeId: null }, privateMode: false })
}

/** Activates a tab (and its space, or private mode). */
export function activateTab(ws: Workspace, id: string): Workspace {
  const loc = tabLocation(ws, id)
  const tab = ws.tabs[id]
  if (!loc || !tab) return ws
  const tabs = { ...ws.tabs, [id]: { ...tab, lastActive: Date.now() } }
  if (loc.kind === 'private') return { ...ws, tabs, priv: { ...ws.priv, activeId: id }, privateMode: true }
  return { ...updateSpace({ ...ws, tabs }, loc.spaceId, (s) => ({ ...s, activeTabId: id })), activeSpaceId: loc.spaceId, privateMode: false }
}

/** Updates a tab's info (page navigation). The id never changes. */
export function updateTab(ws: Workspace, id: string, patch: Partial<TabInfo>): Workspace {
  const tab = ws.tabs[id]
  if (!tab) return ws
  return { ...ws, tabs: { ...ws.tabs, [id]: { ...tab, ...patch, id } } }
}

export interface MoveTarget {
  zone: TabZone
  /** Position in the zone; default: at the end. */
  index?: number
  /** Target space; default: the tab's own. */
  spaceId?: string
}

/** Pins, favorites, unpins or reorders a tab (also to another space). Private tabs do not move. */
export function moveTab(ws: Workspace, id: string, to: MoveTarget): Workspace {
  const loc = tabLocation(ws, id)
  const tab = ws.tabs[id]
  if (!loc || loc.kind !== 'space' || !tab) return ws
  const toSpace = to.spaceId && spaceById(ws, to.spaceId) ? to.spaceId : loc.spaceId
  const moved: TabInfo = to.zone === 'today' ? { ...tab, pinnedUrl: undefined } : { ...tab, pinnedUrl: tab.pinnedUrl ?? tab.url }
  if (to.zone === 'today') delete moved.pinnedUrl
  let next: Workspace = { ...ws, tabs: { ...ws.tabs, [id]: moved } }
  next = updateSpace(next, loc.spaceId, (s) => ({ ...s, [loc.zone]: s[loc.zone].filter((x) => x !== id) }))
  next = updateSpace(next, toSpace, (s) => ({ ...s, [to.zone]: insertId(s[to.zone], id, to.index), activeTabId: s.activeTabId ?? id }))
  // The active tab of the origin space left the space: a neighbor takes over.
  if (toSpace !== loc.spaceId) {
    next = updateSpace(next, loc.spaceId, (s) => (s.activeTabId === id ? { ...s, activeTabId: spaceTabIds(s).at(-1) ?? null } : s))
    if (ws.activeSpaceId === loc.spaceId && !ws.privateMode) next = ensureTab(next)
  }
  return next
}

/** Pins (or unpins) a tab. */
export const pinTab = (ws: Workspace, id: string, pinned = true): Workspace => moveTab(ws, id, { zone: pinned ? 'pinned' : 'today' })
export const favoriteTab = (ws: Workspace, id: string, favorite = true): Workspace => moveTab(ws, id, { zone: favorite ? 'favorites' : 'today' })

/** Enters private browsing (opens an empty private tab if there is none) or leaves it (private tabs stay open). */
export function setPrivateMode(ws: Workspace, on: boolean): Workspace {
  if (!on) return ensureTab({ ...ws, privateMode: false })
  if (ws.priv.tabIds.length === 0) return openTab({ ...ws, privateMode: true }, { url: '', private: true })
  return ensureTab({ ...ws, privateMode: true })
}

/** Duplicates a tab: the copy (already loaded on the same address) lands in the space's day tabs, or among the private ones, without becoming the active tab. `newId`: id of the copy. */
export function duplicateTab(ws: Workspace, id: string, newId: string): Workspace {
  const loc = tabLocation(ws, id)
  const tab = ws.tabs[id]
  if (!loc || !tab) return ws
  const base = { url: tab.url, title: tab.title, id: newId, activate: false }
  const next =
    loc.kind === 'private'
      ? openTab(ws, { ...base, private: true, index: loc.index + 1 })
      : openTab(ws, { ...base, private: false, spaceId: loc.spaceId, zone: 'today', index: loc.zone === 'today' ? loc.index + 1 : undefined })
  return tab.favicon ? updateTab(next, newId, { favicon: tab.favicon }) : next
}

/** Sends a tab to another space, in the same zone (favorite, pinned or day). No effect for a private tab or the same space. */
export function moveTabToSpace(ws: Workspace, id: string, spaceId: string): Workspace {
  const loc = tabLocation(ws, id)
  if (!loc || loc.kind !== 'space' || loc.spaceId === spaceId || !spaceById(ws, spaceId)) return ws
  return moveTab(ws, id, { zone: loc.zone, spaceId })
}

/** A closed tab and where it came from. */
export interface ClosedTab {
  tab: TabInfo
  at: TabLocation
}

/** Batch of tabs closed at once, so everything can be reopened ("Undo"). */
export interface ClosedBatch {
  spaceId: string
  tabs: ClosedTab[]
  /** Empty tab opened in its place when the space was left with no tab: removed on reopen if it stayed empty. */
  placeholderId?: string
}

/** Like `closeTodayTabs`, but also returns the closed batch (`null` if there was nothing to close). */
export function closeTodayTabsBatch(ws: Workspace, spaceId: string = ws.activeSpaceId): { workspace: Workspace; batch: ClosedBatch | null } {
  const space = spaceById(ws, spaceId)
  if (!space || space.today.length === 0) return { workspace: ws, batch: null }
  const tabs: ClosedTab[] = []
  space.today.forEach((id, index) => {
    const tab = ws.tabs[id]
    if (tab) tabs.push({ tab, at: { kind: 'space', spaceId, zone: 'today', index } })
  })
  const workspace = closeTodayTabs(ws, spaceId)
  const placeholderId = spaceById(workspace, spaceId)?.today.find((id) => !ws.tabs[id])
  return { workspace, batch: { spaceId, tabs, ...(placeholderId ? { placeholderId } : {}) } }
}

/** Reopens a batch of closed tabs, in place among the day tabs. The shown tab does not change, unless it was only an empty tab opened at closing time. */
export function reopenTabs(ws: Workspace, batch: ClosedBatch): Workspace {
  if (!spaceById(ws, batch.spaceId)) return ws
  let next = ws
  let replacedActive = false
  const placeholder = batch.placeholderId ? next.tabs[batch.placeholderId] : undefined
  if (batch.placeholderId && placeholder && placeholder.url === '') {
    const pid = batch.placeholderId
    const tabs = { ...next.tabs }
    delete tabs[pid]
    next = updateSpace({ ...next, tabs }, batch.spaceId, (s) => {
      replacedActive = s.activeTabId === pid
      return { ...s, today: s.today.filter((x) => x !== pid), activeTabId: replacedActive ? null : s.activeTabId }
    })
  }
  const restored = batch.tabs.filter((c) => !next.tabs[c.tab.id]).sort((a, b) => (a.at.kind === 'space' && b.at.kind === 'space' ? a.at.index - b.at.index : 0))
  if (restored.length === 0) return ensureTab(next)
  const tabs = { ...next.tabs }
  for (const c of restored) tabs[c.tab.id] = c.tab
  next = updateSpace({ ...next, tabs }, batch.spaceId, (s) => {
    let today = s.today
    for (const c of restored) today = insertId(today, c.tab.id, c.at.kind === 'space' ? c.at.index : undefined)
    return { ...s, today, activeTabId: s.activeTabId ?? restored.at(-1)?.tab.id ?? null }
  })
  return ensureTab(next)
}

/** True if the space can be deleted (at least one must always remain). */
export const canRemoveSpace = (ws: Workspace): boolean => ws.spaces.length > 1

/** True if a space can still be created. */
export const canAddSpace = (ws: Workspace): boolean => ws.spaces.length < MAX_SPACES

// ── Spaces ──

/** Switches space (leaves private mode). */
export function setActiveSpace(ws: Workspace, id: string): Workspace {
  if (!spaceById(ws, id)) return ws
  return ensureTab({ ...ws, activeSpaceId: id, privateMode: false })
}

/** Creates a space (at most `MAX_SPACES`), with an empty tab, and activates it. `defaultName`: translated name if the draft has none. */
export function addSpace(ws: Workspace, draft: SpaceDraft, defaultName: string): Workspace {
  if (ws.spaces.length >= MAX_SPACES) return ws
  const space = blankSpace(cleanSpaceDraft(draft, defaultName))
  return ensureTab({ ...ws, spaces: [...ws.spaces, space], activeSpaceId: space.id, privateMode: false })
}

/** Edits the name, emoji, hue or gradient of a space. */
export function editSpace(ws: Workspace, id: string, draft: SpaceDraft): Workspace {
  const space = spaceById(ws, id)
  if (!space) return ws
  const clean = cleanSpaceDraft(draft, space.name)
  return updateSpace(ws, id, (s) => {
    const next: MobileSpace = { ...s, name: clean.name, icon: clean.icon, hue: clean.hue, theme: clean.theme }
    if (!clean.theme) delete next.theme
    return next
  })
}

/** Deletes a space and its tabs. The last space cannot be deleted. */
export function removeSpace(ws: Workspace, id: string): Workspace {
  const space = spaceById(ws, id)
  if (!space || ws.spaces.length <= 1) return ws
  const tabs = { ...ws.tabs }
  for (const t of spaceTabIds(space)) delete tabs[t]
  const index = ws.spaces.findIndex((s) => s.id === id)
  const spaces = ws.spaces.filter((s) => s.id !== id)
  const activeSpaceId = ws.activeSpaceId === id ? (spaces[index] ?? spaces[index - 1]).id : ws.activeSpaceId
  return ensureTab({ ...ws, spaces, tabs, activeSpaceId })
}

/** Moves a space to position `to` in the list. */
export function moveSpace(ws: Workspace, id: string, to: number): Workspace {
  const from = ws.spaces.findIndex((s) => s.id === id)
  if (from < 0) return ws
  const spaces = [...ws.spaces]
  const [s] = spaces.splice(from, 1)
  spaces.splice(Math.min(Math.max(0, to), spaces.length), 0, s)
  return { ...ws, spaces }
}

/** Neighbor space (`delta` = ±1), for the horizontal swipe. `null` at the edge. */
export function adjacentSpace(ws: Workspace, delta: 1 | -1): MobileSpace | null {
  const i = ws.spaces.findIndex((s) => s.id === ws.activeSpaceId)
  return ws.spaces[i + delta] ?? null
}

// ── Saving ──

/** Tab fields kept from one launch to the next. */
export interface StoredTab {
  id: string
  url: string
  title: string
  lastActive: number
  favicon?: string
  pinnedUrl?: string
}

export interface StoredWorkspace {
  spaces: MobileSpace[]
  activeSpaceId: string
  tabs: StoredTab[]
}

/** Saveable version: without private tabs, without loading state. */
export function serializeWorkspace(ws: Workspace): StoredWorkspace {
  const tabs: StoredTab[] = []
  for (const space of ws.spaces) {
    for (const id of spaceTabIds(space)) {
      const t = ws.tabs[id]
      if (!t) continue
      tabs.push({
        id,
        url: t.url,
        title: t.title,
        lastActive: t.lastActive,
        ...(t.favicon ? { favicon: t.favicon } : {}),
        ...(t.pinnedUrl ? { pinnedUrl: t.pinnedUrl } : {})
      })
    }
  }
  return { spaces: ws.spaces, activeSpaceId: ws.activeSpaceId, tabs }
}

const ids = (v: unknown, known: Set<string>, claimed: Set<string>): string[] => {
  if (!Array.isArray(v)) return []
  const out: string[] = []
  for (const x of v as unknown[]) {
    if (typeof x === 'string' && known.has(x) && !claimed.has(x)) {
      claimed.add(x)
      out.push(x)
    }
  }
  return out
}

/** Reloads a saved workspace. Tolerant: orphan tabs removed, malformed spaces ignored, at least one space and one tab on output. */
export function parseWorkspace(raw: unknown, defaultName: string): Workspace {
  const stored = (raw && typeof raw === 'object' ? raw : {}) as Partial<Record<keyof StoredWorkspace, unknown>>
  const tabs: Record<string, TabInfo> = {}
  if (Array.isArray(stored.tabs)) {
    for (const item of stored.tabs as unknown[]) {
      const s = item as Partial<StoredTab> | null
      if (!s || typeof s.id !== 'string' || !s.id || typeof s.url !== 'string' || tabs[s.id]) continue
      const tab = newTab(s.url, typeof s.title === 'string' ? s.title : '')
      tabs[s.id] = {
        ...tab,
        id: s.id,
        lastActive: typeof s.lastActive === 'number' ? s.lastActive : tab.lastActive,
        ...(typeof s.favicon === 'string' && s.favicon ? { favicon: s.favicon } : {}),
        ...(typeof s.pinnedUrl === 'string' && s.pinnedUrl ? { pinnedUrl: s.pinnedUrl } : {})
      }
    }
  }
  const known = new Set(Object.keys(tabs))
  const claimed = new Set<string>()
  const spaces: MobileSpace[] = []
  const seen = new Set<string>()
  if (Array.isArray(stored.spaces)) {
    for (const item of stored.spaces as unknown[]) {
      const s = item as Partial<MobileSpace> | null
      if (!s || typeof s.id !== 'string' || !s.id || seen.has(s.id) || spaces.length >= MAX_SPACES) continue
      seen.add(s.id)
      const clean = cleanSpaceDraft({ name: s.name as string, icon: s.icon as string, hue: s.hue as number, theme: s.theme }, defaultName)
      const space: MobileSpace = {
        id: s.id,
        ...clean,
        favorites: ids(s.favorites, known, claimed),
        pinned: ids(s.pinned, known, claimed),
        today: ids(s.today, known, claimed),
        activeTabId: null
      }
      space.activeTabId = typeof s.activeTabId === 'string' && spaceTabIds(space).includes(s.activeTabId) ? s.activeTabId : (spaceTabIds(space).at(-1) ?? null)
      spaces.push(space)
    }
  }
  // Tabs attached to no space: removed.
  for (const id of known) if (!claimed.has(id)) delete tabs[id]
  if (spaces.length === 0) return createWorkspace(defaultName)
  const activeSpaceId = typeof stored.activeSpaceId === 'string' && seen.has(stored.activeSpaceId) ? stored.activeSpaceId : spaces[0].id
  return ensureTab({ spaces, activeSpaceId, tabs, priv: { tabIds: [], activeId: null }, privateMode: false })
}

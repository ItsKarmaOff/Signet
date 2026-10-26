// Saved schema of the mobile app: a single versioned JSON document, read back tolerantly (never throws),
// with migration from previous formats. Private tabs never appear in it.
import type { Bookmark, HistoryEntry } from '../types'
import { parseBookmarks } from './bookmarks'
import { parseHistory } from './history'
import { DEFAULT_MOBILE_SETTINGS, sanitizeSettings, type MobileSettings } from './settings'
import { createWorkspace, parseWorkspace, serializeWorkspace, type StoredWorkspace, type Workspace } from './workspace'

/** Current document version. Increment here AND add a step in `migrate` when the format changes. */
export const PERSIST_VERSION = 2
/** AsyncStorage key of the document. */
export const STORAGE_KEY = 'signet.mobile.state'
/** Key of the initial skeleton (version 1: flat tab list, `StoredTabList`), read once to migrate. */
export const LEGACY_TABS_KEY = 'signet.tabs.v1'

export interface UiState {
  /** True once onboarding is finished or skipped. */
  onboardingDone: boolean
}

export const DEFAULT_UI: UiState = { onboardingDone: false }

/** Everything that is saved. */
export interface PersistedState {
  version: typeof PERSIST_VERSION
  workspace: StoredWorkspace
  history: HistoryEntry[]
  bookmarks: Bookmark[]
  settings: MobileSettings
  ui: UiState
}

export interface LoadedState {
  workspace: Workspace
  history: HistoryEntry[]
  bookmarks: Bookmark[]
  settings: MobileSettings
  ui: UiState
}

export interface ParseContext {
  /** Name (translated) of the first space, created when nothing is saved. */
  defaultSpaceName: string
  /** Raw content of the `LEGACY_TABS_KEY` key, if it exists. */
  legacyTabs?: string | null
}

const tryParse = (text: string | null | undefined): unknown => {
  if (!text) return undefined
  try {
    return JSON.parse(text)
  } catch {
    return undefined
  }
}

const isObject = (v: unknown): v is Record<string, unknown> => !!v && typeof v === 'object' && !Array.isArray(v)

/**
 * Brings a document of any version up to the current version (as a not-yet-sanitized object).
 * v1 (skeleton): `{ tabs: [{id, url, title, lastActive}], activeId }` -> one space with these day tabs.
 */
export function migrate(doc: unknown, defaultSpaceName: string): Record<string, unknown> {
  if (!isObject(doc)) return {}
  const version = typeof doc.version === 'number' ? doc.version : 1
  if (version >= 2) return doc
  // v1 -> v2: the flat tab list becomes the day tabs of the first space.
  const legacy = Array.isArray(doc.tabs) ? (doc.tabs as { id?: unknown }[]) : []
  const ws = createWorkspace(defaultSpaceName)
  const space = ws.spaces[0]
  const tabs = legacy.filter((t) => isObject(t) && typeof t.id === 'string' && t.id)
  const ids = tabs.map((t) => (t as { id: string }).id)
  return {
    version: 2,
    workspace: {
      spaces: [{ ...space, today: ids, activeTabId: typeof doc.activeId === 'string' && ids.includes(doc.activeId) ? doc.activeId : (ids.at(-1) ?? null) }],
      activeSpaceId: space.id,
      tabs
    },
    ui: { onboardingDone: true }
  }
}

/** Default state of a first launch. */
export function defaultState(defaultSpaceName: string): LoadedState {
  return { workspace: createWorkspace(defaultSpaceName), history: [], bookmarks: [], settings: DEFAULT_MOBILE_SETTINGS, ui: DEFAULT_UI }
}

/** Reads back the saved text (or the old format). Missing, unreadable or corrupted document: default values for the affected part. */
export function parsePersisted(text: string | null | undefined, ctx: ParseContext): LoadedState {
  let doc: unknown = tryParse(text)
  if (!isObject(doc) && ctx.legacyTabs) doc = tryParse(ctx.legacyTabs)
  if (!isObject(doc)) return defaultState(ctx.defaultSpaceName)
  const v2 = migrate(doc, ctx.defaultSpaceName)
  const ui = isObject(v2.ui) ? v2.ui : {}
  return {
    workspace: parseWorkspace(v2.workspace, ctx.defaultSpaceName),
    history: parseHistory(v2.history),
    bookmarks: parseBookmarks(v2.bookmarks),
    settings: sanitizeSettings(v2.settings),
    ui: { onboardingDone: typeof ui.onboardingDone === 'boolean' ? ui.onboardingDone : DEFAULT_UI.onboardingDone }
  }
}

export function toPersisted(s: LoadedState): PersistedState {
  return { version: PERSIST_VERSION, workspace: serializeWorkspace(s.workspace), history: s.history, bookmarks: s.bookmarks, settings: s.settings, ui: s.ui }
}

export const serializePersisted = (s: LoadedState): string => JSON.stringify(toPersisted(s))

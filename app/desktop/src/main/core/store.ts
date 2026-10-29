import { app, type Rectangle } from 'electron'
import { copyFileSync, existsSync, readFileSync } from 'node:fs'
import { isAbsolute, join } from 'node:path'
import { APP_ICONS } from '@shared/types'
import type { Boost, DownloadInfo, Settings, Space, SpaceDraft, SpaceTheme, TabGroup, TabInfo, Zone } from '@shared/types'
import { cleanSpaceDraft as cleanDraft, newSpace, newTab } from '@signet/shared/model'
import { itemTabIds, pruneItems, stripGroups, syncGroups } from './tree'
import { ARCHIVE_MAX } from '../model/types'
import { AtomicWriter, writeFileSyncDurable } from './atomic'
import { aliasTarget, isAliasName } from '@shared/devhosts'
import { cleanOverrides } from '@shared/shortcuts'
import { cleanSiteZoom, isFontSize, isMinFontSize, isZoomPercent, type SiteZoom } from '@shared/pagesettings'
import { pruneBookmarks } from '../features/bookmarks'
import { defaultLanguages, normalizeLanguages, takeStartupState } from '../web/languages'
import { initLocale, t } from '../app/i18n'
import { isSearchEngineId } from '@signet/shared/mobile/search'

export interface ArchivedTab {
  /** Unique identifier (absent from archives predating its addition). */
  id?: string
  url: string
  title: string
  favicon?: string
  archivedAt: number
}

/** Closed tab, with its place and its navigation history (in-memory stack, never written to disk). */
export interface ClosedTab {
  url: string
  title: string
  spaceId: string
  favicon?: string
  pinnedUrl?: string
  customTitle?: string
  customIcon?: string
  /** Original area, folder and rank; the rank counts within the list (area root or folder). */
  zone: Zone
  folderId?: string
  index: number
  history?: TabHistory
  /** Same value for the tabs of a group closed at once: ⇧⌘T reopens them together. */
  batch?: number
  /** Header of the closed group (on its first tab). */
  group?: TabGroup
  /** Today tab group the tab belonged to: it goes back there if it still exists. */
  groupId?: string
}

/** Navigation history entry of a tab (main process only). */
export interface HistoryEntry {
  url: string
  title: string
  /** Scroll position, forms… (Chromium base64). */
  pageState?: string
}

export interface TabHistory {
  entries: HistoryEntry[]
  index: number
}

export const HISTORY_MAX_ENTRIES = 50
/** Maximum cumulative size of a tab's `pageState`, then of all tabs (characters). */
export const HISTORY_TAB_STATE_MAX = 200_000
export const HISTORY_TOTAL_STATE_MAX = 3_000_000

/** Bounds a history: 50 entries around the active one, `pageState` dropped for the farthest ones beyond the budget. */
export function trimHistory(entries: HistoryEntry[], index: number, budget = HISTORY_TAB_STATE_MAX): TabHistory | null {
  if (!entries.length || index < 0 || index >= entries.length) return null
  let start = Math.max(0, index - Math.floor(HISTORY_MAX_ENTRIES / 2))
  const end = Math.min(entries.length, start + HISTORY_MAX_ENTRIES)
  start = Math.max(0, end - HISTORY_MAX_ENTRIES)
  const kept = entries.slice(start, end).map((e) => ({ url: e.url, title: e.title, pageState: e.pageState }))
  const active = index - start
  // Budget consumed starting from the active entry, outward.
  let left = budget
  const order = kept.map((_, i) => i).sort((a, b) => Math.abs(a - active) - Math.abs(b - active))
  for (const i of order) {
    const size = kept[i].pageState?.length ?? 0
    if (size <= left) left -= size
    else kept[i].pageState = undefined
  }
  return { entries: kept, index: active }
}

function validHistory(h: unknown): TabHistory | null {
  const x = h as TabHistory | undefined
  if (!x || !Array.isArray(x.entries) || !Number.isInteger(x.index)) return null
  const entries = x.entries
    .filter((e) => e && typeof e.url === 'string' && e.url)
    .map((e) => ({ url: e.url, title: typeof e.title === 'string' ? e.title : '', pageState: typeof e.pageState === 'string' ? e.pageState : undefined }))
  if (entries.length !== x.entries.length) return null
  return trimHistory(entries, x.index)
}

export interface State {
  version: 1
  tabs: Record<string, TabInfo>
  favorites: string[]
  spaces: Space[]
  activeSpaceId: string
  sidebarVisible: boolean
  sidebarWidth: number
  archive: ArchivedTab[]
  downloads: DownloadInfo[]
  settings: Settings
  /** Boosts per site (key: `siteOf(url)`). */
  boosts: Record<string, Boost>
  /** Permission decisions per origin (see permissions.ts). */
  sitePermissions: Record<string, Record<string, 'allow' | 'deny'>>
  bounds: Rectangle | null
  /** Last size and position of the mini player. */
  pipBounds: Rectangle | null
  /** Zoom level the user chose per site host (regular windows only; not part of backups). Bounded, see `rememberSiteZoom`. */
  siteZoom: SiteZoom
}

export const SIDEBAR_MIN = 180
export const SIDEBAR_MAX = 420
/** Overshoot tolerated during the gesture (elastic effect); never persisted. */
export const SIDEBAR_OVERSHOOT = 80
export const DOWNLOADS_MAX = 30

export { itemTabIds, ARCHIVE_MAX }

export { newSpace, newTab } from '@signet/shared/model'

export function cleanSpaceDraft(draft: SpaceDraft): { name: string; icon: string; hue: number; theme?: SpaceTheme } {
  return cleanDraft(draft, t('defaults.space'))
}

function initialState(): State {
  // Starting space: purple → red gradient, half grain.
  const space = newSpace({ name: t('defaults.space'), icon: '🏠', hue: 250, theme: { hues: [250, 0], grain: 0.5 } })
  return {
    version: 1,
    tabs: {},
    favorites: [],
    spaces: [space],
    activeSpaceId: space.id,
    sidebarVisible: true,
    sidebarWidth: 240,
    archive: [],
    downloads: [],
    settings: {
      archiveAfterHours: 12,
      peekExternalLinks: true,
      adblock: true,
      // Only a fresh state shows the introduction (see `sanitize` and the fallback after an unreadable file).
      onboarded: false,
      appIcon: 'aurore',
      compactMode: false,
      autoPip: true,
      routes: [],
      languages: defaultLanguages()
    },
    boosts: {},
    sitePermissions: {},
    bounds: null,
    pipBounds: null,
    siteZoom: {}
  }
}

/** `Settings` keys validated by `cleanSettings`. */
export const CLEAN_SETTING_KEYS = [
  'archiveAfterHours',
  'tabSleepMinutes',
  'devAliases',
  'shortcuts',
  'extensionOrder',
  'pinnedExtensions',
  'searchEngine',
  'downloadDir',
  'fontSize',
  'minimumFontSize',
  'defaultZoom'
] as const

/**
 * Settings validated field by field: an invalid value in `patch` leaves the one from `base`.
 * Returns `base` with only the `CLEAN_SETTING_KEYS` fields replaced (absent from `patch` = unchanged).
 */
export function cleanSettings(patch: Partial<Settings>, base: Settings): Settings {
  const out: Settings = { ...base }
  const hours = patch.archiveAfterHours
  if (typeof hours === 'number' && Number.isFinite(hours) && hours >= 0) out.archiveAfterHours = hours
  const sleep = patch.tabSleepMinutes
  if (typeof sleep === 'number' && Number.isInteger(sleep) && sleep >= 0) out.tabSleepMinutes = sleep
  // Local names: valid name, normalized local target, only one per name (the first wins).
  if (Array.isArray(patch.devAliases)) {
    const seen = new Set<string>()
    const aliases = patch.devAliases.flatMap((a) => {
      const name = typeof a?.name === 'string' ? a.name.trim().toLowerCase() : ''
      const target = typeof a?.target === 'string' ? aliasTarget(a.target) : null
      if (!isAliasName(name) || !target || seen.has(name)) return []
      seen.add(name)
      return [{ name, target }]
    })
    if (aliases.length) out.devAliases = aliases
    else delete out.devAliases
  }
  // Shortcuts: the whole set is rejected if inconsistent (invalid, duplicate or reserved shortcut).
  if (patch.shortcuts !== undefined) {
    const shortcuts = cleanOverrides(patch.shortcuts, process.platform === 'darwin')
    if (shortcuts && Object.keys(shortcuts).length) out.shortcuts = shortcuts
    else if (shortcuts) delete out.shortcuts
  }
  // Extension buttons: valid identifiers only, no duplicates.
  const ids = (v: unknown): string[] | undefined =>
    Array.isArray(v) ? [...new Set(v.filter((x): x is string => typeof x === 'string' && /^[a-p]{32}$/.test(x)))] : undefined
  const order = ids(patch.extensionOrder)
  if (order) out.extensionOrder = order
  const pinned = ids(patch.pinnedExtensions)
  if (pinned) out.pinnedExtensions = pinned
  // Search engine: known identifier only; otherwise keep the one from `base` (absent = Google).
  if (isSearchEngineId(patch.searchEngine)) out.searchEngine = patch.searchEngine
  // Downloads folder: absolute path; an empty string falls back to the system folder.
  if (patch.downloadDir === '') delete out.downloadDir
  else if (typeof patch.downloadDir === 'string' && isAbsolute(patch.downloadDir) && patch.downloadDir.length < 4096) out.downloadDir = patch.downloadDir
  // Web pages: values from the choice lists only (see shared/pagesettings.ts); anything else keeps the previous one.
  if (isFontSize(patch.fontSize)) out.fontSize = patch.fontSize
  if (isMinFontSize(patch.minimumFontSize)) out.minimumFontSize = patch.minimumFontSize
  if (isZoomPercent(patch.defaultZoom)) out.defaultZoom = patch.defaultZoom
  return out
}

/** Copies the validated fields of `cleaned` onto `target` (a field absent from `cleaned` is removed from `target`). */
export function applyCleaned(target: Settings, cleaned: Settings): void {
  const t = target as unknown as Record<string, unknown>
  const c = cleaned as unknown as Record<string, unknown>
  for (const k of CLEAN_SETTING_KEYS) {
    if (c[k] === undefined) delete t[k]
    else t[k] = c[k]
  }
}

/** Straightens out a state read from disk: missing fields, references to unknown tabs. */
function sanitize(raw: Partial<State>): State {
  if (!raw || typeof raw !== 'object') throw new Error('state.json invalide')
  const base = initialState()
  const isObj = (v: unknown): boolean => !!v && typeof v === 'object' && !Array.isArray(v)
  const state: State = {
    ...base,
    ...raw,
    version: 1,
    settings: { ...base.settings, ...raw.settings },
    boosts: raw.boosts && typeof raw.boosts === 'object' ? raw.boosts : {},
    sitePermissions: raw.sitePermissions && typeof raw.sitePermissions === 'object' ? raw.sitePermissions : {},
    tabs: {}
  }
  // State already on disk without the field (written before the introduction): the user does not need to see it again.
  if (typeof raw.settings?.onboarded !== 'boolean') state.settings.onboarded = true
  // Settings written by hand or restored: invalid values reset to the original ones.
  applyCleaned(state.settings, cleanSettings(state.settings, base.settings))
  // Migrated to tab-history.json: does not stay in state.json
  delete (state as { tabHistory?: unknown }).tabHistory
  if (!APP_ICONS.some((i) => i.id === state.settings.appIcon)) state.settings.appIcon = 'aurore'
  if (!Array.isArray(state.settings.routes)) state.settings.routes = []
  state.siteZoom = cleanSiteZoom(raw.siteZoom)
  const pb = state.pipBounds as Partial<Rectangle> | null
  if (!pb || ![pb.x, pb.y, pb.width, pb.height].every((n) => Number.isFinite(n))) state.pipBounds = null
  // Migration: without saved languages, follow the system ones.
  state.settings.languages = normalizeLanguages(state.settings.languages)
  if (Array.isArray(state.spaces)) state.spaces = state.spaces.filter(isObj)
  for (const s of Array.isArray(state.spaces) ? state.spaces : []) {
    if (!Array.isArray(s.pinned)) s.pinned = []
    if (!Array.isArray(s.today)) s.today = []
  }
  if (!Array.isArray(state.spaces) || state.spaces.length === 0) state.spaces = base.spaces
  if (!state.spaces.some((s) => s.id === state.activeSpaceId)) state.activeSpaceId = state.spaces[0].id
  state.sidebarWidth = Math.min(SIDEBAR_MAX, Math.max(SIDEBAR_MIN, Number(state.sidebarWidth) || 240))
  state.archive = Array.isArray(state.archive) ? state.archive.slice(0, ARCHIVE_MAX) : []
  // A download left in progress when the app closed will not resume.
  state.downloads = (Array.isArray(state.downloads) ? state.downloads.filter(isObj) : []).slice(0, DOWNLOADS_MAX).map((d) => ({
    ...d,
    status: d.status === 'progressing' || d.status === 'paused' ? 'interrupted' : d.status
  }))

  for (const [id, t] of Object.entries(isObj(raw.tabs) ? raw.tabs! : {})) {
    if (!isObj(t)) continue
    state.tabs[id] = {
      ...newTab(t.url, t.title),
      id,
      favicon: t.favicon,
      pinnedUrl: t.pinnedUrl,
      lastActive: t.lastActive ?? Date.now(),
      muted: !!t.muted,
      customTitle: t.customTitle,
      customIcon: t.customIcon
    }
  }
  const known = (id: string) => id in state.tabs
  state.favorites = (state.favorites ?? []).filter(known)
  for (const space of state.spaces) {
    space.pinned = pruneItems(space.pinned, known, true)
    space.today = pruneItems(space.today, known, false)
    stripGroups(space.pinned)
    syncGroups(space, state.tabs)
    if (space.bookmarks !== undefined) space.bookmarks = pruneBookmarks(space.bookmarks)
    if (space.activeTabId && !known(space.activeTabId)) space.activeTabId = null
  }
  return state
}

/** Map that signals its changes, to know when to rewrite tab-history.json. */
class WatchedMap<K, V> extends Map<K, V> {
  onChange: () => void = () => {}
  set(key: K, value: V): this {
    super.set(key, value)
    this.onChange()
    return this
  }
  delete(key: K): boolean {
    const had = super.delete(key)
    if (had) this.onChange()
    return had
  }
  clear(): void {
    super.clear()
    this.onChange()
  }
}

class StateLockedError extends Error {}

export class Store {
  state: State
  /** Tabs without a place in the sidebar (Peek, Little Signet): never written or sent. */
  readonly detached = new Set<string>()
  /** Stack of recently closed tabs (⇧⌘T), 20 at most. */
  readonly closed: ClosedTab[] = []
  /** Navigation history per tab: never sent to the interface. */
  readonly histories = new WatchedMap<string, TabHistory>()
  private timer: NodeJS.Timeout | null = null
  /** Deadline of the state write timer (ms): a later request does not push back an earlier deadline. */
  private timerAt = 0
  private historyTimer: NodeJS.Timeout | null = null
  private historyDirty = false
  private file = join(app.getPath('userData'), 'state.json')
  private historyFile = join(app.getPath('userData'), 'tab-history.json')
  private bookmarksTimer: NodeJS.Timeout | null = null
  private bookmarksDirty = false
  private bookmarksFile = join(app.getPath('userData'), 'bookmarks.json')
  /**
   * True once bookmarks.json contains the bookmarks: only then can state.json do without them.
   * (While false, e.g. failed migration, state.json keeps the bookmarks: nothing is lost.)
   */
  private bookmarksPersisted = false
  /** Space identifiers at the last serialization: a change forces bookmarks.json to be rewritten. */
  private spaceKey = ''
  /** Bookmarks of spaces unknown at load: rewritten as is in bookmarks.json. */
  private bookmarkOrphans: Record<string, unknown> = {}
  /** bookmarks.json unreadable (I/O error): it is not written during this session. */
  private bookmarksLocked = false
  /** state.json unreadable (I/O error): neither it nor the linked files are written during this session. */
  private stateLocked = false
  private stateWriter = new AtomicWriter(this.file, () => (this.stateLocked ? null : this.serializeState()))
  private bookmarksWriter = new AtomicWriter(
    this.bookmarksFile,
    () => this.serializeBookmarks(),
    () => (this.bookmarksDirty = true)
  )
  private historyWriter = new AtomicWriter(
    this.historyFile,
    () => this.serializeHistory(),
    () => (this.historyDirty = true)
  )

  constructor() {
    let legacy: unknown
    let text: string | null = null
    const startup = takeStartupState()
    if (!startup) {
      // I/O error (antivirus, lock…): a few retries, then state.json is no longer written during this session.
      for (let attempt = 0; attempt < 4; attempt++) {
        try {
          text = readFileSync(this.file, 'utf8')
          this.stateLocked = false
          break
        } catch (e) {
          if ((e as NodeJS.ErrnoException).code === 'ENOENT') {
            this.stateLocked = false
            break
          }
          this.stateLocked = true
          if (attempt < 3) Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 100)
        }
      }
    }
    try {
      if (this.stateLocked) throw new StateLockedError()
      if (!startup && text === null) throw Object.assign(new Error('absent'), { code: 'ENOENT' })
      const raw = (startup ?? JSON.parse(text!)) as Partial<State>
      initLocale(raw.settings?.uiLanguage) // before `sanitize`, which names some default items
      this.state = sanitize(raw)
      legacy = (raw as { tabHistory?: unknown }).tabHistory
    } catch (e) {
      // Invalid file: keep it aside before starting from scratch (never for a mere read error)
      if (!(e instanceof StateLockedError) && (e as NodeJS.ErrnoException).code !== 'ENOENT') {
        try {
          copyFileSync(this.file, `${this.file}.corrupt-${Date.now()}`)
        } catch {
          // no file to back up
        }
      }
      this.state = initialState()
      // Unreadable or corrupt file: not a new user, no introduction.
      if ((e as NodeJS.ErrnoException).code !== 'ENOENT' || e instanceof StateLockedError) this.state.settings.onboarded = true
    }
    initLocale(this.state.settings.uiLanguage)
    this.loadBookmarks()
    this.loadHistories(legacy)
    this.histories.onChange = () => this.scheduleHistory()
  }

  /**
   * Bookmarks: bookmarks.json (`{ [spaceId]: Bookmark[] }`) is authoritative for the spaces it mentions;
   * a space absent from the file keeps the bookmarks from state.json (old format, or old binary relaunched in the meantime).
   * In that case bookmarks.json is written right away: state.json only loses its bookmarks after that write succeeds.
   */
  private loadBookmarks(): void {
    let saved: Record<string, unknown> = {}
    let corrupt = false
    if (this.stateLocked) {
      this.bookmarksLocked = true
      return
    }
    try {
      const raw = JSON.parse(readFileSync(this.bookmarksFile, 'utf8'))
      if (raw && typeof raw === 'object' && !Array.isArray(raw)) saved = raw as Record<string, unknown>
      else corrupt = true
    } catch (e) {
      if (e instanceof SyntaxError) corrupt = true
      else if ((e as NodeJS.ErrnoException).code !== 'ENOENT') {
        // I/O error (EACCES, EIO…): the file may be intact, leave it alone for this session
        // and state.json keeps the bookmarks.
        this.bookmarksLocked = true
        this.spaceKey = this.state.spaces.map((s) => s.id).join()
        return
      }
    }
    if (corrupt) {
      try {
        copyFileSync(this.bookmarksFile, `${this.bookmarksFile}.corrupt-${Date.now()}`)
      } catch {
        // no file to back up
      }
    }
    let migrate = corrupt
    const ids = new Set(this.state.spaces.map((s) => s.id))
    for (const space of this.state.spaces) {
      const own = saved[space.id]
      if (Array.isArray(own)) space.bookmarks = pruneBookmarks(own)
      else if (space.bookmarks !== undefined) migrate = true // bookmarks only in state.json
    }
    // Spaces unknown at load (e.g. state.json lost): kept as is and rewritten, never erased.
    for (const [id, own] of Object.entries(saved)) if (!ids.has(id) && Array.isArray(own)) this.bookmarkOrphans[id] = own
    if (Object.keys(this.bookmarkOrphans).length > 0) {
      // A single copy, rewritten only if the orphans changed: no accumulation at each launch.
      try {
        const copy = `${this.bookmarksFile.replace(/\.json$/, '')}.orphans.json`
        const json = JSON.stringify(this.bookmarkOrphans)
        let same = false
        if (existsSync(copy)) same = readFileSync(copy, 'utf8') === json
        if (!same) writeFileSyncDurable(copy, json)
      } catch {
        // best-effort backup
      }
    }
    this.spaceKey = this.state.spaces.map((s) => s.id).join()
    this.bookmarksPersisted = !migrate
    if (migrate) {
      this.bookmarksDirty = true
      this.bookmarksPersisted = this.bookmarksWriter.flushSync()
    }
  }

  /** Per-tab histories: tab-history.json, or failing that the old `tabHistory` field of state.json. */
  private loadHistories(legacy: unknown): void {
    let saved: unknown
    let migrate = false
    try {
      saved = JSON.parse(readFileSync(this.historyFile, 'utf8'))
    } catch (e) {
      if ((e as NodeJS.ErrnoException).code === 'ENOENT') {
        saved = legacy
        migrate = true
      }
    }
    for (const [id, h] of Object.entries(saved && typeof saved === 'object' ? (saved as Record<string, unknown>) : {})) {
      const valid = id in this.state.tabs ? validHistory(h) : null
      if (valid) this.histories.set(id, valid)
    }
    // Migration: write the new file right away, since state.json will no longer embed the histories.
    if (migrate && this.histories.size) {
      this.historyDirty = true
      this.historyWriter.flushSync()
    }
  }

  schedule(delay = 500): void {
    const at = Date.now() + delay
    if (this.timer) {
      if (this.timerAt <= at) return
      clearTimeout(this.timer)
    }
    this.timerAt = at
    this.timer = setTimeout(() => {
      this.timer = null
      this.stateWriter.request()
    }, delay)
  }

  /** To call after any bookmark change (in addition to `schedule`): rewrites bookmarks.json. */
  scheduleBookmarks(delay = 500): void {
    this.bookmarksDirty = true
    if (this.bookmarksTimer) return
    this.bookmarksTimer = setTimeout(() => {
      this.bookmarksTimer = null
      this.bookmarksWriter.request()
    }, delay)
  }

  private scheduleHistory(): void {
    this.historyDirty = true
    if (this.historyTimer) return
    this.historyTimer = setTimeout(() => {
      this.historyTimer = null
      this.historyWriter.request()
    }, 5000)
  }

  /** Full synchronous write (app shutdown). */
  flush(): void {
    if (this.timer) clearTimeout(this.timer)
    if (this.historyTimer) clearTimeout(this.historyTimer)
    if (this.bookmarksTimer) clearTimeout(this.bookmarksTimer)
    this.timer = this.historyTimer = this.bookmarksTimer = null
    // Safety net: bookmarks are always rewritten (identical content is skipped), before the state.
    this.bookmarksDirty = true
    if (this.bookmarksWriter.flushSync() && !this.bookmarksLocked) this.bookmarksPersisted = true
    this.stateWriter.flushSync()
    this.historyWriter.flushSync()
  }

  /** Tabs to persist (excluding detached tabs). */
  private persistedTabs(): State['tabs'] {
    const tabs: State['tabs'] = {}
    for (const [id, t] of Object.entries(this.state.tabs)) {
      if (this.detached.has(id)) continue
      tabs[id] = {
        id,
        url: t.url,
        title: t.title,
        favicon: t.favicon,
        pinnedUrl: t.pinnedUrl,
        lastActive: t.lastActive,
        muted: t.muted,
        customTitle: t.customTitle,
        customIcon: t.customIcon
      } as TabInfo
    }
    return tabs
  }

  private serializeState(): string {
    // Space created, deleted or imported: bookmarks.json must follow, even if the caller forgot `scheduleBookmarks`.
    const key = this.state.spaces.map((s) => s.id).join()
    if (key !== this.spaceKey) {
      this.spaceKey = key
      this.scheduleBookmarks()
    }
    // Bookmarks live in bookmarks.json (without touching the in-memory state); private downloads are never written.
    const spaces = this.bookmarksPersisted ? this.state.spaces.map(({ bookmarks: _bookmarks, ...s }) => s) : this.state.spaces
    return JSON.stringify({ ...this.state, spaces, tabs: this.persistedTabs(), downloads: this.state.downloads.filter((d) => !d.private) })
  }

  private serializeBookmarks(): string | null {
    if (this.bookmarksLocked || !this.bookmarksDirty) return null
    this.bookmarksDirty = false
    const all: Record<string, unknown> = { ...this.bookmarkOrphans }
    for (const s of this.state.spaces) if (s.bookmarks !== undefined) all[s.id] = s.bookmarks
    return JSON.stringify(all)
  }

  /** Histories: only persisted tabs, global `pageState` budget (most recent first). */
  private serializeHistory(): string | null {
    if (!this.historyDirty || this.stateLocked) return null
    this.historyDirty = false
    const tabs = this.persistedTabs()
    const tabHistory: Record<string, TabHistory> = {}
    let budget = HISTORY_TOTAL_STATE_MAX
    const byRecent = Object.keys(tabs).sort((a, b) => (tabs[b].lastActive ?? 0) - (tabs[a].lastActive ?? 0))
    for (const id of byRecent) {
      const h = this.histories.get(id)
      if (!h) continue
      const trimmed = trimHistory(h.entries, h.index, Math.min(budget, HISTORY_TAB_STATE_MAX))
      if (!trimmed) continue
      budget -= trimmed.entries.reduce((n, e) => n + (e.pageState?.length ?? 0), 0)
      tabHistory[id] = trimmed
    }
    return JSON.stringify(tabHistory)
  }
}

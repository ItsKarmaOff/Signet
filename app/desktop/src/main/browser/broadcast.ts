import type { BrowserWindow, WebContents } from 'electron'
import type { Snapshot, StatePatch, TabInfo } from '@shared/types'
import type { UiEventMap } from '@shared/ipc/contract'
import { pushUi } from '../ipc/push'
import type { Store } from '../core/store'
import { syncGroups } from '../core/tree'
import { activeSpace, displayedTabIds } from '../model/locate'
import { isSidebarOn } from '../model/chrome'
import type { Compact } from './compact'
import type { Floating } from './floating'
import type { Layout } from './layout'
import type { Media } from './media'
import type { TabPreviews } from './preview'

/** `TabInfo` fields that do not affect the views' layout. */
const META_FIELDS = new Set<keyof TabInfo>([
  'title',
  'favicon',
  'loading',
  'audible',
  'themeColor',
  'canGoBack',
  'canGoForward',
  'url',
  'customTitle',
  'customIcon',
  'muted',
  'reader',
  'foreignLang',
  'translation',
  'liveUnread',
  'passwordKey',
  'consoleErrors',
  'consoleWarnings',
  'devOverrides'
])
/** Fields updated by engine events (not by a gesture): their broadcast can be batched. */
const ENGINE_FIELDS = new Set<keyof TabInfo>([
  'title',
  'favicon',
  'loading',
  'audible',
  'themeColor',
  'canGoBack',
  'canGoForward',
  'url',
  'consoleErrors',
  'consoleWarnings'
])
/** Fields sent alone (`state:patch`): read only from `snapshot.tabs[id]` by the views, with no derived data elsewhere. */
const PATCH_FIELDS = new Set<keyof TabInfo>([...ENGINE_FIELDS, 'reader', 'foreignLang', 'translation', 'passwordKey'])
/** Fields written to state.json (see Store.flush). */
const PERSISTED_FIELDS = new Set<keyof TabInfo>(['url', 'title', 'favicon', 'pinnedUrl', 'lastActive', 'muted', 'customTitle', 'customIcon'])
/** Window (ms) for batching broadcasts caused by engine events. */
const SOFT_APPLY_MS = 32
/** Delay (ms) before writing when only the tab title or icon changed. */
export const SLOW_PERSIST_MS = 5000
/** Deferred write of the tab title and icon only. */
const LAZY_PERSIST_MS = 60_000

export interface ChangeOpts {
  layout?: boolean
  persist?: boolean
  soft?: boolean
  slow?: boolean
  lazy?: boolean
  patch?: boolean
}
export type Changed = (opts?: ChangeOpts) => void

/** The overlay as seen by the broadcast: whether it is open, and who to talk to. */
export interface OverlaySurface {
  readonly open: boolean
  readonly leaving: boolean
  readonly popoverOpen: boolean
  readonly ready: boolean
  readonly contents: WebContents | null
}

export interface BroadcastDeps {
  store: Store
  window: { readonly win: BrowserWindow | null }
  layout: Pick<Layout, 'update' | 'panes' | 'dividers' | 'liveSidebarWidth' | 'shownTabs'>
  floating: Pick<Floating, 'shown' | 'leaving' | 'sidebar'>
  compact: Pick<Compact, 'floating' | 'leaving' | 'send'>
  media: Pick<Media, 'current'>
  previews: Pick<TabPreviews, 'onLoaded'>
  overlay: OverlaySurface
  /** App menu: the key summarizes the states that check its items, `rebuild` rebuilds it. */
  menu: { key: () => string; rebuild: () => void }
  /** Secondary windows (Little Signet, apps, private, settings, library…) to keep up to date after each broadcast. */
  satellites: (snapshot: Snapshot | undefined) => void
}

/**
 * Batches state changes: layout recomputation, sending the state to the UIs, deferred save.
 * `changed()` marks what needs redoing; `apply()` runs the cycle (next tick, or after 32 ms for engine events).
 */
export class Broadcast {
  private pending = false
  /** Pending "soft" broadcast (engine events): batches bursts, an ordinary `changed()` overtakes it. */
  private applyTimer: NodeJS.Timeout | null = null
  private layoutDirty = false
  private persistDirty = false
  /** True if the pending save must go out quickly (otherwise only title or icon changed: it can wait). */
  private persistFast = false
  private persistLazy = false
  /** States that check View menu items, as of the last menu built. */
  private menuKey = ''
  /** Tab changes waiting to be sent (`state:patch`) as long as no structural change requires the full state. */
  private tabPatches = new Map<string, Set<keyof TabInfo>>()
  private needsFull = true
  /** Tabs present in the last full state sent, and its media (data derived from tab fields). */
  private sentTabIds = new Set<string>()
  private sentMedia = ''
  /** Did the floating views receive the state in the last cycle (see `apply`)? */
  private sidebarSent = false
  private overlaySent = false
  private toolbarSent = false

  constructor(private readonly d: BroadcastDeps) {}

  /**
   * Batches changes: layout recomputation, state sending, save.
   * By default everything is redone; `layout: false` (state only) and `persist: false` (nothing to write) lighten the cycle.
   */
  changed(opts?: ChangeOpts): void {
    // Any change that is not a plain tab field change requires the full state.
    if (!opts?.patch) this.needsFull = true
    if (opts?.layout !== false) this.layoutDirty = true
    if (opts?.persist !== false) {
      // Only title or icon changed: the write can wait a long time (they come back on reload).
      if (!this.persistDirty) this.persistLazy = opts?.lazy === true
      else if (!opts?.lazy) this.persistLazy = false
      this.persistDirty = true
      if (!opts?.slow && !opts?.lazy) this.persistFast = true
    }
    if (opts?.soft) {
      // Engine event: broadcast batched over a short window, unless one is already planned.
      if (this.pending) return
      this.pending = true
      this.applyTimer = setTimeout(() => this.apply(false), SOFT_APPLY_MS)
      return
    }
    if (this.pending && !this.applyTimer) return
    // A gesture occurs while a soft broadcast is waiting: it goes out right away.
    if (this.applyTimer) clearTimeout(this.applyTimer)
    this.applyTimer = null
    this.pending = true
    setImmediate(() => this.apply(false))
  }

  /** `soft`: change caused by a page event (not a gesture), batched broadcast. */
  patchTab(tabId: string, patch: Partial<TabInfo>, opts?: { soft?: boolean }): void {
    const { store } = this.d
    const tab = store.state.tabs[tabId]
    if (!tab) return
    const keys = Object.keys(patch) as (keyof TabInfo)[]
    if (keys.every((k) => tab[k] === patch[k])) return
    Object.assign(tab, patch)
    // Tab metadata does not touch the layout; we only write if a persisted field changes.
    const metaOnly = keys.every((k) => META_FIELDS.has(k))
    if (patch.loading === false && this.d.layout.shownTabs.includes(tabId)) this.d.previews.onLoaded(tabId)
    const persisted = keys.filter((k) => PERSISTED_FIELDS.has(k))
    const patchable = keys.every((k) => PATCH_FIELDS.has(k))
    if (patchable) {
      const set = this.tabPatches.get(tabId) ?? new Set<keyof TabInfo>()
      for (const k of keys) set.add(k)
      this.tabPatches.set(tabId, set)
    }
    this.changed({
      patch: patchable,
      layout: !metaOnly,
      persist: persisted.length > 0,
      // Title and icon alone change often (dynamic pages): the write can wait.
      lazy: persisted.every((k) => k === 'title' || k === 'favicon'),
      soft: opts?.soft === true || keys.every((k) => ENGINE_FIELDS.has(k))
    })
  }

  /** The state sent to the UIs. */
  snapshot(): Snapshot {
    const { store, layout, floating, compact, media, overlay } = this.d
    const { state, detached } = store
    const tabs = Object.fromEntries(Object.entries(state.tabs).filter(([id]) => !detached.has(id)))
    const sidebarOn = isSidebarOn(state)
    return {
      tabs,
      favorites: state.favorites,
      // Bookmarks of other spaces (potentially heavy) are not used by any view: only the active space's bar reads them.
      spaces: state.spaces.map((s) => (s.id === state.activeSpaceId || !s.bookmarks ? s : { ...s, bookmarks: undefined })),
      activeSpaceId: state.activeSpaceId,
      sidebarVisible: sidebarOn,
      sidebarWidth: layout.liveSidebarWidth ?? state.sidebarWidth,
      panes: layout.panes,
      settings: state.settings,
      downloads: state.downloads,
      sidebarFloating: floating.shown && !sidebarOn,
      toolbarFloating: compact.floating && state.settings.compactMode,
      dividers: layout.dividers,
      media: media.current(displayedTabIds(state, activeSpace(state))),
      boostedSites: Object.values(state.boosts)
        .filter((boost) => boost.enabled)
        .map((boost) => boost.site),
      overlayOpen: overlay.open || overlay.leaving
    }
  }

  /** `full`: direct call, layout and save included; otherwise only what `changed()` requested. */
  apply(full = true): void {
    const { store, layout, floating, compact, overlay } = this.d
    this.pending = false
    // Invariant of tab groups and their headers, whatever path modified the lists.
    for (const space of store.state.spaces) syncGroups(space, store.state.tabs)
    if (this.applyTimer) clearTimeout(this.applyTimer)
    this.applyTimer = null
    // View menu checks: rebuilt only when one of these states changes, whatever the path.
    const menuKey = this.d.menu.key()
    if (menuKey !== this.menuKey) {
      this.menuKey = menuKey
      this.d.menu.rebuild()
    }
    const doLayout = full || this.layoutDirty
    // Opening/closing a panel changes nothing persisted: only `changed()` requests the write.
    const persist = this.persistDirty
    const fast = this.persistFast
    const lazy = this.persistLazy
    this.persistLazy = false
    this.layoutDirty = false
    this.persistDirty = false
    this.persistFast = false
    const win = this.d.window.win
    let snapshot: Snapshot | null = null
    if (win && !win.isDestroyed()) {
      if (doLayout) layout.update(win)
      const sidebarShown = (floating.shown || floating.leaving) && !isSidebarOn(store.state)
      const toolbarShown = store.state.settings.compactMode && (compact.floating || compact.leaving)
      const overlayWanted = overlay.popoverOpen && overlay.ready
      const patches = this.tabPatches
      let patch: StatePatch | null = null
      if (
        !full &&
        !this.needsFull &&
        patches.size > 0 &&
        sidebarShown === this.sidebarSent &&
        toolbarShown === this.toolbarSent &&
        overlayWanted === this.overlaySent
      ) {
        // Only already-sent tab fields, and nothing derived (media) moved: a small message is enough.
        const tabs: Record<string, Partial<TabInfo>> = {}
        for (const [id, keys] of patches) {
          const tab = store.state.tabs[id]
          if (!tab || !this.sentTabIds.has(id) || store.detached.has(id)) {
            patch = null
            break
          }
          const part: Record<string, unknown> = {}
          for (const k of keys) part[k] = tab[k]
          tabs[id] = part as Partial<TabInfo>
          patch = { tabs }
        }
        if (patch && JSON.stringify(this.d.media.current(displayedTabIds(store.state, activeSpace(store.state)))) !== this.sentMedia) patch = null
      }
      const sendAll = <C extends 'state' | 'state:patch'>(channel: C, ...args: UiEventMap[C]): void => {
        pushUi(win.webContents, channel, ...args)
        if (sidebarShown || this.sidebarSent) floating.sidebar?.send(channel, ...args)
        if (toolbarShown || this.toolbarSent) compact.send(channel, ...args)
        if (overlayWanted && overlay.contents) pushUi(overlay.contents, channel, ...args)
      }
      if (patch) {
        this.tabPatches = new Map()
        sendAll('state:patch', patch)
      } else {
        snapshot = this.snapshot()
        this.tabPatches = new Map()
        this.needsFull = false
        this.sentTabIds = new Set(Object.keys(snapshot.tabs))
        this.sentMedia = JSON.stringify(snapshot.media)
        this.overlaySent = overlayWanted
        pushUi(win.webContents, 'state', snapshot)
        // Hidden floating views receive nothing, except a last state when they hide
        // (without it, they would never see `…Floating` go back to false).
        if (sidebarShown || this.sidebarSent) floating.sidebar?.send('state', snapshot)
        this.sidebarSent = sidebarShown
        if (toolbarShown || this.toolbarSent) compact.send('state', snapshot)
        this.toolbarSent = toolbarShown
        // A popover shows the current state (spaces, folder, downloads): it follows the updates.
        if (overlayWanted && overlay.contents) pushUi(overlay.contents, 'state', snapshot)
      }
    }
    this.d.satellites(snapshot ?? undefined)
    if (persist) store.schedule(fast ? undefined : lazy ? LAZY_PERSIST_MS : SLOW_PERSIST_MS)
  }
}

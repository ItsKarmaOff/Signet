// Per-tab page display state that is not persisted: progress, load error, desktop version, text size.
// Outside the main store (none of this survives the app) but outside components too: a WebView that is unloaded then reloaded
// gets its settings back (a tab in desktop version stays that way).
import { create } from 'zustand'
import type { ReaderPrefs } from '@signet/shared/mobile/reader'

export interface LoadFailure {
  code: number
  description: string
  /** Address that could not be loaded. */
  url: string
}

export interface TabView {
  /** 0 to 1. */
  progress: number
  error: LoadFailure | null
  desktop: boolean
  /** Text size in percent (100 = normal). */
  zoom: number
  /** The page has a login form and saved logins match it (page menu: "Fill password"). */
  canFill: boolean
}

export const DEFAULT_TAB_VIEW: TabView = { progress: 0, error: null, desktop: false, zoom: 100, canFill: false }

export const MIN_ZOOM = 70
export const MAX_ZOOM = 200
export const ZOOM_STEP = 10

interface ViewState {
  byTab: Record<string, TabView>
  patch: (id: string, patch: Partial<TabView>) => void
  /** Forgets tabs that no longer exist. */
  prune: (keep: readonly string[]) => void
  /** Reader settings, shared by all tabs (until the app is closed). */
  readerPrefs: ReaderPrefs | null
  setReaderPrefs: (prefs: ReaderPrefs) => void
}

export const useViewStore = create<ViewState>((set) => ({
  byTab: {},
  patch: (id, patch) =>
    set((s) => {
      const cur = s.byTab[id] ?? DEFAULT_TAB_VIEW
      const next = { ...cur, ...patch }
      if (next.progress === cur.progress && next.error === cur.error && next.desktop === cur.desktop && next.zoom === cur.zoom && next.canFill === cur.canFill)
        return s
      return { byTab: { ...s.byTab, [id]: next } }
    }),
  prune: (keep) =>
    set((s) => {
      const ids = Object.keys(s.byTab)
      if (ids.every((id) => keep.includes(id))) return s
      return { byTab: Object.fromEntries(ids.filter((id) => keep.includes(id)).map((id) => [id, s.byTab[id]])) }
    }),
  readerPrefs: null,
  setReaderPrefs: (readerPrefs) => set({ readerPrefs })
}))

export const useTabView = (id: string | undefined): TabView => useViewStore((s) => (id ? s.byTab[id] : undefined) ?? DEFAULT_TAB_VIEW)

export const patchTabView = (id: string, patch: Partial<TabView>) => useViewStore.getState().patch(id, patch)

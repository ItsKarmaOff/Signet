import type { StateCreator } from 'zustand'
import {
  createWorkspace,
  activateTab,
  addSpace,
  closePrivateTabs,
  closeTab,
  closeTodayTabsBatch,
  duplicateTab,
  moveTabToSpace,
  reopenTabs,
  editSpace,
  favoriteTab,
  moveSpace,
  moveTab,
  openTab,
  pinTab,
  removeSpace,
  reopenTab,
  setActiveSpace,
  setPrivateMode,
  tabLocation,
  updateTab,
  isPrivateTab
} from '@signet/shared/mobile/workspace'
import { newTabUrl } from '@signet/shared/mobile/settings'
import { newId } from '@signet/shared/ids'
import { t } from '@/lib/i18n'
import type { AppState, WorkspaceSlice } from '../types'

export const createWorkspaceSlice: StateCreator<AppState, [], [], WorkspaceSlice> = (set, get) => ({
  workspace: createWorkspace(t('spaces.defaultName')),
  lastClosed: null,
  lastClosedBatch: null,

  openTab: (opts) => {
    // Without an address, a new tab follows the "home page" setting.
    const id = newId()
    const url = opts.url || newTabUrl(get().settings)
    set((s) => ({ workspace: openTab(s.workspace, { ...opts, url, id }) }))
    return id
  },

  closeTab: (id) =>
    set((s) => {
      const tab = s.workspace.tabs[id]
      const at = tabLocation(s.workspace, id)
      const keep = tab && at && !isPrivateTab(s.workspace, id) ? { tab, at } : s.lastClosed
      return { workspace: closeTab(s.workspace, id), lastClosed: keep }
    }),

  undoCloseTab: () => set((s) => (s.lastClosed ? { workspace: reopenTab(s.workspace, s.lastClosed.tab, s.lastClosed.at), lastClosed: null } : s)),

  activateTab: (id) => set((s) => ({ workspace: activateTab(s.workspace, id) })),
  updateTab: (id, patch) => set((s) => ({ workspace: updateTab(s.workspace, id, patch) })),
  moveTab: (id, to) => set((s) => ({ workspace: moveTab(s.workspace, id, to) })),
  pinTab: (id, pinned = true) => set((s) => ({ workspace: pinTab(s.workspace, id, pinned) })),
  favoriteTab: (id, favorite = true) => set((s) => ({ workspace: favoriteTab(s.workspace, id, favorite) })),
  closeTodayTabs: (spaceId) =>
    set((s) => {
      const { workspace, batch } = closeTodayTabsBatch(s.workspace, spaceId)
      return batch ? { workspace, lastClosedBatch: batch } : s
    }),
  undoCloseTodayTabs: () => set((s) => (s.lastClosedBatch ? { workspace: reopenTabs(s.workspace, s.lastClosedBatch), lastClosedBatch: null } : s)),
  duplicateTab: (id) => {
    const copy = newId()
    set((s) => ({ workspace: duplicateTab(s.workspace, id, copy) }))
    return copy
  },
  moveTabToSpace: (id, spaceId) => set((s) => ({ workspace: moveTabToSpace(s.workspace, id, spaceId) })),
  setPrivateMode: (on) => set((s) => ({ workspace: setPrivateMode(s.workspace, on) })),
  closePrivateTabs: () => set((s) => ({ workspace: closePrivateTabs(s.workspace) })),
  setActiveSpace: (id) => set((s) => ({ workspace: setActiveSpace(s.workspace, id) })),

  addSpace: (draft) => {
    set((s) => ({ workspace: addSpace(s.workspace, draft, t('spaces.defaultName')) }))
    return get().workspace.activeSpaceId
  },
  editSpace: (id, draft) => set((s) => ({ workspace: editSpace(s.workspace, id, draft) })),
  removeSpace: (id) => set((s) => ({ workspace: removeSpace(s.workspace, id) })),
  moveSpace: (id, to) => set((s) => ({ workspace: moveSpace(s.workspace, id, to) })),

  reportNavigation: (id, page, patch) => {
    const state = get()
    const tab = state.workspace.tabs[id]
    if (!tab) return
    const next: Partial<typeof tab> = { ...patch, url: page.url }
    if (page.title !== undefined) next.title = page.title
    if (page.favicon !== undefined) next.favicon = page.favicon
    state.updateTab(id, next)
    // Never any history in private browsing.
    // (and only once the page has loaded: during loading, the title is still the previous page's).
    if (!isPrivateTab(state.workspace, id) && patch?.loading !== true) state.addVisit(page)
  }
})

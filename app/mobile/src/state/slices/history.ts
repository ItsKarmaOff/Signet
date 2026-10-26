import type { StateCreator } from 'zustand'
import { addVisit, clearHistory, removeFromHistory, restoreHistoryEntry, setHistoryFavicon, setHistoryTitle } from '@signet/shared/mobile/history'
import type { AppState, HistorySlice } from '../types'

export const createHistorySlice: StateCreator<AppState, [], [], HistorySlice> = (set) => ({
  history: [],
  addVisit: (visit) =>
    set((s) => {
      // Same page (up to the fragment) already at the top: we only fill in the title, without counting another visit.
      const top = s.history[0]
      if (top && top.url.replace(/#.*$/, '') === visit.url.replace(/#.*$/, '') && Date.now() - top.lastVisit < 3000) {
        return { history: visit.title ? setHistoryTitle(s.history, visit.url, visit.title) : s.history }
      }
      return { history: addVisit(s.history, visit) }
    }),
  removeHistoryEntry: (url) => set((s) => ({ history: removeFromHistory(s.history, url) })),
  clearHistory: (since) => set((s) => ({ history: clearHistory(s.history, since) })),
  restoreHistoryEntry: (entry) => set((s) => ({ history: restoreHistoryEntry(s.history, entry) })),
  setHistoryFavicon: (url, favicon) => set((s) => ({ history: setHistoryFavicon(s.history, url, favicon) }))
})

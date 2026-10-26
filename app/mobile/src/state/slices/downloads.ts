// Downloads: in-memory list, saved separately from the main document (own key) so this slice stays independent.
// Read when the slice starts, delayed write on every change. A download left "in progress" becomes "failed" on re-read.
import AsyncStorage from '@react-native-async-storage/async-storage'
import type { StateCreator } from 'zustand'
import { addDownload, clearDownloads, parseDownloads, removeDownload, serializeDownloads, updateDownload } from '@signet/shared/mobile/downloads'
import type { AppState, DownloadsSlice } from '../types'

export const DOWNLOADS_KEY = 'signet.mobile.downloads'
const WRITE_DELAY = 400

export const createDownloadsSlice: StateCreator<AppState, [], [], DownloadsSlice> = (set, _get, api) => {
  let loaded = false
  let timer: ReturnType<typeof setTimeout> | undefined

  AsyncStorage.getItem(DOWNLOADS_KEY)
    .then((text) => {
      let stored: ReturnType<typeof parseDownloads> = []
      try {
        stored = parseDownloads(text ? JSON.parse(text) : [])
      } catch {
        // unreadable text: empty list
      }
      // Whatever was added before reading finished goes first.
      set((s) => ({ downloads: [...s.downloads, ...stored.filter((d) => !s.downloads.some((x) => x.id === d.id))] }))
    })
    .catch(() => undefined)
    .finally(() => {
      loaded = true
    })

  api.subscribe((s, prev) => {
    if (!loaded || s.downloads === prev.downloads) return
    if (timer) clearTimeout(timer)
    timer = setTimeout(() => {
      timer = undefined
      AsyncStorage.setItem(DOWNLOADS_KEY, serializeDownloads(api.getState().downloads)).catch(() => undefined)
    }, WRITE_DELAY)
  })

  return {
    downloads: [],
    addDownload: (entry) => set((s) => ({ downloads: addDownload(s.downloads, entry) })),
    updateDownload: (id, patch) => set((s) => ({ downloads: updateDownload(s.downloads, id, patch) })),
    removeDownload: (id) => set((s) => ({ downloads: removeDownload(s.downloads, id) })),
    clearDownloads: () => set((s) => ({ downloads: clearDownloads(s.downloads) }))
  }
}

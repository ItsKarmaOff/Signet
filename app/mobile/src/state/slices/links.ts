// Paired computers: in-memory list, saved separately from the main document (own key), like downloads.
// Each link's token is NOT here: see `features/link/secrets.ts` (expo-secure-store).
import AsyncStorage from '@react-native-async-storage/async-storage'
import type { StateCreator } from 'zustand'
import { LINKS_STORAGE_KEY, addLink, parseLinks, patchLink, removeLink, serializeLinks } from '@signet/shared/mobile/link'
import type { AppState, LinksSlice } from '../types'

const WRITE_DELAY = 300

export const createLinksSlice: StateCreator<AppState, [], [], LinksSlice> = (set, _get, api) => {
  let loaded = false
  let timer: ReturnType<typeof setTimeout> | undefined

  AsyncStorage.getItem(LINKS_STORAGE_KEY)
    .then((text) => {
      let stored: ReturnType<typeof parseLinks> = []
      try {
        stored = parseLinks(text ? JSON.parse(text) : [])
      } catch {
        // unreadable text: no link
      }
      // A link added before reading finishes goes first.
      set((s) => ({ links: [...s.links, ...stored.filter((l) => !s.links.some((x) => x.linkId === l.linkId))] }))
    })
    .catch(() => undefined)
    .finally(() => {
      loaded = true
      set({ linksLoaded: true })
    })

  api.subscribe((s, prev) => {
    if (!loaded || s.links === prev.links) return
    if (timer) clearTimeout(timer)
    timer = setTimeout(() => {
      timer = undefined
      AsyncStorage.setItem(LINKS_STORAGE_KEY, serializeLinks(api.getState().links)).catch(() => undefined)
    }, WRITE_DELAY)
  })

  return {
    links: [],
    linksLoaded: false,
    addLink: (link) => set((s) => ({ links: addLink(s.links, link) })),
    patchLink: (linkId, patch) => set((s) => ({ links: patchLink(s.links, linkId, patch) })),
    removeLink: (linkId) => set((s) => ({ links: removeLink(s.links, linkId) }))
  }
}

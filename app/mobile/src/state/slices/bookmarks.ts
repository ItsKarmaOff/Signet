import type { StateCreator } from 'zustand'
import {
  addBookmark,
  addFolder,
  importBookmarks,
  insertBookmark,
  isBookmarked,
  moveBookmark,
  moveBookmarkBy,
  removeBookmark,
  removeBookmarkByUrl,
  setBookmarkFavicon,
  updateBookmark
} from '@signet/shared/mobile/bookmarks'
import type { AppState, BookmarksSlice } from '../types'

export const createBookmarksSlice: StateCreator<AppState, [], [], BookmarksSlice> = (set, get) => ({
  bookmarks: [],
  addBookmark: (link, folderId) => set((s) => ({ bookmarks: addBookmark(s.bookmarks, link, folderId) })),
  addBookmarkFolder: (name, parentId) => set((s) => ({ bookmarks: addFolder(s.bookmarks, name, parentId) })),
  removeBookmark: (id) => set((s) => ({ bookmarks: removeBookmark(s.bookmarks, id) })),
  removeBookmarkByUrl: (url) => set((s) => ({ bookmarks: removeBookmarkByUrl(s.bookmarks, url) })),
  updateBookmark: (id, patch) => set((s) => ({ bookmarks: updateBookmark(s.bookmarks, id, patch) })),
  moveBookmark: (id, target) => set((s) => ({ bookmarks: moveBookmark(s.bookmarks, id, target) })),
  insertBookmark: (node, folderId, index) => set((s) => ({ bookmarks: insertBookmark(s.bookmarks, node, folderId, index) })),
  moveBookmarkBy: (id, delta) => set((s) => ({ bookmarks: moveBookmarkBy(s.bookmarks, id, delta) })),
  importBookmarks: (nodes, folderName) => set((s) => ({ bookmarks: importBookmarks(s.bookmarks, nodes, folderName) })),
  setBookmarkFavicon: (url, favicon) => set((s) => ({ bookmarks: setBookmarkFavicon(s.bookmarks, url, favicon) })),
  toggleBookmark: (link) => {
    const had = isBookmarked(get().bookmarks, link.url)
    set((s) => ({ bookmarks: had ? removeBookmarkByUrl(s.bookmarks, link.url) : addBookmark(s.bookmarks, link) }))
    return !had
  }
})

import type { Browser } from '../browser/browser'
import { showAddFolderMenu, showFolderMenu, showGroupMenu } from '../app/menu'
import type { Registrar } from './registrar'

/** Sidebar: folders, groups, live folders, bookmarks, width, today tabs. */
export function registerSidebarIpc(b: Browser, { handle }: Registrar): void {
  handle('createFolder', () => b.folders.create())
  handle('showAddFolderMenu', () => showAddFolderMenu(b))
  handle('renameFolder', (_c, id, name) => b.folders.rename(id, name))
  handle('toggleFolder', (_c, id, open) => b.folders.toggle(id, open))
  handle('showFolderMenu', (_c, id) => showFolderMenu(b, id))
  handle('renameGroup', (_c, id, name) => b.groups.rename(id, name))
  handle('toggleGroup', (_c, id, open) => b.groups.toggle(id, open))
  handle('showGroupMenu', (_c, id) => showGroupMenu(b, id))

  handle('saveLiveFolder', (_c, folderId, source) => b.liveFolders.save(folderId, source))
  handle('refreshLiveFolder', (_c, folderId) => b.liveFolders.refresh(folderId))
  handle('hasGithubToken', () => b.liveFolders.hasToken())
  handle('setGithubToken', (_c, token) => b.liveFolders.setToken(token))

  handle('toggleBookmark', (_c, tabId) => b.bookmarks.toggle(tabId))
  handle('addBookmark', (_c, tabId, target) => b.bookmarks.add(tabId, target))
  handle('moveBookmark', (_c, id, target) => b.bookmarks.move(id, target))
  handle('openBookmark', (_c, id, tabId, newTab) => b.bookmarks.open(id, tabId, newTab))
  handle('renameBookmark', (_c, id, name) => b.bookmarks.rename(id, name))
  handle('showBookmarkMenu', (_c, id, tabId) => b.bookmarks.showMenu(id, tabId))
  handle('showBookmarksBarMenu', () => b.bookmarks.showBarMenu())

  handle('toggleSidebar', () => b.shell.toggleSidebar())
  handle('setSidebarWidth', (_c, width, live) => b.shell.setSidebarWidth(width, live === true))
  handle('setSidebarHover', (_c, hover) => b.floating.setHover(hover))
  handle('clearToday', () => b.tabs.clearToday())
  handle('undoArchive', (_c, id) => b.undo.undo(id))
  handle('toggleCompact', () => b.shell.toggleCompact())
  handle('setToolbarHover', (_c, hover) => b.compact.hover(hover))
}

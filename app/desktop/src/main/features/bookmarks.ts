// Bookmarks: one bar per space, under the address bar of each panel (can be shown or hidden, ⌘⇧B).
import { randomUUID } from 'node:crypto'
import type { MenuItemConstructorOptions } from 'electron'
import type { Bookmark, BookmarkTarget, Space } from '@shared/types'
import type { Browser } from '../browser/browser'
import { t } from '../app/i18n'
import { buildMenu, menuIcon } from '../app/menuicons'
import { pushUi } from '../ipc/push'

/** Height of the bookmarks bar (same value as the interface's `BOOKMARK_BAR`). */
export const BOOKMARK_BAR = 32

type BookmarkFolder = Extract<Bookmark, { kind: 'folder' }>
interface Slot {
  list: Bookmark[]
  index: number
  item: Bookmark
}

const MAX_DEPTH = 20
const norm = (url: string) => url.replace(/\/$/, '')

function findIn(list: Bookmark[], pred: (b: Bookmark) => boolean): Slot | null {
  for (let index = 0; index < list.length; index++) {
    const item = list[index]
    if (pred(item)) return { list, index, item }
    if (item.kind === 'folder') {
      const inner = findIn(item.items, pred)
      if (inner) return inner
    }
  }
  return null
}

/** True if `id` designates `folder` itself or one of its subfolders. */
const contains = (folder: BookmarkFolder, id: string): boolean => folder.id === id || folder.items.some((i) => i.kind === 'folder' && contains(i, id))

/** Cleans bookmarks read from disk (shape, depth, lengths). */
export function pruneBookmarks(items: unknown, depth = 0): Bookmark[] {
  if (!Array.isArray(items) || depth > MAX_DEPTH) return []
  return items.flatMap((i): Bookmark[] => {
    if (!i || typeof i !== 'object') return []
    const id = typeof i.id === 'string' && i.id ? i.id : randomUUID()
    if (i.kind === 'link' && typeof i.url === 'string' && i.url) {
      return [
        {
          kind: 'link',
          id,
          url: i.url,
          title: typeof i.title === 'string' ? i.title.slice(0, 300) : i.url,
          ...(typeof i.favicon === 'string' && i.favicon ? { favicon: i.favicon } : {})
        }
      ]
    }
    if (i.kind === 'folder') {
      return [
        {
          kind: 'folder',
          id,
          name: typeof i.name === 'string' && i.name ? i.name.slice(0, 120) : t('defaults.folderFallback'),
          items: pruneBookmarks(i.items, depth + 1)
        }
      ]
    }
    return []
  })
}

/** True if the address can become a bookmark (not a blank page or a data address). */
const bookmarkable = (url: string) => /^(https?|file|signet):/i.test(url)

export class Bookmarks {
  constructor(private b: Browser) {}

  get visible(): boolean {
    return this.b.store.state.settings.showBookmarksBar !== false
  }

  /** ⌘⇧B: shows or hides the bar (the web card gains or loses its height). */
  toggleBar(): void {
    this.b.store.state.settings.showBookmarksBar = !this.visible
    this.b.broadcast.changed()
  }

  /** Bookmark mutation: state broadcast and bookmarks.json rewritten. */
  private persist(): void {
    this.b.store.scheduleBookmarks()
    this.b.broadcast.changed()
  }

  private list(space: Space = this.b.space): Bookmark[] {
    return (space.bookmarks ??= [])
  }

  private find(id: string, space?: Space): Slot | null {
    return findIn(this.list(space), (i) => i.id === id)
  }

  /** Bookmark for this address in the active space, wherever it is filed. */
  findUrl(url: string): Slot | null {
    if (!url) return null
    const n = norm(url)
    return findIn(this.list(), (i) => i.kind === 'link' && norm(i.url) === n)
  }

  /** Targeted list: bar root or folder; null if the folder no longer exists. */
  private target(folderId?: string): Bookmark[] | null {
    if (!folderId) return this.list()
    const slot = this.find(folderId)
    return slot?.item.kind === 'folder' ? slot.item.items : null
  }

  private linkFor(tabId: string): Bookmark | null {
    const tab = this.b.store.state.tabs[tabId]
    if (!tab || !bookmarkable(tab.url)) return null
    return { kind: 'link', id: randomUUID(), url: tab.url, title: tab.customTitle || tab.title || tab.url, ...(tab.favicon ? { favicon: tab.favicon } : {}) }
  }

  /** ⌘D and star: adds the page at the end of the bar, or removes its bookmark if it already exists. */
  toggle(tabId: string | null): void {
    const id = tabId ?? this.b.space.activeTabId
    const tab = id ? this.b.store.state.tabs[id] : undefined
    if (!id || !tab) return
    const existing = this.findUrl(tab.url)
    if (existing) existing.list.splice(existing.index, 1)
    else {
      const link = this.linkFor(id)
      if (!link) return
      this.list().push(link)
    }
    this.persist()
  }

  /** Tab dropped on the bar (or into a bookmarks folder). */
  add(tabId: string, target: BookmarkTarget): void {
    const list = this.target(target?.folderId)
    const link = this.linkFor(tabId)
    if (!list || !link) return
    list.splice(Math.min(list.length, Math.max(0, target.index)), 0, link)
    this.persist()
  }

  move(id: string, target: BookmarkTarget): void {
    const from = this.find(id)
    const list = this.target(target?.folderId)
    if (!from || !list) return
    // Never a folder into itself or into one of its subfolders.
    if (from.item.kind === 'folder' && target.folderId && contains(from.item, target.folderId)) return
    from.list.splice(from.index, 1)
    let index = target.index
    if (from.list === list && from.index < index) index -= 1
    list.splice(Math.min(list.length, Math.max(0, index)), 0, from.item)
    this.persist()
  }

  /** In the panel's tab (otherwise the active tab, otherwise a new tab), or in a new background tab. */
  open(id: string, tabId: string | null, newTab: boolean): void {
    const slot = this.find(id)
    if (slot?.item.kind !== 'link') return
    const url = slot.item.url
    const target = tabId ?? this.b.space.activeTabId
    if (newTab || !target || !this.b.store.state.tabs[target]) this.b.tabs.open(url, { activate: !newTab })
    else this.b.tabs.loadUrl(target, url)
  }

  /** Opens all the links of the folder (subfolders included) in new tabs. */
  private openAll(folder: BookmarkFolder): void {
    const links: string[] = []
    const walk = (items: Bookmark[]) => items.forEach((i) => (i.kind === 'link' ? links.push(i.url) : walk(i.items)))
    walk(folder.items)
    for (const url of links.reverse()) this.b.tabs.open(url, { activate: false })
  }

  rename(id: string, name: string): void {
    const slot = this.find(id)
    const clean = String(name ?? '').trim()
    if (!slot || !clean) return
    if (slot.item.kind === 'link') slot.item.title = clean.slice(0, 300)
    else slot.item.name = clean.slice(0, 120)
    this.persist()
  }

  private remove(id: string): void {
    const slot = this.find(id)
    if (!slot) return
    slot.list.splice(slot.index, 1)
    this.persist()
  }

  /** New folder at the end of the bar, immediately in rename mode. */
  private newFolder(): void {
    const folder: BookmarkFolder = { kind: 'folder', id: randomUUID(), name: t('defaults.bookmarkFolder'), items: [] }
    this.list().push(folder)
    if (!this.visible) this.b.store.state.settings.showBookmarksBar = true
    this.persist()
    this.requestRename(folder.id)
  }

  private requestRename(id: string): void {
    // Let the bar redraw before looking for the bookmark on screen.
    setTimeout(() => {
      if (this.b.win) pushUi(this.b.win.webContents, 'bookmark:rename', { id })
    }, 80)
  }

  showMenu(id: string, tabId: string | null): void {
    const slot = this.find(id)
    if (!slot) return
    const item = slot.item
    const items: MenuItemConstructorOptions[] =
      item.kind === 'link'
        ? [
            { label: t('menu.bookmark.open'), click: () => this.open(id, tabId, false) },
            { label: t('menu.bookmark.openInNewTab'), icon: menuIcon('openInNewTab'), click: () => this.open(id, tabId, true) },
            { type: 'separator' },
            { label: t('menu.bookmark.rename'), click: () => this.requestRename(id) },
            { label: t('menu.bookmark.delete'), icon: menuIcon('delete'), click: () => this.remove(id) }
          ]
        : [
            { label: t('menu.bookmark.openAll'), enabled: item.items.length > 0, click: () => this.openAll(item) },
            { type: 'separator' },
            { label: t('menu.bookmark.rename'), click: () => this.requestRename(id) },
            { label: t('menu.bookmark.deleteFolder'), icon: menuIcon('delete'), click: () => this.remove(id) }
          ]
    items.push({ type: 'separator' }, { label: t('menu.bookmark.newFolder'), click: () => this.newFolder() })
    buildMenu(items).popup({ window: this.b.win ?? undefined })
  }

  showBarMenu(): void {
    buildMenu([
      { label: t('menu.bookmark.newFolder'), click: () => this.newFolder() },
      { type: 'separator' },
      { label: t('menu.view.hideBookmarksBar'), click: () => this.toggleBar() }
    ]).popup({ window: this.b.win ?? undefined })
  }
}

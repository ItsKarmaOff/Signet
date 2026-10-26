import { BrowserWindow, type WebContents } from 'electron'
import { windowChrome } from '../app/platform'
import { HISTORY_QUERY_MAX } from '@shared/types'
import type { ArchivedEntry, HistoryEntry, IconTarget, LibrarySection, Snapshot } from '@shared/types'
import type { Browser } from '../browser/browser'
import { fold, loadRoute, preloadPath } from '../core/urls'
import { t } from '../app/i18n'
import { itemTabIds } from '../core/tree'
import { pushUi } from '../ipc/push'

const SECTIONS: LibrarySection[] = ['history', 'archive', 'downloads', 'spaces']
/** Unique id set at archiving; older entries keep the old schema. */
const archiveId = (e: { id?: string; archivedAt: number; url: string }) => e.id ?? `${e.archivedAt}:${e.url}`

/** Library window (history, archives, downloads, spaces): a single instance, reused. */
export class LibraryWindow {
  private win: BrowserWindow | null = null
  private ready = false
  /** Key of the fields the page reads, at the last state sent: a state that does not change it is not sent again. */
  private lastKey = ''

  constructor(private b: Browser) {}

  /** True if `wc` is the Library page. */
  owns(wc: WebContents): boolean {
    return !!this.win && !this.win.isDestroyed() && this.win.webContents === wc
  }

  open(section: LibrarySection = 'history'): void {
    const wanted = SECTIONS.includes(section) ? section : 'history'
    if (this.win && !this.win.isDestroyed()) {
      if (this.win.isMinimized()) this.win.restore()
      this.win.show()
      this.win.focus()
      this.sendSection(wanted)
      return
    }
    const win = new BrowserWindow({
      width: 920,
      height: 660,
      minWidth: 700,
      minHeight: 460,
      title: t('window.library'),
      ...windowChrome({ lights: { x: 16, y: 18 }, vibrancy: true }),
      webPreferences: { preload: preloadPath, sandbox: true, contextIsolation: true, nodeIntegration: false }
    })
    this.win = win
    this.ready = false
    this.lastKey = ''
    // Links open in a new tab of the main window.
    win.webContents.setWindowOpenHandler(({ url }) => {
      if (/^https?:\/\//i.test(url)) {
        this.b.window.show()
        this.b.tabs.open(url, { activate: true })
      }
      return { action: 'deny' }
    })
    win.webContents.on('did-finish-load', () => {
      this.ready = true
      this.lastKey = ''
      this.sendSection(wanted)
      this.push()
      setTimeout(() => this.sendSection(wanted), 150) // gives React time to subscribe
    })
    win.on('closed', () => {
      this.win = null
      this.lastKey = ''
    })
    loadRoute(win.webContents, '#library')
  }

  /** Window title in the current language. */
  retitle(): void {
    if (this.win && !this.win.isDestroyed()) this.win.setTitle(t('window.library'))
  }

  private sendSection(section: LibrarySection): void {
    const wc = this.win && !this.win.isDestroyed() ? this.win.webContents : null
    if (wc && this.ready && !wc.isDestroyed()) pushUi(wc, 'library:section', section)
  }

  /** Relays the state (downloads, spaces) to the open window. */
  push(snapshot?: Snapshot): void {
    const wc = this.win && !this.win.isDestroyed() ? this.win.webContents : null
    if (!wc || !this.ready || wc.isDestroyed()) return
    // The page only reads the downloads, the active space and, per space, name, icon, tint, profile and tab counts.
    const { state } = this.b.store
    const key = JSON.stringify([
      state.downloads,
      state.activeSpaceId,
      state.spaces.map((sp) => [sp.id, sp.name, sp.icon, sp.hue, sp.profile, sp.pinned.flatMap(itemTabIds).length, sp.today.flatMap(itemTabIds).length])
    ])
    if (key === this.lastKey) return
    this.lastKey = key
    pushUi(wc, 'state', snapshot ?? this.b.broadcast.snapshot())
  }

  // ───────────────────────── Historique ─────────────────────────

  queryHistory(query: string, offset: number, limit: number): HistoryEntry[] {
    const n = Math.min(Math.max(Math.floor(Number(limit)) || 100, 1), HISTORY_QUERY_MAX)
    return this.b.history.list(String(query ?? ''), Math.max(0, Math.floor(Number(offset)) || 0), n)
  }

  deleteHistory(url: string): void {
    this.b.history.remove(url)
    this.b.broadcast.changed() // updates the Settings counters
  }

  // ───────────────────────── Archives ─────────────────────────

  listArchive(query: string): ArchivedEntry[] {
    const q = fold(String(query ?? '').trim())
    return this.b.store.state.archive
      .filter((e) => !q || fold(e.title).includes(q) || fold(e.url).includes(q))
      .map((e) => ({ id: archiveId(e), url: e.url, title: e.title, favicon: e.favicon, archivedAt: e.archivedAt }))
  }

  /** Reopens the archived tab in the active space and removes it from the archives. */
  restoreArchived(id: string): void {
    const { state } = this.b.store
    const entry = state.archive.find((e) => archiveId(e) === id)
    if (!entry) return
    state.archive = state.archive.filter((e) => e !== entry)
    this.b.window.show()
    const tabId = this.b.tabs.open(entry.url, { activate: true })
    this.b.broadcast.patchTab(tabId, { title: entry.title, favicon: entry.favicon })
  }

  deleteArchived(id: string): void {
    const { state } = this.b.store
    state.archive = state.archive.filter((e) => archiveId(e) !== id)
    this.b.broadcast.changed()
  }

  // ───────────────────────── Spaces ─────────────────────────

  /** "Edit" button of a space card: form in the main window's sidebar. */
  editSpace(spaceId: string): void {
    this.b.window.show()
    const hidden = !this.b.store.state.sidebarVisible
    this.b.store.state.sidebarVisible = true
    this.b.broadcast.changed()
    // Sidebar hidden: give React time to mount it.
    if (hidden) setTimeout(() => this.b.spaces.edit(spaceId), 300)
    else this.b.spaces.edit(spaceId)
  }
}

// ───────────────────────── A tab's name and icon ─────────────────────────

/** Custom title; `null` or empty = go back to the page title. */
export function renameTab(b: Browser, tabId: string, title: string | null): void {
  const clean = typeof title === 'string' ? title.trim().slice(0, 200) : ''
  b.broadcast.patchTab(tabId, { customTitle: clean || undefined })
}

/** First displayed character of a chosen emoji; absent if empty (back to the original icon). */
export function firstEmoji(emoji: unknown): string | undefined {
  if (typeof emoji !== 'string' || !emoji.trim()) return undefined
  const first = [...new Intl.Segmenter(undefined, { granularity: 'grapheme' }).segment(emoji.trim())][0]
  return first?.segment.slice(0, 16) || undefined
}

/** Custom emoji; `null` = go back to the favicon. */
export function setTabIcon(b: Browser, tabId: string, emoji: string | null): void {
  b.broadcast.patchTab(tabId, { customIcon: firstEmoji(emoji) })
}

/** Emoji of a tab, a folder or a tab group; `null` = original icon. */
export function setIcon(b: Browser, target: IconTarget, emoji: string | null): void {
  if (target?.kind === 'tab') setTabIcon(b, target.tabId, emoji)
  else if (target?.kind === 'folder') b.folders.setIcon(target.folderId, firstEmoji(emoji))
  else if (target?.kind === 'group') b.groups.setIcon(target.groupId, firstEmoji(emoji))
}

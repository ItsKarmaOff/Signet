// Live folders: pinned folders filled by a source (RSS or Atom feed, GitHub pull requests)
// and updated on their own. Tabs are attached to each item's address: they survive updates.
import { app, net, safeStorage } from 'electron'
import { existsSync, readFileSync } from 'node:fs'
import { unlink, writeFile } from 'node:fs/promises'
import { join } from 'node:path'
import type { GithubQuery, Item, LiveSource, Space } from '@shared/types'
import type { Browser } from '../../browser/browser'
import { newTab } from '../../core/store'
import { findFolderSlot, insertAt, newFolder, tabItem, type Folder } from '../../core/tree'
import { cleanLiveSource, discoverFeed, looksLikeHtml, parseFeed, type FeedEntry } from '@signet/shared/feeds'
import { toUrl } from '../../core/urls'
import { t } from '../../app/i18n'

/** Items kept per folder (the most recent). */
const MAX_ITEMS = 15
const REFRESH_EVERY = 15 * 60_000
/** First update after startup: the window and tabs first. */
const FIRST_DELAY = 8000
const TIMEOUT = 15_000
const MAX_BYTES = 5 * 1024 * 1024
const TOKEN_FILE = 'github-token.bin'
const GITHUB_QUERIES: Record<GithubQuery, string> = {
  'review-requested': 'is:pr is:open archived:false review-requested:@me',
  mine: 'is:pr is:open archived:false author:@me'
}

/** Update failure, with a message translated for the UI. */
class LiveError extends Error {}

/** Live folders of a pinned list (nested folders included). */
function liveFoldersOf(items: Item[], out: Folder[] = []): Folder[] {
  for (const item of items) {
    if (item.kind !== 'folder') continue
    if (item.live) out.push(item)
    liveFoldersOf(item.items, out)
  }
  return out
}

export class LiveFolders {
  /** Folders being updated (only one at a time per folder). */
  private busy = new Set<string>()

  constructor(private b: Browser) {}

  /** "New" dots recomputed right away, first update a bit later, then every 15 minutes. */
  start(): void {
    for (const { folder } of this.all()) this.markUnread(folder)
    this.b.broadcast.changed({ layout: false, persist: false })
    setTimeout(() => void this.refreshAll(), FIRST_DELAY)
    setInterval(() => void this.refreshAll(), REFRESH_EVERY)
  }

  private all(): { folder: Folder; space: Space }[] {
    return this.b.store.state.spaces.flatMap((space) => liveFoldersOf(space.pinned).map((folder) => ({ folder, space })))
  }

  private find(folderId: string): { folder: Folder; space: Space } | null {
    for (const space of this.b.store.state.spaces) {
      const folder = findFolderSlot(space.pinned, folderId)?.item
      if (folder) return { folder, space }
    }
    return null
  }

  /** Creation form (`folderId` null) or source change, placed above the pages near the sidebar. */
  openForm(folderId: string | null): void {
    const win = this.b.win
    if (!win) return
    const { width } = win.getContentBounds()
    const { sidebarWidth } = this.b.store.state
    const right = this.b.layout.sidebarRight
    const anchor = this.b.layout.horizontal
      ? { x: 78, y: 6, width: 140, height: 28 }
      : { x: right ? Math.max(8, width - sidebarWidth - 312) : sidebarWidth + 8, y: 52, width: 1, height: 1 }
    this.b.overlay.openPopover({ kind: 'live-folder', folderId }, anchor)
  }

  refreshAll(): Promise<void> {
    return Promise.all(this.all().map(({ folder }) => this.refresh(folder.id))).then(() => {})
  }

  /** Creates a live folder at the top of the active space's pinned tabs, or changes a folder's source; error message or null. */
  async save(folderId: string | null, input: LiveSource): Promise<string | null> {
    const source = this.normalize(input)
    if (!source) return t('liveFolders.error.address')
    let feed: { title: string; entries: FeedEntry[] }
    try {
      feed = await this.fetchSource(source)
    } catch (error) {
      return error instanceof LiveError ? error.message : t('liveFolders.error.network')
    }
    const { title: feedTitle, entries } = feed
    const name =
      source.kind === 'github'
        ? t(source.query === 'mine' ? 'liveFolders.github.mine' : 'liveFolders.github.reviewRequested')
        : feedTitle || this.hostOf(source.url)
    if (folderId) {
      const found = this.find(folderId)
      if (!found) return null
      found.folder.live = { source, seen: [] }
      this.apply(found.folder, found.space, entries, true)
    } else {
      const space = this.b.space
      const folder: Folder = { ...newFolder(name.slice(0, 80) || t('defaults.folder')), live: { source, seen: [] } }
      insertAt(space.pinned, 0, folder)
      this.apply(folder, space, entries, true)
    }
    this.b.broadcast.changed()
    return null
  }

  /** Updates a folder; a failure keeps the previous items and records the error. */
  async refresh(folderId: string): Promise<void> {
    const found = this.find(folderId)
    const live = found?.folder.live
    if (!found || !live || this.busy.has(folderId)) return
    this.busy.add(folderId)
    live.refreshing = true
    this.b.broadcast.changed({ persist: false })
    let entries: FeedEntry[] | null = null
    let error = ''
    const sourceKey = JSON.stringify(live.source)
    try {
      entries = (await this.fetchSource(live.source)).entries
    } catch (e) {
      error = e instanceof LiveError ? e.message : t('liveFolders.error.network')
    } finally {
      this.busy.delete(folderId)
    }
    // The folder may have been deleted, moved or had its source changed in the meantime.
    const now = this.find(folderId)
    if (!now?.folder.live) return
    delete now.folder.live.refreshing
    // Source changed during the request: the result no longer applies to it.
    if (JSON.stringify(now.folder.live.source) !== sourceKey) {
      this.b.broadcast.changed()
      return
    }
    if (entries) this.apply(now.folder, now.space, entries, false)
    else now.folder.live.error = error
    this.b.broadcast.changed()
  }

  /** Displayed tab: if it comes from a live folder, it is no longer "new". */
  onActivated(tabId: string): void {
    const tab = this.b.store.state.tabs[tabId]
    if (!tab?.liveUnread) return
    const url = tab.pinnedUrl ?? tab.url
    for (const { folder } of this.all()) {
      if (!folder.items.some((i) => i.kind === 'tab' && i.tabId === tabId)) continue
      const live = folder.live!
      live.seen = [...new Set([...(live.seen ?? []), url])].slice(-200)
      tab.liveUnread = false
    }
  }

  // ───────────────────────── GitHub token ─────────────────────────

  private get tokenFile(): string {
    return join(app.getPath('userData'), TOKEN_FILE)
  }

  hasToken(): boolean {
    return existsSync(this.tokenFile)
  }

  private token(): string | null {
    try {
      return safeStorage.decryptString(readFileSync(this.tokenFile))
    } catch {
      return null
    }
  }

  /** Checks the token with GitHub then saves it encrypted (`null` removes it); error message or null. */
  async setToken(input: string | null): Promise<string | null> {
    if (input === null) {
      await unlink(this.tokenFile).catch(() => {})
      return null
    }
    const token = String(input).trim()
    if (!/^[\w-]{20,255}$/.test(token)) return t('liveFolders.error.tokenFormat')
    if (!safeStorage.isEncryptionAvailable()) return t('liveFolders.error.encryption')
    try {
      const res = await net.fetch('https://api.github.com/user', { headers: this.githubHeaders(token), signal: AbortSignal.timeout(TIMEOUT) })
      if (res.status === 401 || res.status === 403) return t('liveFolders.error.tokenInvalid')
      if (!res.ok) return t('liveFolders.error.network')
    } catch {
      return t('liveFolders.error.network')
    }
    await writeFile(this.tokenFile, safeStorage.encryptString(token), { mode: 0o600 })
    // GitHub folders waiting for a token are filled right away.
    for (const { folder } of this.all()) if (folder.live?.source.kind === 'github') void this.refresh(folder.id)
    return null
  }

  // ───────────────────────── Sources ─────────────────────────

  private normalize(input: LiveSource): LiveSource | null {
    if (input?.kind === 'feed' && typeof input.url === 'string') {
      const url = toUrl(input.url.trim())
      return cleanLiveSource({ kind: 'feed', url: url ?? '' })
    }
    return cleanLiveSource(input)
  }

  private hostOf(url: string): string {
    try {
      return new URL(url).hostname.replace(/^www\./, '')
    } catch {
      return url
    }
  }

  private async fetchSource(source: LiveSource): Promise<{ title: string; entries: FeedEntry[] }> {
    if (source.kind === 'github') return { title: '', entries: await this.fetchGithub(source.query) }
    let url = source.url
    let body = await this.fetchText(url)
    // An ordinary page: we follow the feed it announces.
    if (looksLikeHtml(body)) {
      const found = discoverFeed(body, url)
      if (!found) throw new LiveError(t('liveFolders.error.notFeed'))
      url = found
      body = await this.fetchText(url)
    }
    const feed = parseFeed(body, url)
    if (!feed) throw new LiveError(t('liveFolders.error.notFeed'))
    return feed
  }

  private async fetchText(url: string): Promise<string> {
    let res: Response
    try {
      res = await net.fetch(url, {
        headers: { accept: 'application/rss+xml, application/atom+xml, application/xml;q=0.9, text/html;q=0.8, */*;q=0.5' },
        signal: AbortSignal.timeout(TIMEOUT)
      })
    } catch {
      throw new LiveError(t('liveFolders.error.network'))
    }
    if (!res.ok) throw new LiveError(t('liveFolders.error.http', { status: res.status }))
    const text = await res.text()
    return text.length > MAX_BYTES ? text.slice(0, MAX_BYTES) : text
  }

  private githubHeaders(token: string): Record<string, string> {
    return { authorization: `Bearer ${token}`, accept: 'application/vnd.github+json', 'x-github-api-version': '2022-11-28' }
  }

  private async fetchGithub(query: GithubQuery): Promise<FeedEntry[]> {
    const token = this.token()
    if (!token) throw new LiveError(t('liveFolders.error.noToken'))
    const q = encodeURIComponent(GITHUB_QUERIES[query])
    let res: Response
    try {
      res = await net.fetch(`https://api.github.com/search/issues?q=${q}&sort=updated&order=desc&per_page=${MAX_ITEMS}`, {
        headers: this.githubHeaders(token),
        signal: AbortSignal.timeout(TIMEOUT)
      })
    } catch {
      throw new LiveError(t('liveFolders.error.network'))
    }
    if (res.status === 401) throw new LiveError(t('liveFolders.error.tokenInvalid'))
    if (!res.ok) throw new LiveError(t('liveFolders.error.http', { status: res.status }))
    const data = (await res.json()) as {
      items?: { html_url?: string; title?: string; number?: number; repository_url?: string; updated_at?: string }[]
    }
    return (data.items ?? []).flatMap((it) => {
      if (typeof it.html_url !== 'string' || !/^https:\/\/github\.com\//.test(it.html_url)) return []
      const repo = (it.repository_url ?? '').split('/').slice(-2).join('/')
      const title = `${it.title ?? ''}${repo ? ` · ${repo}#${it.number ?? ''}` : ''}`.trim()
      return [{ url: it.html_url, title: title || it.html_url, date: Date.parse(it.updated_at ?? '') || 0 }]
    })
  }

  // ───────────────────────── Folder tabs ─────────────────────────

  /**
   * Puts the items in the folder: one tab per address (existing tabs are kept), from newest to oldest. An item that
   * disappeared from the source is removed, unless its tab is displayed. `first`: everything is already "seen".
   */
  private apply(folder: Folder, space: Space, all: FeedEntry[], first: boolean): void {
    const live = folder.live!
    const { tabs } = this.b.store.state
    // Newest to oldest when the source dates its items; otherwise the source order.
    const dated = all.filter((e) => e.date > 0).length >= all.length / 2
    const sorted = dated ? [...all].sort((x, y) => y.date - x.date) : all
    const entries: FeedEntry[] = []
    const urls = new Set<string>()
    for (const e of sorted) {
      if (urls.has(e.url)) continue
      urls.add(e.url)
      entries.push(e)
      if (entries.length >= MAX_ITEMS) break
    }

    const byUrl = new Map<string, Item>()
    const others: Item[] = []
    for (const item of folder.items) {
      const tab = item.kind === 'tab' ? tabs[item.tabId] : undefined
      if (tab) byUrl.set(tab.pinnedUrl ?? tab.url, item)
      else others.push(item)
    }
    const shown = new Set(this.b.broadcast.snapshot().panes.map((p) => p.tabId))
    const next: Item[] = []
    for (const e of entries) {
      const existing = byUrl.get(e.url)
      if (existing && existing.kind === 'tab') {
        byUrl.delete(e.url)
        const tab = tabs[existing.tabId]
        // Source title until the page has loaded (it then provides its own).
        if (tab && !tab.loaded) tab.title = e.title
        next.push(existing)
      } else {
        const tab = newTab(e.url, e.title)
        tab.pinnedUrl = e.url
        tabs[tab.id] = tab
        next.push(tabItem(tab.id))
      }
    }
    // Gone from the source: kept only if displayed (active tab, panel of a split view).
    for (const item of byUrl.values()) {
      if (item.kind === 'tab' && (space.activeTabId === item.tabId || shown.has(item.tabId))) next.push(item)
      else if (item.kind === 'tab') this.b.views.forget(item.tabId)
    }
    folder.items = [...next, ...others]
    live.seen = first ? [...urls] : (live.seen ?? []).filter((u) => urls.has(u))
    live.refreshedAt = Date.now()
    delete live.error
    this.markUnread(folder)
  }

  /** "New" dot on the folder's tabs whose address has not been opened yet. */
  private markUnread(folder: Folder): void {
    const seen = new Set(folder.live?.seen ?? [])
    for (const item of folder.items) {
      const tab = item.kind === 'tab' ? this.b.store.state.tabs[item.tabId] : undefined
      if (tab) tab.liveUnread = !seen.has(tab.pinnedUrl ?? tab.url)
    }
  }
}

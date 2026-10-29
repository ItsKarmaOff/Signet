import type { BrowserWindow, Session, WebContents, WebContentsView } from 'electron'
import { sessionFor } from '../web/profiles'
import { isUiAddress } from '../core/urls'
import { resolveInternal } from '../web/internal'
import { trimHistory, type Store, type TabHistory } from '../core/store'
import type { Extensions } from '../features/extensions/extensions'
import { locate } from '../model/locate'

/** Delay for batching history copies after a burst of navigations. */
const HISTORY_QUEUE_MS = 250

export interface ViewsDeps {
  store: Store
  window: { readonly win: BrowserWindow | null }
  /** Creates a tab's web view, listeners included (see tabs.ts). */
  create: (tabId: string, ses: Session) => WebContentsView
  extensions: Pick<Extensions, 'track' | 'whenReady'>
  /** HTTPS-only mode: the address to load for a tab (`url` itself when it stays as is). */
  upgradeUrl: (tabId: string, ses: Session, url: string) => string
  /** State broadcast (called when a tab is forgotten). */
  changed: () => void
}

/**
 * Web view lifecycle: one `WebContentsView` per loaded tab, created on demand and destroyed on sleep
 * or on closing. Also holds the `webContents` ↔ tab mappings, the sessions and the navigation history copy.
 */
export class Views {
  private views = new Map<string, WebContentsView>()
  /** Split view → tab, to find a tab from its `webContents` without walking all the views. */
  private tabByContents = new Map<number, string>()
  private contentsByTab = new Map<string, number>()
  /** Session of the detached tabs opened from a sidebar tab (Peek). */
  private detachedSessions = new Map<string, Session>()
  /** Tabs being unloaded (closing that waits for `beforeunload`). */
  readonly discarding = new Set<string>()
  private historyDirty = new Set<string>()
  private historyTimer: NodeJS.Timeout | null = null
  private goneListeners: ((tabId: string) => void)[] = []
  private releaseListeners: ((view: WebContentsView) => void)[] = []

  constructor(private readonly d: ViewsDeps) {}

  /** Called at the start of a view's destruction (the mini player gives the page back, sleep forgets the tab…). */
  onGone(fn: (tabId: string) => void): void {
    this.goneListeners.push(fn)
  }

  /** Called when a view is removed: the layout takes it out of its stack. */
  onRelease(fn: (view: WebContentsView) => void): void {
    this.releaseListeners.push(fn)
  }

  viewOf(tabId: string): WebContentsView | undefined {
    return this.views.get(tabId)
  }

  has(tabId: string): boolean {
    return this.views.has(tabId)
  }

  /** Identifiers of the tabs that have a live view. */
  ids(): IterableIterator<string> {
    return this.views.keys()
  }

  all(): IterableIterator<WebContentsView> {
    return this.views.values()
  }

  /** Creates a tab's view on demand. */
  ensureView(tabId: string, win: BrowserWindow | null = this.d.window.win): WebContentsView {
    const existing = this.views.get(tabId)
    if (existing && !existing.webContents.isDestroyed() && !this.discarding.has(tabId)) return existing
    const { store } = this.d
    const ses = this.sessionOfTab(tabId)
    const view = this.d.create(tabId, ses)
    // Single replacement for the view being closed: later calls find this one.
    this.discarding.delete(tabId)
    this.views.set(tabId, view)
    this.indexView(tabId, view)
    this.d.extensions.track(view.webContents, win)
    const tab = store.state.tabs[tabId]
    tab.loaded = true
    view.webContents.setAudioMuted(tab.muted)
    // Isolated partition opened for the first time: its extension scripts must be in place before the page.
    const ready = this.d.extensions.whenReady(ses)
    const load = () => this.loadTab(tabId, view.webContents, store.state.tabs[tabId]?.url ?? tab.url, store.histories.get(tabId))
    if (!ready) load()
    else {
      void ready.then(() => {
        const wc = view.webContents
        // A navigation started in the meantime (typed address) takes priority.
        if (!wc.isDestroyed() && !wc.getURL() && !wc.isLoading()) load()
      })
    }
    return view
  }

  /** Restores the history if its active entry matches the tab's URL, otherwise loads the URL alone. */
  private loadTab(tabId: string, wc: WebContents, tabUrl: string, history?: TabHistory): void {
    // Address saved as signetui: (never legitimate: notes are written as signet://note/…): nothing is loaded.
    const url = tabUrl && !isUiAddress(tabUrl) ? resolveInternal(tabUrl) : ''
    // HTTPS-only: an http address is opened over https (without restoring its history, which holds the http entry).
    const target = url ? this.d.upgradeUrl(tabId, wc.session, url) : url
    const plain = () => {
      if (target && !wc.isDestroyed()) wc.loadURL(target).catch(() => {})
    }
    if (target !== url) return plain()
    if (!history || history.entries[history.index]?.url !== url) return plain()
    try {
      wc.navigationHistory.restore({ entries: history.entries.map((e) => ({ ...e })), index: history.index }).catch(plain)
    } catch {
      plain()
    }
  }

  // ───────────────────────── Navigation history ─────────────────────────

  /** Requests the copy of a tab's history: batched, only once per burst of navigations. */
  queueHistory(tabId: string): void {
    this.historyDirty.add(tabId)
    if (this.historyTimer) return
    this.historyTimer = setTimeout(() => {
      this.historyTimer = null
      const ids = [...this.historyDirty]
      this.historyDirty.clear()
      for (const id of ids) this.writeHistory(id)
    }, HISTORY_QUEUE_MS)
  }

  /** Copies the view's history into the store right away (bounds applied). */
  saveHistory(tabId: string): void {
    this.historyDirty.delete(tabId)
    this.writeHistory(tabId)
  }

  private writeHistory(tabId: string): void {
    const { store } = this.d
    const wc = this.views.get(tabId)?.webContents
    if (!wc || wc.isDestroyed() || store.detached.has(tabId) || !store.state.tabs[tabId]) return
    try {
      const nav = wc.navigationHistory
      const trimmed = trimHistory(
        nav.getAllEntries().map((e) => ({ url: e.url, title: e.title, pageState: e.pageState })),
        nav.getActiveIndex()
      )
      if (!trimmed) return
      const old = store.histories.get(tabId)
      if (old && old.index === trimmed.index && old.entries.length === trimmed.entries.length) {
        const same = old.entries.every((e, i) => {
          const n = trimmed.entries[i]
          return e.url === n.url && e.title === n.title && e.pageState === n.pageState
        })
        if (same) return
      }
      store.histories.set(tabId, trimmed)
    } catch {
      // view being destroyed: we keep the previous history
    }
  }

  saveAllHistories(): void {
    this.historyDirty.clear()
    for (const id of this.views.keys()) this.saveHistory(id)
  }

  // ───────────────────────── Sessions ─────────────────────────

  /**
   * Session of a tab: its space's if isolated, the default session otherwise
   * (favorites, Little Signet). A Peek keeps that of the tab it comes from.
   */
  sessionOfTab(tabId: string): Session {
    const inherited = this.detachedSessions.get(tabId)
    if (inherited) return inherited
    return sessionFor(locate(this.d.store.state, tabId)?.space?.id ?? null)
  }

  /** Detached tab living in a given session (private window tab, Peek). */
  inheritSession(tabId: string, ses: Session): void {
    this.detachedSessions.set(tabId, ses)
  }

  /** The detached tab receives a slot: it takes its space's session again. */
  dropSession(tabId: string): void {
    this.detachedSessions.delete(tabId)
  }

  /** Destroys the views that are no longer in the right session (profile or space change); they reload when displayed. */
  reconcileSessions(tabIds: string[]): void {
    for (const id of tabIds) {
      const view = this.views.get(id)
      if (view && !view.webContents.isDestroyed() && view.webContents.session !== this.sessionOfTab(id)) this.destroyView(id)
    }
  }

  // ───────────────────────── Mappings ─────────────────────────

  /** Tab whose web view is this `webContents`. */
  tabIdOfContents(id: number): string | null {
    return this.tabByContents.get(id) ?? null
  }

  /** Keeps the `webContents` → tab mapping up to date (replaces the tab's old entry). */
  private indexView(tabId: string, view: WebContentsView | null): void {
    const old = this.contentsByTab.get(tabId)
    if (old !== undefined) this.tabByContents.delete(old)
    this.contentsByTab.delete(tabId)
    if (!view) return
    const id = view.webContents.id
    this.tabByContents.set(id, tabId)
    this.contentsByTab.set(tabId, id)
  }

  // ───────────────────────── Destruction ─────────────────────────

  /** Destroys a tab's view; the tab stays in the state, unloaded. */
  destroyView(tabId: string): void {
    const view = this.views.get(tabId)
    const tab = this.d.store.state.tabs[tabId]
    for (const fn of this.goneListeners) fn(tabId)
    this.saveHistory(tabId)
    if (tab) Object.assign(tab, { loaded: false, loading: false, audible: false })
    if (!view) return
    this.views.delete(tabId)
    this.indexView(tabId, null)
    for (const fn of this.releaseListeners) fn(view)
    // Reentrant closing: the extensions library calls `tabs.close` again when the webContents is destroyed, view already emptied.
    const wc = view.webContents as WebContents | undefined
    if (wc && !wc.isDestroyed()) wc.close()
  }

  /** Destroys the view and removes the tab from the state (detached or permanently closed tab). */
  forget(tabId: string): void {
    const { store } = this.d
    this.destroyView(tabId)
    delete store.state.tabs[tabId]
    store.histories.delete(tabId)
    store.detached.delete(tabId)
    this.detachedSessions.delete(tabId)
    this.d.changed()
  }
}

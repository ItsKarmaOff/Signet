import { app, type BrowserWindow, type WebContentsView } from 'electron'
import type { Store } from '../core/store'
import type { DevTools } from '../devtools/devtools'
import type { Media } from './media'
import type { Pip } from '../windows/pip'
import type { Permissions } from '../web/permissions'
import type { Views } from './views'

/** Minutes out of view before a tab is put to sleep (its view destroyed), with no setting; checked every minute. */
const DEFAULT_SLEEP_MINUTES = 30
const DISCARD_INTERVAL = 60_000
/** Delay before giving up on a sleep whose page does not respond. */
const DISCARD_TIMEOUT = 10_000

export interface SleepDeps {
  store: Store
  window: { readonly win: BrowserWindow | null }
  views: Pick<Views, 'ids' | 'viewOf' | 'discarding' | 'saveHistory' | 'destroyView' | 'onGone'>
  pip: Pick<Pip, 'owns'>
  devtools: Pick<DevTools, 'isOpen'>
  media: Pick<Media, 'isActive'>
  permissions: Pick<Permissions, 'usesDevices'>
  /** Tabs displayed in the main window. */
  shown: () => readonly string[]
  /** Current Peek tab. */
  peekTabId: () => string | null
  /** True during a tab drag: nothing is unloaded. */
  paused: () => boolean
  changed: (opts: { persist: false }) => void
}

/** Sleep for tabs hidden for a long time: the view is destroyed, the address and history remain. */
export class TabSleep {
  /** Since when each live view is no longer displayed (ms); used for automatic unloading. */
  private hiddenSince = new Map<string, number>()
  private timer: NodeJS.Timeout | null = null

  constructor(private readonly d: SleepDeps) {
    d.views.onGone((tabId) => this.hiddenSince.delete(tabId))
  }

  /** Periodic check, as long as the main window exists. */
  start(): void {
    this.timer = setInterval(() => this.discardIdle(), DISCARD_INTERVAL)
    this.timer.unref()
  }

  stop(): void {
    if (this.timer) clearInterval(this.timer)
    this.timer = null
    this.hiddenSince.clear()
  }

  /** Notes since when each live view has been hidden (displayed ones are removed from tracking). */
  trackHidden(shown: string[]): void {
    const now = Date.now()
    for (const id of this.d.views.ids()) {
      if (shown.includes(id)) this.hiddenSince.delete(id)
      else if (!this.hiddenSince.has(id)) this.hiddenSince.set(id, now)
    }
  }

  /** Unloads tabs hidden for a long time; same path as a pinned tab's "Unload", the address and history are kept. */
  private discardIdle(): void {
    const { store, views } = this.d
    if (!this.d.window.win || this.d.paused()) return
    const { state } = store
    // An ongoing download may come from a hidden tab: nothing is unloaded as long as it lasts.
    if (state.downloads.some((d) => d.status === 'progressing')) return
    // Setting "Put hidden tabs to sleep after": 0 = never.
    const minutes = state.settings.tabSleepMinutes ?? DEFAULT_SLEEP_MINUTES
    if (!Number.isFinite(minutes) || minutes <= 0) return
    const limit = Date.now() - minutes * 60_000
    for (const [id, since] of [...this.hiddenSince]) {
      const tab = state.tabs[id]
      const view = views.viewOf(id)
      if (since > limit || !tab || !view || views.discarding.has(id)) continue
      // Tabs of other windows (private, apps…): their history is not kept, so we do not unload them.
      if (store.detached.has(id)) continue
      if (this.d.shown().includes(id) || id === this.d.peekTabId() || this.d.pip.owns(id) || this.d.devtools.isOpen(id)) continue
      if (tab.audible || tab.loading || this.d.media.isActive(id) || this.d.permissions.usesDevices(view.webContents)) continue
      // Development: each sleep in the console, to check the real delay.
      if (!app.isPackaged) console.log(`[sleep] ${tab.title || tab.url} — hidden for ${Math.round((Date.now() - since) / 60_000)} min`)
      this.discard(id, view)
    }
  }

  /** Closes the page while honoring `beforeunload`: a page that refuses (unsaved draft) stays loaded. */
  private discard(tabId: string, view: WebContentsView): void {
    const { views } = this.d
    const wc = view.webContents
    if (wc.isDestroyed()) return
    views.discarding.add(tabId)
    views.saveHistory(tabId)
    // The engine never responds: we do not block the tab for all that.
    const timeout = setTimeout(() => {
      if (views.viewOf(tabId) === view) views.discarding.delete(tabId)
    }, DISCARD_TIMEOUT)
    timeout.unref()
    const refused = () => {
      clearTimeout(timeout)
      wc.removeListener('destroyed', gone)
      // Replaced in the meantime (ensureView): the old page has no reason to live anymore.
      const cur = views.viewOf(tabId)
      if (cur && cur !== view) return void (!wc.isDestroyed() && wc.close())
      views.discarding.delete(tabId)
      if (this.hiddenSince.has(tabId)) this.hiddenSince.set(tabId, Date.now()) // try again later
    }
    const gone = () => {
      clearTimeout(timeout)
      wc.removeListener('will-prevent-unload', refused)
      const cur = views.viewOf(tabId)
      if (cur && cur !== view) return
      views.discarding.delete(tabId)
      if (cur !== view) return
      views.destroyView(tabId)
      this.d.changed({ persist: false })
    }
    wc.once('will-prevent-unload', refused)
    wc.once('destroyed', gone)
    wc.close({ waitForBeforeUnload: true })
  }
}

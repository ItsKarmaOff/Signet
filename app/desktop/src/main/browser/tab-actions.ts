import { clipboard, type WebContents } from 'electron'
import { matchRoute } from '@signet/shared/routes'
import type { PaletteMode } from '@shared/types'
import { resolveAlias } from '@shared/devhosts'
import type { Store } from '../core/store'
import { httpFallback, isHttpsFallbackError, isUiAddress, isUpgradedInput, engineSearchUrl, toUrl } from '../core/urls'
import type { Model } from '../model'
import type { OpenOptions } from '../model/opening'
import { isHorizontal } from '../model/chrome'
import type { WebApps } from '../features/webapps'
import { resolveInternal } from '../web/internal'
import type { LittleWindows } from '../windows/little'
import type { PrivateWindows } from '../windows/private'
import type { Changed } from './broadcast'
import { reloadContents } from './tabs'
import type { Overlay } from './overlay'
import type { Peek } from './peek'
import type { Undo } from './undo'
import type { Views } from './views'

/** Interval of the check for today's tabs left too long without activation. */
const SWEEP_INTERVAL = 5 * 60_000

export interface TabActionsDeps {
  store: Store
  model: Model
  views: Pick<Views, 'viewOf' | 'tabIdOfContents' | 'dropSession' | 'saveHistory'>
  changed: Changed
  undo: Pick<Undo, 'announce'>
  overlay: Pick<Overlay, 'close'>
  peek: Pick<Peek, 'tabId'>
  privates: Pick<PrivateWindows, 'owns' | 'discardBlank' | 'ownsBar' | 'tabIdForBar'>
  webApps: Pick<WebApps, 'ownsBar' | 'tabIdForBar'>
  little: Pick<LittleWindows, 'open' | 'tabIdForBar'>
  showMain: () => void
  /** Address that opens an app screen (settings, library…) rather than a tab: handled, nothing to open. */
  openShortcut: (url: string) => boolean
  /** Opens the address in a split view with the active tab. */
  openInSplit: (url: string) => void
  /** Tab that sound, the mini player, media or developer tools keep open. */
  busy: (tabId: string) => boolean
}

/** Opening, navigation, closing and activation of tabs, and commands on the tab's page. */
export class TabActions {
  /** Last active tab change, to give the original page back at the start of a drag (see drag.ts). */
  private lastSwitch: { tabId: string; before: string; at: number } | null = null
  /** Addresses typed without a scheme and promoted to https://: http fallback possible if they fail. */
  private upgraded = new Map<string, string>()

  constructor(private readonly d: TabActionsDeps) {
    setInterval(() => this.sweepArchive(), SWEEP_INTERVAL).unref()
  }

  // ───────────────────────── Activation ─────────────────────────

  activate(tabId: string): void {
    const result = this.d.model.activation.activate(tabId)
    if (!result) return
    const { before } = result
    if (before && before !== tabId) this.lastSwitch = { tabId, before, at: Date.now() }
    this.d.changed()
  }

  /** The last active tab change, only once. */
  takeLastSwitch(): { tabId: string; before: string; at: number } | null {
    const last = this.lastSwitch
    this.lastSwitch = null
    return last
  }

  /** Next (+1) or previous (-1) tab: favorites, pinned, then today's tabs. */
  cycle(step: number): void {
    const id = this.d.model.cycleTarget(step)
    if (id) this.activate(id)
  }

  /** ⌘1 to ⌘8: nth tab as the bar shows it; ⌘9: the last one. */
  gotoNumber(n: number): void {
    const id = this.d.model.numberTarget(n, isHorizontal(this.d.store.state))
    if (id) this.activate(id)
  }

  /** Tab targeted by a command: that of the emitting Little Signet or private window, otherwise the active tab. */
  idForSender(sender: WebContents): string | null {
    const { privates, webApps, little, model } = this.d
    if (privates.ownsBar(sender)) return privates.tabIdForBar(sender)
    if (webApps.ownsBar(sender)) return webApps.tabIdForBar(sender)
    return little.tabIdForBar(sender) ?? model.space.activeTabId
  }

  // ───────────────────────── Opening ─────────────────────────

  /**
   * `afterTabId`: right after this today's tab (otherwise at the top or bottom, depending on the setting). `openerId`: tab from which the link was opened;
   * if it is among today's tabs of the current space, the new tab joins its group (created if needed), at the end.
   */
  open(url: string, opts: OpenOptions = {}): string {
    const id = this.d.model.opening.open(url, opts)
    this.d.changed()
    return id
  }

  duplicate(tabId: string): void {
    const tab = this.d.store.state.tabs[tabId]
    if (tab) this.open(tab.url, { activate: true, afterTabId: tabId })
  }

  openUrl(url: string, mode: PaletteMode): void {
    if (this.d.openShortcut(url)) return // Signet screen: no tab
    const active = this.d.model.activeTab()
    if (mode === 'edit-url' && active) this.loadUrl(active.id, url)
    else if (mode === 'split') this.d.openInSplit(url)
    else this.openRouted(url, false)
  }

  /** Opens a link in the space designated by a routing rule, otherwise in the active space. */
  openRouted(url: string, always: boolean): void {
    const { store, model } = this.d
    const rule = matchRoute(url, store.state.settings.routes)
    const target = rule && store.state.spaces.find((s) => s.id === rule.spaceId)
    if (!target || (!always && target.id === model.space.id)) {
      this.open(url, { activate: true })
      return
    }
    model.opening.openIn(target, url)
    this.d.changed()
  }

  /** Link received from outside: routed space in the main window, otherwise Little Signet. */
  openExternal(url: string): void {
    if (this.d.openShortcut(url)) return
    if (matchRoute(url, this.d.store.state.settings.routes)) {
      this.d.showMain()
      this.openRouted(url, true)
    } else this.d.little.open(url)
  }

  /** Gives a slot (among the active space's today's tabs, like a new tab) to a detached tab, and activates it. */
  adopt(tabId: string): void {
    this.d.views.dropSession(tabId)
    this.d.model.opening.adopt(tabId)
    this.d.changed()
  }

  // ───────────────────────── Navigation ─────────────────────────

  /** Called on a load failure: retries over http the input promoted to https, only once. */
  retryTypedHttp(tabId: string, failedUrl: string, code: number): boolean {
    const pending = this.upgraded.get(tabId)
    if (!pending) return false
    this.upgraded.delete(tabId)
    const http = isHttpsFallbackError(code) ? httpFallback(pending, failedUrl) : null
    if (!http) return false
    this.loadUrl(tabId, http)
    return true
  }

  /** Page validated: the input no longer needs a fallback. */
  clearTypedUpgrade(tabId: string): void {
    this.upgraded.delete(tabId)
  }

  loadUrl(tabId: string, url: string): void {
    const tab = this.d.store.state.tabs[tabId]
    this.upgraded.delete(tabId)
    if (!tab || isUiAddress(url) || this.d.openShortcut(url)) return
    tab.url = url
    this.d.views
      .viewOf(tabId)
      ?.webContents.loadURL(resolveInternal(url))
      .catch(() => {})
    this.d.changed()
  }

  navigate(tabId: string, input: string): void {
    // A local name (`mysite/admin`) takes precedence over everything: otherwise `mysite.test` looks like a domain.
    const aliased = resolveAlias(input, this.d.store.state.settings.devAliases)
    const typed = aliased ? null : toUrl(input)
    this.loadUrl(tabId, aliased ?? typed ?? engineSearchUrl(input))
    if (typed && isUpgradedInput(input, typed)) this.upgraded.set(tabId, typed) // after loadUrl, which clears it
  }

  resetPinned(tabId: string): void {
    const home = this.d.store.state.tabs[tabId]?.pinnedUrl
    if (home) this.loadUrl(tabId, home)
  }

  // ───────────────────────── Closing ─────────────────────────

  close(tabId: string): void {
    if (this.d.model.removal.close(tabId)) this.d.changed()
  }

  /** Silently closes the today's tab opened just for a download (never displayed), or the Peek that leads to it. */
  discardBlank(contentsId: number): void {
    const { store, model, views, privates, peek, overlay } = this.d
    const tabId = views.tabIdOfContents(contentsId)
    const tab = tabId ? store.state.tabs[tabId] : undefined
    if (!tabId || !tab || (tab.title && tab.url !== 'about:blank')) return
    if (privates.owns(tabId)) privates.discardBlank(tabId)
    else if (tabId === peek.tabId) overlay.close()
    else if (model.locate(tabId)?.zone === 'today') {
      model.removal.remove(tabId)
      this.d.changed()
    }
  }

  /** Reopens the last closed tab in its original place (otherwise in the current space), with its history. */
  reopenClosed(): void {
    const reopened = this.d.model.closed.reopen()
    if (!reopened) return
    this.d.changed()
    for (let k = 0; k < reopened.more; k++) this.reopenClosed()
  }

  /** Archives all of the current space's today's tabs; the UI offers to undo (`undo.undo`). */
  clearToday(): void {
    const archived = this.d.model.archive.clearToday()
    if (!archived) return
    this.d.changed()
    this.d.undo.announce('archived', archived.count, '', () => {
      if (archived.restore()) this.d.changed()
    })
  }

  /** Archives today's tabs left too long without activation (except active tabs). */
  private sweepArchive(): void {
    const { store, model, changed } = this.d
    const busy = (id: string) => !!store.state.tabs[id]?.audible || this.d.busy(id)
    if (model.archive.sweep(busy) > 0) changed()
  }

  // ───────────────────────── Commands on the page ─────────────────────────

  goBack(tabId: string | null): void {
    if (tabId) this.d.views.viewOf(tabId)?.webContents.navigationHistory.goBack()
  }

  goForward(tabId: string | null): void {
    if (tabId) this.d.views.viewOf(tabId)?.webContents.navigationHistory.goForward()
  }

  reload(tabId: string | null, force = false): void {
    const wc = tabId ? this.d.views.viewOf(tabId)?.webContents : undefined
    if (wc) reloadContents(wc, force)
  }

  /** Toolbar button: stops the load in progress, otherwise reloads. */
  reloadOrStop(tabId: string | null): void {
    const wc = tabId ? this.d.views.viewOf(tabId)?.webContents : undefined
    if (wc?.isLoading()) wc.stop()
    else if (wc) reloadContents(wc)
  }

  toggleMute(tabId: string): void {
    const tab = this.d.store.state.tabs[tabId]
    if (!tab) return
    tab.muted = !tab.muted
    this.d.views.viewOf(tabId)?.webContents.setAudioMuted(tab.muted)
    this.d.changed()
  }

  copyUrl(tabId: string | null): void {
    const tab = tabId ? this.d.store.state.tabs[tabId] : undefined
    if (tab) void clipboard.writeText(tab.url)
  }

  /** `delta` in zoom levels; 0 restores the actual size. */
  zoom(tabId: string | null, delta: number): void {
    const wc = tabId ? this.d.views.viewOf(tabId)?.webContents : undefined
    if (wc) wc.setZoomLevel(delta === 0 ? 0 : wc.getZoomLevel() + delta)
  }
}

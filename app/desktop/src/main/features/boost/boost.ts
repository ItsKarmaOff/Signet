import { BrowserWindow, type WebContents } from 'electron'
import { windowChrome } from '../../app/platform'
import type { Boost, BoostEditorState } from '@shared/types'
import type { Browser } from '../../browser/browser'
import { denyWindowOpen } from '../../web/guard'
import { loadRoute, preloadPath, siteOf } from '../../core/urls'
import { t } from '../../app/i18n'
import { ZAP_SCRIPT, ZAP_STOP_SCRIPT } from '../zap'
import { EMOJI_CSS, EMOJI_SCRIPT, EMOJI_STOP_SCRIPT, hasDarkPreset } from './emoji'
import { pushUi } from '../../ipc/push'

const WORLD = 1001
/** Isolated world of the emoji script (dark mode); 1002 is the media one. */
const EMOJI_WORLD = 1007
const isWeb = (url: string) => /^https?:\/\//i.test(url)

interface Editor {
  win: BrowserWindow
  ready: boolean
  last: string
}

interface Zap {
  site: string
  tabId: string
  wc: WebContents
}

/** Full CSS of a Boost: hidden elements first, so that malformed free CSS does not swallow them. */
function cssOf(boost: Boost): string {
  const hidden = boost.zapped.map((s) => `${s} { display: none !important; }`).join('\n')
  const emoji = hasDarkPreset(boost.css) ? `\n${EMOJI_CSS}` : ''
  return `${hidden}\n${boost.css}${emoji}`
}

/** Boosts: CSS application to tabs, edit windows (one per site) and the "Hide" tool. */
export class Boosts {
  private editors = new Map<string, Editor>()
  /** `insertCSS` key per webContents. */
  private keys = new Map<number, string>()
  /** Application queue per webContents: insertions and removals never overlap. */
  private chains = new Map<number, Promise<void>>()
  /** webContents where the emoji script runs (dark mode). */
  private emoji = new Set<number>()
  private zap: Zap | null = null

  constructor(private b: Browser) {}

  private get all(): Record<string, Boost> {
    return this.b.store.state.boosts
  }

  // ───────────────────────── CSS application ─────────────────────────

  /** Called when each tab view is created. */
  attach(wc: WebContents): void {
    const id = wc.id
    wc.on('dom-ready', () => {
      // The key stays: apply() removes it (no effect if it dates from the old document), leaving no orphan.
      this.emoji.delete(id)
      this.apply(wc)
    })
    wc.once('destroyed', () => {
      this.keys.delete(id)
      this.chains.delete(id)
      this.emoji.delete(id)
      if (this.zap?.wc === wc) this.endZap()
    })
  }

  private apply(wc: WebContents): void {
    const id = wc.id
    const run = async () => {
      if (wc.isDestroyed()) return
      const previous = this.keys.get(id)
      if (previous) {
        this.keys.delete(id)
        await wc.removeInsertedCSS(previous).catch(() => {})
      }
      const boost = this.all[siteOf(wc.getURL())]
      this.syncEmoji(wc, !!boost?.enabled && hasDarkPreset(boost.css))
      if (!boost?.enabled) return
      const css = cssOf(boost)
      if (!css.trim()) return
      const key = await wc.insertCSS(css).catch(() => null)
      if (key) this.keys.set(id, key)
    }
    this.chains.set(id, (this.chains.get(id) ?? Promise.resolve()).then(run, run))
  }

  /** Starts or stops the script that exempts emojis from dark mode. */
  private syncEmoji(wc: WebContents, dark: boolean): void {
    const id = wc.id
    if (dark === this.emoji.has(id)) return
    if (dark) this.emoji.add(id)
    else this.emoji.delete(id)
    const code = dark ? EMOJI_SCRIPT : EMOJI_STOP_SCRIPT
    wc.executeJavaScriptInIsolatedWorld(EMOJI_WORLD, [{ code }]).catch(() => {})
  }

  private refresh(site: string): void {
    for (const tabId of Object.keys(this.b.store.state.tabs)) {
      const view = this.b.views.viewOf(tabId)
      if (!view || view.webContents.isDestroyed()) continue
      if (siteOf(view.webContents.getURL()) === site) this.apply(view.webContents)
    }
  }

  /** True if at least one loaded tab of the site exists (same predicate as `tabsOf`, without building the list). */
  private hasTab(site: string): boolean {
    return Object.keys(this.b.store.state.tabs).some((id) => this.isLoadedAt(id, site))
  }

  private isLoadedAt(id: string, site: string): boolean {
    if (this.b.privates.owns(id)) return false
    const wc = this.b.views.viewOf(id)?.webContents
    return !!wc && !wc.isDestroyed() && siteOf(wc.getURL()) === site
  }

  /** Loaded tabs of the site, those open in the space first. */
  private tabsOf(site: string): string[] {
    // Private tabs are excluded: nothing from their browsing must end up in state.json.
    return Object.keys(this.b.store.state.tabs).filter((id) => this.isLoadedAt(id, site))
  }

  // ───────────────────────── Modifications ─────────────────────────

  private draft(site: string): Boost {
    return { site, enabled: true, css: '', zapped: [], updatedAt: 0 }
  }

  private edit(site: string, fn: (boost: Boost) => void): void {
    if (!site) return
    const boost = this.all[site] ?? this.draft(site)
    fn(boost)
    boost.updatedAt = Date.now()
    this.all[site] = boost
    this.b.store.schedule()
    this.refresh(site)
    this.push(site)
  }

  save(site: string, patch: { css?: string; enabled?: boolean }): void {
    this.edit(site, (boost) => {
      if (typeof patch.css === 'string') boost.css = patch.css
      if (typeof patch.enabled === 'boolean') boost.enabled = patch.enabled
    })
  }

  unzap(site: string, selector: string): void {
    if (!this.all[site]) return
    this.edit(site, (boost) => {
      boost.zapped = boost.zapped.filter((s) => s !== selector)
    })
  }

  delete(site: string): void {
    if (!this.all[site]) return
    if (this.zap?.site === site) this.stopZap()
    delete this.all[site]
    this.b.store.schedule()
    this.refresh(site)
    this.push(site)
  }

  // ───────────────────────── Edit window ─────────────────────────

  /** Opens the editor for the given tab's site (ignored outside http/https). */
  openForTab(tabId: string | null | undefined): void {
    if (tabId && this.b.privates.owns(tabId)) return
    const url = tabId ? this.b.store.state.tabs[tabId]?.url : ''
    if (url && isWeb(url)) this.open(siteOf(url))
  }

  /** Titles of the edit windows in the current language. */
  retitle(): void {
    for (const [site, editor] of this.editors) if (!editor.win.isDestroyed()) editor.win.setTitle(t('window.boost', { site }))
  }

  open(site: string): void {
    if (!site) return
    const existing = this.editors.get(site)
    if (existing && !existing.win.isDestroyed()) {
      if (existing.win.isMinimized()) existing.win.restore()
      existing.win.show()
      existing.win.focus()
      return
    }
    const win = new BrowserWindow({
      width: 400,
      height: 620,
      minWidth: 360,
      minHeight: 420,
      title: t('window.boost', { site }),
      ...windowChrome({ lights: { x: 14, y: 14 }, vibrancy: true }),
      webPreferences: { preload: preloadPath, sandbox: true, contextIsolation: true, nodeIntegration: false }
    })
    const editor: Editor = { win, ready: false, last: '' }
    this.editors.set(site, editor)
    win.webContents.on('did-finish-load', () => {
      editor.ready = true
      this.push(site, true)
      setTimeout(() => this.push(site, true), 150) // give React time to subscribe
    })
    win.on('closed', () => {
      this.editors.delete(site)
      if (this.zap?.site === site) this.stopZap()
    })
    denyWindowOpen(win.webContents)
    loadRoute(win.webContents, '#boost')
  }

  private stateOf(site: string): BoostEditorState {
    return {
      site,
      boost: this.all[site] ?? this.draft(site),
      zapping: this.zap?.site === site,
      hasTab: this.hasTab(site),
      hue: this.b.space.hue
    }
  }

  /** Sends the state to the site's editor (all if empty); with no change since the last send, does nothing. */
  push(site?: string, force = false): void {
    for (const [s, editor] of this.editors) {
      if (site && s !== site) continue
      const wc = editor.win.isDestroyed() ? null : editor.win.webContents
      if (!wc || wc.isDestroyed() || !editor.ready) continue
      const state = this.stateOf(s)
      const json = JSON.stringify(state)
      if (!force && json === editor.last) continue
      editor.last = json
      pushUi(wc, 'boost:state', state)
    }
  }

  // ───────────────────────── Outil « Masquer » ─────────────────────────

  async startZap(site: string): Promise<void> {
    if (!site) return
    if (this.zap) this.stopZap()
    const tabs = this.tabsOf(site)
    const active = this.b.space.activeTabId
    const tabId = (active && tabs.includes(active) ? active : null) ?? tabs[0]
    const wc = tabId ? this.b.views.viewOf(tabId)?.webContents : undefined
    if (!tabId || !wc || wc.isDestroyed()) return

    // Detached tabs (Peek, Little Signet) live in their own window: we do not touch the main one.
    if (!this.b.store.detached.has(tabId)) {
      this.b.window.show()
      this.b.tabs.activate(tabId)
    }
    const zap: Zap = { site, tabId, wc }
    this.zap = zap
    // A navigation destroys the isolated world and the promise never returns: we stop.
    const onNav = (details: { isMainFrame: boolean; isSameDocument: boolean }) => {
      if (details.isMainFrame && !details.isSameDocument) this.endZap(zap)
    }
    wc.on('did-start-navigation', onNav)
    this.push(site)
    setTimeout(() => !wc.isDestroyed() && wc.focus(), 80)

    try {
      while (this.zap === zap && !wc.isDestroyed()) {
        const result: unknown = await wc.executeJavaScriptInIsolatedWorld(WORLD, [{ code: ZAP_SCRIPT }], true)
        if (this.zap !== zap) break
        if (typeof result !== 'string' || !result) break
        this.edit(site, (boost) => {
          if (!boost.zapped.includes(result)) boost.zapped.push(result)
        })
      }
    } catch {
      // page closed or navigation during execution
    } finally {
      if (!wc.isDestroyed()) wc.removeListener('did-start-navigation', onNav)
      this.endZap(zap)
    }
  }

  stopZap(): void {
    const zap = this.zap
    if (!zap) return
    if (!zap.wc.isDestroyed()) {
      zap.wc.executeJavaScriptInIsolatedWorld(WORLD, [{ code: ZAP_STOP_SCRIPT }]).catch(() => {})
    }
    this.endZap(zap)
  }

  private endZap(zap: Zap | null = this.zap): void {
    if (!zap || this.zap !== zap) return
    this.zap = null
    this.push(zap.site)
  }
}

/** Command bar actions related to Boosts. */
export function boostActions(b: Browser): { id: string; title: string; subtitle?: string; shortcut?: string; run: () => void }[] {
  const tab = b.model.activeTab()
  if (!tab || !isWeb(tab.url)) return []
  const site = siteOf(tab.url)
  if (!site) return []
  return [
    {
      id: 'boost',
      title: t('palette.action.customizeSite', { site }),
      subtitle: b.store.state.boosts[site] ? t('palette.action.boostActive') : undefined,
      shortcut: '⌥⌘B',
      run: () => b.boosts.open(site)
    }
  ]
}

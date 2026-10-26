import { WebContentsView, type BrowserWindow, type WebContents } from 'electron'
import type { OverlayPayload, PaletteMode, PopoverRequest, Rect, SiteInfo } from '@shared/types'
import { pushUi } from '../ipc/push'
import { loadRoute, preloadPath } from '../core/urls'
import type { Store } from '../core/store'
import { activeSpace, activeTab } from '../model/locate'
import type { Passwords } from '../features/passwords/passwords'
import type { Compact } from './compact'
import type { Floating } from './floating'
import type { Changed } from './broadcast'
import type { Layout, LayoutContext, OverlayLayer } from './layout'

/** Duration of the overlay surfaces' fade-out (= --t-fast on the CSS side and in Overlay.tsx). */
export const OVERLAY_EXIT_MS = 120
/** Unsolicited password prompt: the overlay only covers the card (300 px) + a margin for the shadow, the page stays clickable. */
const PW_MARGIN = 24
const PW_W = 300 + PW_MARGIN * 2
const PW_H = 200
/** After this time with no answer, the unsolicited prompt tucks itself away (it stays available from the key button). */
const PW_PROMPT_MS = 30000
/** Wait for the address field's answer before opening the centered bar. */
const EDIT_ADDRESS_MS = 250
/** Fallback position of a popover: under the space switcher, just after the traffic lights. */
const DEFAULT_ANCHOR: Rect = { x: 78, y: 6, width: 140, height: 28 }

export type OverlayState =
  | { kind: 'palette'; mode: PaletteMode }
  | { kind: 'peek' }
  | { kind: 'dropzones' }
  | { kind: 'site' }
  | { kind: 'popover' }
  | { kind: 'password' }
  | { kind: 'capture' }
  | { kind: 'webapp' }
  | null

export type OverlayKind = NonNullable<OverlayState>['kind']

/** The Peek as seen by the overlay: opening something else closes it first. */
export interface PeekHandle {
  readonly tabId: string | null
  close(animate?: boolean): void
  /** Forgets the last card sent (the overlay closes). */
  resetCard(): void
}

export interface OverlayDeps {
  store: Store
  window: { readonly win: BrowserWindow | null }
  layout: Pick<Layout, 'panes' | 'requestFocus' | 'dropArea'>
  floating: Pick<Floating, 'hide'>
  compact: Pick<Compact, 'hover'>
  passwords: Pick<Passwords, 'dismissPrompt'>
  /** Immediate full cycle (layout, state, save requested). */
  apply: () => void
  changed: Changed
}

/** Password prompt without focus: tab whose card bounds the overlay, and last rectangle set (kept during the exit). */
interface CompactBounds {
  tabId: string
  rect: Rect
}

/** Overlay rectangle for the password prompt: top-right corner of the card, shadow margin included. */
function passwordBounds(card: Rect, W: number): Rect {
  const x = Math.max(0, Math.min(card.x + card.width - 300 - 12 - PW_MARGIN, W - PW_W))
  return { x, y: Math.max(0, card.y + 12 - PW_MARGIN), width: PW_W, height: PW_H }
}

/**
 * Full-window UI view placed above the pages: command bar, panels, popovers, drop zones, Peek.
 * It loads its bundle once the window is ready; early sends wait for its `overlay:ready` signal.
 */
export class Overlay implements OverlayLayer {
  private view: WebContentsView | null = null
  private isReady = false
  private state: OverlayState = null
  private queued: OverlayPayload | null = null
  private loading = false
  /** Animated exit in progress: the view stays in place while its content fades out. */
  leaving = false
  private leaveTimer: NodeJS.Timeout | null = null
  private compactBounds: CompactBounds | null = null
  private passwordTimer: NodeJS.Timeout | null = null
  private editAddressTimer: NodeJS.Timeout | null = null
  private peek!: PeekHandle

  constructor(private readonly d: OverlayDeps) {}

  /** The Peek is built after the overlay, which it uses. */
  attach(peek: PeekHandle): void {
    this.peek = peek
  }

  // ───────────────────────── State ─────────────────────────

  get kind(): OverlayKind | undefined {
    return this.state?.kind
  }

  /** True when the overlay (palette, Peek, panel) is open. */
  get open(): boolean {
    return this.state !== null
  }

  /** True when a popover of the tab row is open. */
  get popoverOpen(): boolean {
    return this.state?.kind === 'popover'
  }

  /** Its renderer is subscribed to `overlay:show`. */
  get ready(): boolean {
    return this.isReady
  }

  get contents(): WebContents | null {
    return this.view?.webContents ?? null
  }

  /** The overlay state changes (the Peek settles into it). */
  setState(state: OverlayState): void {
    this.state = state
  }

  // ───────────────────────── View lifecycle ─────────────────────────

  /** Creates the overlay view for a new main window. */
  create(): WebContentsView {
    const overlay = new WebContentsView({
      webPreferences: { preload: preloadPath, sandbox: true, contextIsolation: true, nodeIntegration: false }
    })
    overlay.setBackgroundColor('#00000000')
    // Ready only when React listens to `overlay:show` (`overlay:ready` signal), not at did-finish-load: lazy chunk + getLocale.
    // Any new load (HMR, reload) or crash redoes the handshake.
    overlay.webContents.on('did-start-loading', () => (this.isReady = false))
    overlay.webContents.on('render-process-gone', () => (this.isReady = false))
    this.view = overlay
    this.isReady = false
    this.loading = false
    return overlay
  }

  /** Overlay signal: its renderer is subscribed. Only the current overlay's webContents is accepted. */
  hello(sender: WebContents): void {
    if (!this.view || sender !== this.view.webContents) return
    this.isReady = true
    if (this.queued) this.send(this.queued)
  }

  ensureLoaded(): void {
    const wc = this.view?.webContents
    if (this.loading || !wc || wc.isDestroyed()) return
    this.loading = true
    loadRoute(wc, '#overlay')
  }

  /** Main window closed: no pending state or send anymore. */
  reset(): void {
    if (this.passwordTimer) clearTimeout(this.passwordTimer)
    this.passwordTimer = null
    this.state = null
    this.queued = null
  }

  /** Main window closed: the overlay view goes away. */
  destroyView(): void {
    this.view?.webContents.close()
    this.view = null
    this.loading = false
  }

  /** Places the overlay view (drop zone, password card or full window). */
  place({ W, H, panes, desired }: LayoutContext): void {
    if (!(this.state || this.leaving) || !this.view) return
    // Drop zones: the overlay only covers the web card, the sidebar stays the drag source.
    // Unsolicited password prompt: only the card, so the page keeps clicks and focus.
    const compact = this.compactBounds
    const pane = compact ? panes.find((p) => p.tabId === compact.tabId) : undefined
    if (compact && pane) compact.rect = passwordBounds(pane.card, W)
    this.view.setBounds(this.state?.kind === 'dropzones' ? this.d.layout.dropArea() : (compact?.rect ?? { x: 0, y: 0, width: W, height: H }))
    desired.push(this.view)
  }

  // ───────────────────────── Sending, exit ─────────────────────────

  send(payload: OverlayPayload): void {
    const wc = this.view?.webContents
    if (!wc || wc.isDestroyed()) return
    // An opening during the exit cancels it: the view is already placed, the renderer restarts from scratch
    // (or resumes the exit in progress if the payload is identical, see Overlay.tsx).
    this.cancelExit()
    this.compactBounds = null
    if (!this.isReady) {
      this.queued = payload
      this.ensureLoaded()
      return
    }
    this.queued = null
    pushUi(wc, 'overlay:show', payload)
  }

  private cancelExit(): void {
    if (this.leaveTimer) clearTimeout(this.leaveTimer)
    this.leaveTimer = null
    this.leaving = false
  }

  hide(instant = false): void {
    const wasPopover = this.popoverOpen
    // Palette, site panel and popovers fade out (~--t-fast) before the view is removed; Peek and drop zones go right away.
    // `instant` (click outside the panel): the user is already acting elsewhere, the full-window view must not swallow their next click.
    const kind = this.state?.kind
    const animate = !instant && (kind === 'palette' || kind === 'site' || kind === 'popover' || kind === 'password' || kind === 'webapp' || kind === 'peek')
    this.cancelExit()
    if (animate) {
      this.leaving = true
      this.leaveTimer = setTimeout(() => {
        this.leaveTimer = null
        this.leaving = false
        this.compactBounds = null
        this.d.changed({ persist: false })
      }, OVERLAY_EXIT_MS)
    }
    this.state = null
    if (!animate) this.compactBounds = null
    if (this.passwordTimer) clearTimeout(this.passwordTimer)
    this.passwordTimer = null
    this.queued = null
    this.peek.resetCard()
    const wc = this.view?.webContents
    if (wc) pushUi(wc, 'overlay:hide', !animate)
    // Compact mode's floating bar: without hover, it closes now that its popover is gone.
    if (wasPopover) this.d.compact.hover(false)
    this.d.layout.requestFocus()
    this.d.apply()
  }

  /**
   * Closes the overlay. `instant`: click outside the panel (or click during the exit), no exit animation,
   * so the full-window view leaves right away and no longer captures clicks meant for the page.
   */
  close(instant = false): void {
    if (this.peek.tabId) this.peek.close(!instant)
    else if (this.state) this.hide(instant)
    else if (instant && this.leaving) {
      this.cancelExit()
      const wc = this.view?.webContents
      if (wc) pushUi(wc, 'overlay:hide', true)
      this.d.apply()
    }
  }

  // ───────────────────────── Openings ─────────────────────────

  /** ⌘L: asks the renderer for the visible address field, from which the bar grows; without a quick answer, it opens centered. */
  editAddress(): void {
    const wc = this.d.window.win?.webContents
    if (!wc || wc.isDestroyed() || this.d.store.state.settings.compactMode) return this.openPalette('edit-url')
    if (this.editAddressTimer) clearTimeout(this.editAddressTimer)
    this.editAddressTimer = setTimeout(() => {
      this.editAddressTimer = null
      this.openPalette('edit-url')
    }, EDIT_ADDRESS_MS)
    pushUi(wc, 'palette:edit-url-request')
  }

  /** `anchor`: address field from which the bar grows in `edit-url` mode (window coordinates). */
  openPalette(mode: PaletteMode, anchor?: Rect): void {
    if (this.editAddressTimer) clearTimeout(this.editAddressTimer)
    this.editAddressTimer = null
    if (!this.d.window.win || !this.view) return
    if (this.peek.tabId) this.peek.close()
    this.d.floating.hide()
    this.state = { kind: 'palette', mode }
    const { state } = this.d.store
    const ok = mode === 'edit-url' && anchor && [anchor.x, anchor.y, anchor.width, anchor.height].every(Number.isFinite)
    this.send({
      mode: 'palette',
      paletteMode: mode,
      initial: mode === 'edit-url' ? (activeTab(state)?.url ?? '') : '',
      hue: activeSpace(state).hue,
      ...(ok && { anchor })
    })
    this.d.layout.requestFocus()
    this.d.apply()
  }

  /** Site panel (no scrim) anchored under the toolbar's padlock. */
  openSite(info: SiteInfo, anchor: Rect): void {
    if (!this.d.window.win || !this.view) return
    if (this.peek.tabId) this.peek.close()
    this.d.floating.hide()
    this.state = { kind: 'site' }
    this.send({ mode: 'site', info, anchor })
    this.d.layout.requestFocus()
    this.d.apply()
  }

  /** Prompt to save a password, top-right of the tab's card (overlay without scrim). */
  openPassword(tabId: string, payload: Omit<Extract<OverlayPayload, { mode: 'password' }>, 'mode' | 'card'>): boolean {
    const win = this.d.window.win
    if (!win || !this.view || this.state) return false
    const pane = this.d.layout.panes.find((p) => p.tabId === tabId)
    if (!pane) return false
    this.d.floating.hide()
    this.state = { kind: 'password' }
    this.send({ mode: 'password', ...payload, card: pane.card })
    if (payload.anchor) this.d.layout.requestFocus()
    else {
      // Unsolicited prompt (after a login): no stolen focus, no blocked page. The overlay only covers the card,
      // and it tucks itself away after a while; the address bar's key button reopens it by keyboard.
      this.compactBounds = { tabId, rect: passwordBounds(pane.card, win.getContentBounds().width) }
      if (this.passwordTimer) clearTimeout(this.passwordTimer)
      this.passwordTimer = setTimeout(() => {
        this.passwordTimer = null
        if (this.state?.kind === 'password' && this.compactBounds) this.d.passwords.dismissPrompt()
      }, PW_PROMPT_MS)
    }
    this.d.apply()
    return true
  }

  /** Saved accounts for the site, under the address bar's key button (overlay without scrim). */
  openPasswordList(tabId: string, payload: Omit<Extract<OverlayPayload, { mode: 'passwords' }>, 'mode'>): boolean {
    if (!this.d.window.win || !this.view || this.state) return false
    if (!this.d.layout.panes.some((p) => p.tabId === tabId)) return false
    this.d.floating.hide()
    this.state = { kind: 'password' }
    this.send({ mode: 'passwords', ...payload })
    this.d.layout.requestFocus()
    this.d.apply()
    return true
  }

  /** Prompt to install the site as an app, top-right of the tab's card (overlay without scrim). */
  openWebApp(tabId: string, payload: Omit<Extract<OverlayPayload, { mode: 'webapp' }>, 'mode' | 'card'>): boolean {
    if (!this.d.window.win || !this.view) return false
    const pane = this.d.layout.panes.find((p) => p.tabId === tabId)
    if (!pane) return false
    if (this.peek.tabId) this.peek.close()
    this.d.floating.hide()
    this.state = { kind: 'webapp' }
    this.send({ mode: 'webapp', ...payload, card: pane.card })
    this.d.layout.requestFocus()
    this.d.apply()
    return true
  }

  /** Screenshot tool on the displayed tab's web view; false if the tab is not in the main window. */
  openCapture(tabId: string): boolean {
    if (!this.d.window.win || !this.view) return false
    if (this.peek.tabId) this.peek.close()
    const pane = this.d.layout.panes.find((p) => p.tabId === tabId)
    if (!pane) return false
    this.d.floating.hide()
    this.state = { kind: 'capture' }
    this.send({ mode: 'capture', tabId, rect: pane.rect, hue: activeSpace(this.d.store.state).hue })
    this.d.layout.requestFocus()
    this.d.apply()
    return true
  }

  /** Menu or small form above the pages (top tab row, bookmarks bar), anchored under `anchor`. */
  openPopover(popover: PopoverRequest, anchor: Rect = DEFAULT_ANCHOR): void {
    if (!this.d.window.win || !this.view) return
    if (this.peek.tabId) this.peek.close()
    this.d.floating.hide()
    this.state = { kind: 'popover' }
    this.send({ mode: 'popover', popover, anchor, hue: activeSpace(this.d.store.state).hue })
    this.d.layout.requestFocus()
    this.d.apply()
  }
}

import type { BrowserWindow } from 'electron'
import type { UiEventMap } from '@shared/ipc/contract'
import { pushUi } from '../ipc/push'
import type { Store } from '../core/store'
import { isHorizontal, isSidebarOn } from '../model/chrome'
import { floatExitMs } from './compact'
import { UiView } from './uiview'
import type { Changed } from './broadcast'

/** Warm-up of the floating sidebar once the sidebar is hidden. */
const SIDEBAR_WARM_MS = 1500
const FLOATING_HIDE_DELAY = 250
/** Delay left to the just-revealed bar to display itself before talking to it. */
export const REVEAL_MS = 80

export interface FloatingDeps {
  store: Store
  window: { readonly win: BrowserWindow | null }
  changed: Changed
  /** True when the overlay (palette, Peek, panel) is open: the bar is not revealed underneath. */
  overlayOpen: () => boolean
}

/** Floating sidebar (revealed when hovering the left edge while the fixed bar is hidden): its view, its delays, its animated exits. */
export class Floating {
  /** The floating bar is revealed. */
  shown = false
  /** Animated exit in progress: the view stays in place while its content slides. */
  leaving = false
  sidebar: UiView | null = null
  private warming = false
  private warmTimers: NodeJS.Timeout[] = []
  private hideTimer: NodeJS.Timeout | null = null
  private leaveTimer: NodeJS.Timeout | null = null

  constructor(private readonly d: FloatingDeps) {}

  /** The visible floating view, when the fixed bar is hidden. */
  get target(): UiView | null {
    return this.shown && !isSidebarOn(this.d.store.state) ? this.sidebar : null
  }

  ensureSidebar(): void {
    this.sidebar ??= new UiView('#sidebar', true)
  }

  /** Signal from the floating view: its page is subscribed. False if the signal does not come from it. */
  markReady(sender: Electron.WebContents): boolean {
    if (!this.sidebar || sender !== this.sidebar.webContents) return false
    this.sidebar.markReady()
    return true
  }

  /** Once the sidebar is hidden, prepares its floating view so it is ready on edge hover. */
  warm(): void {
    const { state } = this.d.store
    if (this.sidebar || isSidebarOn(state) || isHorizontal(state) || state.settings.compactMode || this.warming) return
    this.warming = true
    const timer = setTimeout(() => {
      this.warmTimers = this.warmTimers.filter((t) => t !== timer)
      this.warming = false
      if (this.d.window.win) this.ensureSidebar()
    }, SIDEBAR_WARM_MS)
    this.warmTimers.push(timer)
  }

  /** Floating sidebar: the mouse touches the left edge (true) or left the bar (false, with a short delay). */
  setHover(hover: boolean): void {
    if (this.hideTimer) clearTimeout(this.hideTimer)
    this.hideTimer = null
    const { state } = this.d.store
    if (hover) {
      if (isSidebarOn(state) || isHorizontal(state) || this.d.overlayOpen() || this.shown) return
      // Pointer returns during the exit: the bar restarts from its current position.
      this.cancelLeave()
      this.ensureSidebar()
      this.shown = true
      this.d.changed({ persist: false })
    } else if (this.shown) {
      this.hideTimer = setTimeout(() => {
        this.hideTimer = null
        // Animated exit: rendering goes back to "hidden", the view is only removed at the end of the slide.
        this.shown = false
        this.leaving = true
        this.d.changed({ persist: false })
        this.leaveTimer = setTimeout(() => {
          this.leaveTimer = null
          this.leaving = false
          this.d.changed({ persist: false })
        }, floatExitMs())
      }, FLOATING_HIDE_DELAY)
    }
  }

  /** Hides without animation and without requesting a layout (the caller handles it). */
  hide(): void {
    if (this.hideTimer) clearTimeout(this.hideTimer)
    this.hideTimer = null
    this.cancelLeave()
    this.shown = false
  }

  private cancelLeave(): void {
    if (this.leaveTimer) clearTimeout(this.leaveTimer)
    this.leaveTimer = null
    this.leaving = false
  }

  /** Sends a message to the visible sidebar (main, or floating when it is hidden). */
  send<C extends keyof UiEventMap>(channel: C, ...args: UiEventMap[C]): void {
    const { store, window } = this.d
    const state = store.state
    if (isHorizontal(state)) {
      if (window.win) pushUi(window.win.webContents, channel, ...args)
      return
    }
    if (!isSidebarOn(state) && !this.shown) {
      // Bar hidden or compact: we reveal it, then talk to it once rendered.
      const compact = state.settings.compactMode
      if (compact) {
        this.cancelLeave()
        this.ensureSidebar()
        this.shown = true
        this.d.changed({ persist: false })
      } else {
        state.sidebarVisible = true
        this.d.changed()
      }
      setTimeout(() => {
        const win = window.win
        if (!win || win.isDestroyed()) return
        if (compact) this.sidebar?.send(channel, ...args)
        else pushUi(win.webContents, channel, ...args)
      }, REVEAL_MS)
      return
    }
    const target = this.target
    if (target) target.send(channel, ...args)
    else if (window.win) pushUi(window.win.webContents, channel, ...args)
  }

  /** Main window closed: warm-up timers, floating view and revealed state. */
  reset(): void {
    for (const t of this.warmTimers) clearTimeout(t)
    this.warmTimers = []
    this.warming = false
    this.sidebar?.destroy()
    this.sidebar = null
    this.hide()
  }
}

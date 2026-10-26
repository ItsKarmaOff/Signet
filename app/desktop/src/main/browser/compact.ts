import { systemPreferences } from 'electron'
import type { Rect } from '@shared/types'
import type { UiEventMap } from '@shared/ipc/contract'
import type { Browser } from './browser'
import { UiView } from './uiview'

const HIDE_DELAY = 250
/** Height of the floating toolbar, and margin around the web card in compact mode. */
const FLOAT_HEIGHT = 40
export const COMPACT_MARGIN = 6

/** Duration of the animated exit (slide) of a floating surface, before removing its view: --t, or --t-fast with reduced motion. */
export function floatExitMs(): number {
  try {
    return systemPreferences.getAnimationSettings().prefersReducedMotion ? 120 : 200
  } catch {
    return 200
  }
}

/** Compact mode's floating toolbar (`#toolbar` view), revealed when hovering the top edge. */
export class Compact {
  floating = false
  /** Animated exit in progress: the view stays in place until the slide ends (the pointer coming back cancels it). */
  leaving = false
  private leaveTimer: NodeJS.Timeout | null = null
  private ui: UiView | null = null
  private timer: NodeJS.Timeout | null = null

  constructor(private readonly b: Browser) {}

  get view() {
    return this.ui?.view
  }

  create(): void {
    this.ui ??= new UiView('#toolbar')
  }

  destroy(): void {
    this.hide()
    this.ui?.destroy()
    this.ui = null
  }

  send<C extends keyof UiEventMap>(channel: C, ...args: UiEventMap[C]): void {
    this.ui?.send(channel, ...args)
  }

  /** Position of the bar: inside the top edge of the web card. */
  bounds(card: Rect): Rect {
    return { x: card.x + 8, y: COMPACT_MARGIN, width: Math.max(200, card.width - 16), height: FLOAT_HEIGHT }
  }

  /** "Tabs on top" layout: the floating bar is the tab row, across the full width. */
  stripBounds(width: number): Rect {
    return { x: COMPACT_MARGIN, y: COMPACT_MARGIN, width: Math.max(200, width - COMPACT_MARGIN * 2), height: FLOAT_HEIGHT }
  }

  /** The mouse touches the top edge (true) or left the bar (false, after a short cancelable delay). */
  hover(hover: boolean): void {
    if (this.timer) clearTimeout(this.timer)
    this.timer = null
    // A popover of the row covers the bar: the mouse leaving does not close it.
    if (!hover && this.b.overlay.popoverOpen) return
    if (hover) {
      if (!this.b.store.state.settings.compactMode || this.b.overlay.open || this.floating) return
      this.cancelLeave()
      this.floating = true
      this.b.broadcast.changed({ persist: false })
    } else if (this.floating) {
      this.timer = setTimeout(() => {
        this.timer = null
        this.leave()
      }, HIDE_DELAY)
    }
  }

  /** Hides without animation and without requesting a layout (the caller handles it). */
  hide(): void {
    if (this.timer) clearTimeout(this.timer)
    this.timer = null
    this.cancelLeave()
    this.floating = false
  }

  /** Animated exit: rendering goes back to "hidden" (the bar slides up), the view is only removed afterwards. */
  private leave(): void {
    this.floating = false
    this.leaving = true
    this.b.broadcast.changed({ persist: false })
    this.leaveTimer = setTimeout(() => {
      this.leaveTimer = null
      this.leaving = false
      this.b.broadcast.changed({ persist: false })
    }, floatExitMs())
  }

  private cancelLeave(): void {
    if (this.leaveTimer) clearTimeout(this.leaveTimer)
    this.leaveTimer = null
    this.leaving = false
  }
}

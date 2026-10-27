import { systemPreferences } from 'electron'
import { IS_MAC } from '../app/platform'
import { emulateLanguages } from '../web/languages'
import type { Adblock } from '../web/adblock'
import type { Store } from '../core/store'
import { SIDEBAR_MAX, SIDEBAR_MIN, SIDEBAR_OVERSHOOT } from '../core/store'
import type { Develop } from '../devtools/develop'
import { isHorizontal } from '../model/chrome'
import { clamp } from '../model/types'
import type { Changed } from './broadcast'
import type { Compact } from './compact'
import type { Floating } from './floating'
import type { Layout } from './layout'
import type { Overlay } from './overlay'
import type { Views } from './views'
import { slideState, type SlideMotion } from '@shared/slide'

export interface ShellDeps {
  store: Store
  changed: Changed
  floating: Pick<Floating, 'hide'>
  compact: Pick<Compact, 'hide'>
  overlay: Pick<Overlay, 'kind' | 'hide'>
  layout: Pick<Layout, 'panes' | 'stepSlide'> & { liveSidebarWidth: number | null; sidebarProgress: number | null; slideMotion: SlideMotion | null }
  views: Pick<Views, 'all' | 'viewOf'>
  develop: Pick<Develop, 'reapplyAll'>
  adblock: Pick<Adblock, 'sync'>
}

/** Steps of the pages during the slide (ms): finer than the screen, the spring is read on the clock. */
const SLIDE_STEP_MS = 8
/**
 * A slide from rest starts this much later (ms): the main UI must first receive and draw the new
 * state; without it the pages would leave alone and the bar catch up.
 */
const SLIDE_START_DELAY_MS = 60
/** Closing on a Mac also hides the traffic lights, which relays the window out first: a longer wait. */
const SLIDE_CLOSE_DELAY_MS = 140

const reducedMotion = (): boolean => {
  try {
    return systemPreferences.getAnimationSettings().prefersReducedMotion
  } catch {
    return false
  }
}

/** Window look: sidebar (shown, width), compact mode, tab row on top, and a few settings toggles. */
export class Shell {
  private slideTimer: NodeJS.Timeout | null = null

  constructor(private readonly d: ShellDeps) {}

  /** ⌘S has no purpose when the tabs are on top. */
  toggleSidebar(): void {
    const { store } = this.d
    if (isHorizontal(store.state)) return
    this.d.floating.hide()
    this.d.compact.hide()
    const { state } = store
    if (state.settings.compactMode) {
      // ⌘S in compact mode: we leave the mode and the sidebar is shown.
      state.settings.compactMode = false
      state.sidebarVisible = true
      this.stopSlide()
    } else {
      state.sidebarVisible = !state.sidebarVisible
      this.startSlide(state.sidebarVisible)
    }
    this.d.changed()
  }

  /**
   * Slides the sidebar toward shown or hidden, from where it is now with its speed: a toggle
   * mid-slide turns back smoothly. Main moves the pages; the main UI reads the same spring.
   */
  private startSlide(show: boolean): void {
    const { layout } = this.d
    if (reducedMotion()) return this.stopSlide()
    const now = Date.now()
    const moving = layout.slideMotion
    const current = moving ? slideState(moving, now) : { progress: show ? 0 : 1, velocity: 0 }
    // Turning back mid-slide goes on at once (a pause would show); from rest, both sides start together.
    const start = moving ? now : now + (!show && IS_MAC ? SLIDE_CLOSE_DELAY_MS : SLIDE_START_DELAY_MS)
    layout.slideMotion = { start, from: current.progress, velocity: current.velocity, target: show ? 1 : 0 }
    layout.sidebarProgress = current.progress
    this.slideTimer ??= setInterval(() => {
      if (layout.stepSlide(Date.now())) return
      // Settled: final layout (pages' size, traffic lights).
      this.stopSlide()
      this.d.changed({ persist: false })
    }, SLIDE_STEP_MS)
  }

  /** Ends the slide at once (the layout follows on the caller's next change). */
  private stopSlide(): void {
    if (this.slideTimer) clearInterval(this.slideTimer)
    this.slideTimer = null
    this.d.layout.slideMotion = null
    this.d.layout.sidebarProgress = null
  }

  /** Switches between the sidebar and the tab row on top (⌃⇧S); no page is reloaded. */
  setTabLayout(layout: 'vertical' | 'horizontal'): void {
    const { settings } = this.d.store.state
    if ((settings.tabLayout ?? 'vertical') === layout) return
    if (this.d.overlay.kind === 'popover') this.d.overlay.hide()
    settings.tabLayout = layout
    this.stopSlide()
    this.d.floating.hide()
    this.d.compact.hide()
    this.d.changed()
  }

  toggleTabLayout(): void {
    this.setTabLayout(isHorizontal(this.d.store.state) ? 'vertical' : 'horizontal')
  }

  /** Compact mode (sidebar and toolbar revealed on hovering the edges). */
  setCompact(on: boolean): void {
    const { settings } = this.d.store.state
    if (settings.compactMode === on) return
    settings.compactMode = on
    this.stopSlide()
    this.d.floating.hide()
    this.d.compact.hide()
    this.d.changed()
  }

  toggleCompact(): void {
    this.setCompact(!this.d.store.state.settings.compactMode)
  }

  setSidebarWidth(width: number, live = false): void {
    const { store, layout } = this.d
    const rounded = Math.round(width)
    if (live) {
      const next = clamp(rounded, SIDEBAR_MIN - SIDEBAR_OVERSHOOT, SIDEBAR_MAX + SIDEBAR_OVERSHOOT)
      if (next === layout.liveSidebarWidth) return
      layout.liveSidebarWidth = next
      this.d.changed({ persist: false })
      return
    }
    const next = clamp(rounded, SIDEBAR_MIN, SIDEBAR_MAX)
    const wasLive = layout.liveSidebarWidth !== null
    layout.liveSidebarWidth = null
    if (next === store.state.sidebarWidth && !wasLive) return
    store.state.sidebarWidth = next
    this.d.changed() // layout + state; the write is already deferred by store.schedule
  }

  togglePeekLinks(): void {
    const { settings } = this.d.store.state
    settings.peekExternalLinks = !settings.peekExternalLinks
    this.d.changed()
  }

  toggleAdblock(): void {
    const { settings } = this.d.store.state
    settings.adblock = !settings.adblock
    void this.d.adblock.sync()
    this.d.changed()
  }

  /** Reapplies the languages to the views and reloads the displayed tabs (after a language change). */
  reloadVisible(): void {
    const { languages } = this.d.store.state.settings
    for (const v of this.d.views.all()) emulateLanguages(v.webContents, languages)
    this.d.develop.reapplyAll() // the simulated user agent goes through the same command as the languages
    for (const p of this.d.layout.panes) this.d.views.viewOf(p.tabId)?.webContents.reload()
  }
}

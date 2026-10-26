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

export interface ShellDeps {
  store: Store
  changed: Changed
  floating: Pick<Floating, 'hide'>
  compact: Pick<Compact, 'hide'>
  overlay: Pick<Overlay, 'kind' | 'hide'>
  layout: Pick<Layout, 'panes'> & { liveSidebarWidth: number | null }
  views: Pick<Views, 'all' | 'viewOf'>
  develop: Pick<Develop, 'reapplyAll'>
  adblock: Pick<Adblock, 'sync'>
}

/** Window look: sidebar (shown, width), compact mode, tab row on top, and a few settings toggles. */
export class Shell {
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
    } else {
      state.sidebarVisible = !state.sidebarVisible
    }
    this.d.changed()
  }

  /** Switches between the sidebar and the tab row on top (⌃⇧S); no page is reloaded. */
  setTabLayout(layout: 'vertical' | 'horizontal'): void {
    const { settings } = this.d.store.state
    if ((settings.tabLayout ?? 'vertical') === layout) return
    if (this.d.overlay.kind === 'popover') this.d.overlay.hide()
    settings.tabLayout = layout
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

import { BrowserWindow, type WebContents } from 'electron'
import type { MediaInfo } from '@shared/types'
import type { Browser } from './browser'
import { siteOf } from '../core/urls'

/** A paused tab stays a candidate for 10 minutes. */
const PAUSED_TTL = 10 * 60_000
/** Re-reading the metadata of media playing in a hidden tab (track change). */
const POLL_INTERVAL = 4000
/** Isolated world reserved for reading the metadata. */
const WORLD = 1002
/** The media only affects the state sent to the UI: no layout or save. */
const STATE_ONLY = { layout: false, persist: false }

interface Entry {
  playing: boolean
  /** Last start or last pause (ms). */
  activeAt: number
  title?: string
  artist?: string
  artwork?: string
  hasMeta: boolean
  /** True if the tab actually emitted sound (silent decorative videos do not count). */
  heard: boolean
}

interface Meta {
  title?: string
  artist?: string
  artwork?: string
  hasMeta: boolean
}

const READ_META = `(() => {
  const m = navigator.mediaSession && navigator.mediaSession.metadata
  if (!m) return { hasMeta: false }
  const art = m.artwork && m.artwork.length ? m.artwork[m.artwork.length - 1].src : undefined
  return { hasMeta: true, title: m.title || undefined, artist: m.artist || undefined, artwork: art }
})()`

const TOGGLE = `(() => {
  const mark = Symbol.for('signet.media')
  const els = [...document.querySelectorAll('video,audio')]
  const playing = els.filter((e) => !e.paused && !e.ended)
  if (playing.length) {
    for (const e of playing) { e[mark] = true; e.pause() }
    return
  }
  const el = els.find((e) => e[mark]) || els.find((e) => e.currentTime > 0 && !e.ended) || els[0]
  if (el) { el[mark] = false; el.play().catch(() => {}) }
})()`

/** Per-tab media tracking: sidebar card, play / pause, automatic picture-in-picture. */
export class Media {
  private entries = new Map<string, Entry>()
  private displayed: string[] = []
  private timer: NodeJS.Timeout | null = null
  private expiry = new Map<string, NodeJS.Timeout>()
  private awayTimer: NodeJS.Timeout | null = null
  /** Fake media, set only by the capture hook (dev/capture.ts). */
  debug: MediaInfo | null = null

  constructor(private readonly b: Browser) {}

  /** Hooks the tracking onto a tab's view. */
  attach(tabId: string, wc: WebContents): void {
    const touch = (playing: boolean) => {
      const e = this.entries.get(tabId) ?? { playing: false, activeAt: 0, hasMeta: false, heard: false }
      e.playing = playing
      e.activeAt = Date.now()
      this.entries.set(tabId, e)
      if (!e.heard) return // silent decorative video: never a candidate, nothing to re-read or notify
      this.scheduleExpiry(tabId, playing)
      this.ensureTimer()
      void this.refresh(tabId, wc)
      this.b.broadcast.changed(STATE_ONLY)
    }
    wc.on('media-started-playing', () => touch(true))
    wc.on('media-paused', () => touch(false))
    wc.on('audio-state-changed', (ev) => {
      if (ev.audible) {
        // Without a media element (Web Audio) no play / pause event will come: not "playing", it expires like a pause.
        const e = this.entries.get(tabId) ?? { playing: false, activeAt: Date.now(), hasMeta: false, heard: false }
        if (!e.heard) {
          e.heard = true
          this.entries.set(tabId, e)
          this.scheduleExpiry(tabId, e.playing)
          this.ensureTimer()
          this.b.broadcast.changed(STATE_ONLY)
        }
      }
      if (this.entries.has(tabId)) void this.refresh(tabId, wc)
    })
    wc.once('destroyed', () => this.drop(tabId))
  }

  private drop(tabId: string): void {
    this.entries.delete(tabId)
    this.displayed = this.displayed.filter((id) => id !== tabId)
    const t = this.expiry.get(tabId)
    if (t) clearTimeout(t)
    this.expiry.delete(tabId)
    this.b.broadcast.changed(STATE_ONLY)
  }

  private scheduleExpiry(tabId: string, playing: boolean): void {
    const old = this.expiry.get(tabId)
    if (old) clearTimeout(old)
    this.expiry.delete(tabId)
    if (playing) return
    this.expiry.set(
      tabId,
      setTimeout(() => this.b.broadcast.changed(STATE_ONLY), PAUSED_TTL + 500)
    )
  }

  private ensureTimer(): void {
    if (this.timer) return
    this.timer = setInterval(() => {
      // No candidate left: we stop the interval (re-armed when a tab is heard again).
      if (![...this.entries.values()].some((e) => this.candidate(e))) return this.stopTimer()
      for (const [tabId, e] of this.entries) {
        if (!this.candidate(e) || this.displayed.includes(tabId)) continue
        const wc = this.b.views.viewOf(tabId)?.webContents
        if (wc && !wc.isDestroyed()) void this.refresh(tabId, wc)
      }
    }, POLL_INTERVAL)
    this.timer.unref()
  }

  private stopTimer(): void {
    if (this.timer) clearInterval(this.timer)
    this.timer = null
  }

  /** Re-reads the MediaSession metadata and notifies if it changed. */
  private async refresh(tabId: string, wc: WebContents): Promise<void> {
    try {
      const meta = (await wc.executeJavaScriptInIsolatedWorld(WORLD, [{ code: READ_META }])) as Meta | undefined
      const e = this.entries.get(tabId)
      if (!e || !meta) return
      if (e.hasMeta !== meta.hasMeta || e.title !== meta.title || e.artist !== meta.artist || e.artwork !== meta.artwork) {
        Object.assign(e, meta)
        this.b.broadcast.changed(STATE_ONLY)
      }
    } catch {
      /* page closed or navigating */
    }
  }

  /** True if the tab is playing, or played sound recently: not to be unloaded. */
  isActive(tabId: string): boolean {
    const e = this.entries.get(tabId)
    return !!e && (e.playing || this.candidate(e))
  }

  private candidate(e: Entry): boolean {
    return e.heard && (e.playing || Date.now() - e.activeAt < PAUSED_TTL)
  }

  /** Most recent candidate among the tabs that are not displayed. */
  current(shown: string[]): MediaInfo | null {
    if (this.debug) return this.debug
    let best: [string, Entry] | null = null
    for (const [tabId, e] of this.entries) {
      const tab = this.b.store.state.tabs[tabId]
      if (!tab || this.b.store.detached.has(tabId) || shown.includes(tabId) || !this.candidate(e)) continue
      if (!best || e.activeAt > best[1].activeAt) best = [tabId, e]
    }
    if (!best) return null
    const [tabId, e] = best
    const tab = this.b.store.state.tabs[tabId]
    return {
      tabId,
      title: e.title || tab.title || tab.url,
      artist: e.artist,
      artwork: e.artwork,
      site: siteOf(tab.url) || tab.url,
      playing: e.playing,
      muted: tab.muted,
      canSkip: e.hasMeta
    }
  }

  /** Play / pause (with a user gesture, otherwise the page may refuse to resume). */
  toggle(tabId: string): void {
    const wc = this.b.views.viewOf(tabId)?.webContents
    if (!wc || wc.isDestroyed()) return
    wc.executeJavaScript(TOGGLE, true).catch(() => {})
  }

  /** Next / previous track: media keys sent to the page, best effort. */
  skip(tabId: string, direction: 'next' | 'previous'): void {
    const wc = this.b.views.viewOf(tabId)?.webContents
    if (!wc || wc.isDestroyed()) return
    const keyCode = direction === 'next' ? 'MediaNextTrack' : 'MediaPreviousTrack'
    wc.sendInputEvent({ type: 'keyDown', keyCode })
    wc.sendInputEvent({ type: 'keyUp', keyCode })
  }

  /**
   * Called on every layout with the displayed tabs, before outgoing views are removed:
   * mini player for those leaving while playing, closing for those coming back.
   */
  onLayout(shown: string[]): void {
    const before = this.displayed
    this.displayed = shown
    for (const tabId of shown) if (!before.includes(tabId)) this.b.pip.onShown(tabId)
    if (!this.b.store.state.settings.autoPip) return
    for (const tabId of before) {
      if (shown.includes(tabId)) continue
      const e = this.entries.get(tabId)
      const wc = this.b.views.viewOf(tabId)?.webContents
      if (!e?.playing || !e.heard || !wc || wc.isDestroyed()) continue // no mini player for a silent decorative video
      void this.b.pip.open(tabId, { auto: true })
    }
  }

  /**
   * The main window loses focus (other app, other desktop, other window): mini player for the displayed tab
   * whose video is playing, as when switching tabs. We wait a moment to learn where the focus went.
   */
  onWindowBlur(): void {
    if (this.awayTimer) clearTimeout(this.awayTimer)
    this.awayTimer = setTimeout(() => {
      this.awayTimer = null
      if (!this.b.store.state.settings.autoPip || !this.awayFromMain()) return
      const playing = (id: string) => {
        const e = this.entries.get(id)
        const wc = this.b.views.viewOf(id)?.webContents
        return !!e?.playing && e.heard && !!wc && !wc.isDestroyed() // no mini player for a silent decorative video
      }
      const active = this.b.space.activeTabId
      const id = active && this.displayed.includes(active) && playing(active) ? active : this.displayed.find(playing)
      if (id) void this.b.pip.open(id, { auto: true, away: true })
    }, 250)
  }

  /** Back in the main window: the mini player opened when leaving it gives the video back to its tab. */
  onWindowFocus(): void {
    if (this.awayTimer) clearTimeout(this.awayTimer)
    this.awayTimer = null
    this.b.pip.onReturn()
  }

  /**
   * True if the user left the main window. Windows placed over it (extension popup, detached developer
   * tools, mini player) do not count: the page is still being watched.
   */
  private awayFromMain(): boolean {
    const main = this.b.win
    if (!main || main.isDestroyed()) return false
    if (main.isMinimized() || !main.isVisible()) return true
    if (main.isFocused()) return false
    const focused = BrowserWindow.getFocusedWindow()
    if (!focused) return true // other app, other desktop
    if (focused.getParentWindow() === main || this.b.pip.ownsWindow(focused) || this.b.devtools.tabIdOf(focused)) return false
    return !focused.webContents.getURL().startsWith('chrome-extension://')
  }

  /** Main window closing. */
  reset(): void {
    if (this.awayTimer) clearTimeout(this.awayTimer)
    this.awayTimer = null
    this.displayed = []
    for (const t of this.expiry.values()) clearTimeout(t)
    this.expiry.clear()
    this.entries.clear()
    this.stopTimer()
  }
}

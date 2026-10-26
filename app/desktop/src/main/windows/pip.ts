import { BrowserWindow, screen, systemPreferences, type Rectangle, type WebContents, type WebContentsView } from 'electron'
import type { PipCommand, PipState } from '@shared/types'
import type { Browser } from '../browser/browser'
import { UiView } from '../browser/uiview'
import { preloadPath } from '../core/urls'

const DEFAULT_WIDTH = 400
const MIN_WIDTH = 240
const MARGIN = 16
const SNAP = 48
const STEP = 15
const RADIUS = 12
const TICK = 500
/** Throw: cursor sampling window (ms), maximum speed (px/s), spring period (s), animation step (ms). */
const SAMPLE_WINDOW = 100
const MAX_VELOCITY = 8000
const RESPONSE = 0.4
const FRAME = 8

/** Distance a momentum (px/s) would cover while slowing down like a native scroll (same formula as spring.ts). */
const project = (velocity: number, rate = 0.998) => (velocity / 1000) * (rate / (1 - rate))
/** Progressive resistance beyond a limit (same formula as rubberband() in spring.ts). */
const rubberband = (d: number, dim: number, c = 0.55) => (d === 0 || dim <= 0 ? 0 : (Math.sign(d) * (Math.abs(d) * dim * c)) / (dim + c * Math.abs(d)))
const reducedMotion = () => {
  try {
    return systemPreferences.getAnimationSettings().prefersReducedMotion
  } catch {
    return false
  }
}

/** Key of the mini-player state in the page. */
const KEY = `window[Symbol.for('signet.pip')]`

/** Finds the page's videos (shadow roots included); `seen`: only those that have an image. */
const FIND = `const all = []
  const walk = (root) => {
    for (const v of root.querySelectorAll('video')) all.push(v)
    for (const e of root.querySelectorAll('*')) if (e.shadowRoot) walk(e.shadowRoot)
  }
  walk(document)
  // A music player often plays sound in a 0 × 0 <video> (e.g. Tidal): no mini-player for it.
  const seen = all.filter((e) => e.readyState > 0 && e.videoWidth > 0 && e.videoHeight > 0)`

/**
 * Marks the video (the one playing, otherwise the largest) and moves it, in the same task, to a full-screen container
 * added to `body` (an ancestor's `transform` no longer affects it). Returns its proportions, or null.
 */
const OPEN = `(() => {
  if (${KEY}) return null
  ${FIND}
  const area = (v) => { const r = v.getBoundingClientRect(); return r.width * r.height }
  const best = (list) => list.sort((a, b) => area(b) - area(a))[0]
  const v = best(seen.filter((e) => !e.paused && !e.ended)) || best(seen)
  if (!v) return null
  const wasPlaying = !v.paused
  const rb = v.getBoundingClientRect()
  const st = { v, parent: v.parentNode, next: v.nextSibling, style: v.getAttribute('style'), controls: v.controls }
  const box = document.createElement('div')
  box.setAttribute('data-signet-pip', '')
  box.style.cssText = 'position:fixed;inset:0;z-index:2147483647;background:#000;overflow:hidden;transform:none;margin:0;padding:0;border:0'
  st.box = box
  ;(document.body || document.documentElement).appendChild(box)
  box.appendChild(v)
  v.controls = false
  v.style.cssText = 'position:absolute!important;inset:0!important;width:100%!important;height:100%!important;max-width:none!important;max-height:none!important;object-fit:contain!important;transform:none!important;margin:0!important;padding:0!important;border:0!important;display:block!important;z-index:auto!important;background:#000!important'
  ${KEY} = st
  if (wasPlaying && v.paused) v.play().catch(() => {})
  return { w: v.videoWidth, h: v.videoHeight, rect: { x: rb.left, y: rb.top, width: rb.width, height: rb.height } }
})()`

/** Puts the video back exactly in its place (parent and next sibling) and removes the container. */
const restore = (pause: boolean) => `(() => {
  const s = ${KEY}
  if (!s) return false
  delete window[Symbol.for('signet.pip')]
  const v = s.v
  if (${pause}) v.pause()
  // The page moved or removed the video: we only touch our container.
  if (v.parentNode !== s.box) {
    s.box.remove()
    return true
  }
  const wasPlaying = !v.paused
  if (s.style === null) v.removeAttribute('style'); else v.setAttribute('style', s.style)
  if (v.controls !== s.controls) v.controls = s.controls
  if (s.parent && s.parent.isConnected) s.parent.insertBefore(v, s.next && s.next.parentNode === s.parent ? s.next : null)
  s.box.remove()
  if (wasPlaying && v.paused) v.play().catch(() => {})
  return true
})()`

/** Video state; null if the marked element disappeared or was moved by the page. */
const READ = `(() => {
  const s = ${KEY}
  if (!s) return null
  const v = s.v
  if (!v.isConnected || !s.box.isConnected || v.parentNode !== s.box) return null
  return { playing: !v.paused && !v.ended, muted: v.muted || v.volume === 0, currentTime: v.currentTime, duration: v.duration, w: v.videoWidth, h: v.videoHeight }
})()`

const act = (body: string) => `(() => { const s = ${KEY}; const v = s && s.v; if (!v) return; ${body} })()`
const COMMANDS: Record<string, (value: number) => string> = {
  toggle: () => act('if (v.paused) v.play().catch(() => {}); else v.pause()'),
  back: () => act(`v.currentTime = Math.max(0, v.currentTime - ${STEP})`),
  forward: () => act(`v.currentTime = Number.isFinite(v.duration) ? Math.min(v.duration, v.currentTime + ${STEP}) : v.currentTime + ${STEP}`),
  seek: (value) => act(`v.currentTime = ${Number.isFinite(value) ? Math.max(0, value) : 0}`),
  mute: () => act('v.muted = !(v.muted || v.volume === 0); if (!v.muted && v.volume === 0) v.volume = 1')
}

interface Current {
  tabId: string
  wc: WebContents
  view: WebContentsView
  win: BrowserWindow
  ui: UiView
  css: string
  ratio: number
  auto: boolean
  /** Opened when leaving the main window (the tab is still displayed there): closes again when coming back. */
  away: boolean
  closing: boolean
  hover: boolean
  /** Video rectangle in the main window, in screen coordinates, at opening (null if it was not displayed). */
  home: Rectangle | null
  /** "Back to tab" in progress: the window joins the video before closing. */
  returning: boolean
  timers: NodeJS.Timeout[]
  /** Resize in progress: starting rectangle, the screen's work area. */
  resizing: { start: Rectangle; area: Rectangle; handle: string; dx: number; dy: number; trail: { t: number; b: Rectangle }[] } | null
  off: () => void
  /** Last state sent (JSON): we only send if it changes. */
  lastJson: string
}

const clamp = (n: number, min: number, max: number) => Math.min(max, Math.max(min, n))

/** Floating mini-player: the tab's real page, moved into a small window where only the video is visible. */
export class Pip {
  private cur: Current | null = null
  private busy = false
  /** Fermeture en cours : `open` l'attend avant de repartir. */
  private closing: Promise<void> | null = null
  private animation: NodeJS.Timeout | null = null
  private glideDone: (() => void) | null = null
  /** Set by before-quit: closing the window must no longer be canceled. */
  quitting = false
  /** Recent cursor positions during the drag, for the speed at release. */
  private samples: { t: number; x: number; y: number }[] = []
  private dragTimer: NodeJS.Timeout | null = null

  constructor(private readonly b: Browser) {}

  /** True as long as this tab's view belongs to the mini-player: the main window must not display it. */
  owns(tabId: string): boolean {
    return this.cur?.tabId === tabId
  }

  get tabId(): string | null {
    return this.cur?.tabId ?? null
  }

  /** True if the page contains a video with an image (not a `<video>` that only carries sound). */
  async hasVideo(wc: WebContents): Promise<boolean> {
    if (wc.isDestroyed()) return false
    try {
      return !!(await wc.executeJavaScript(`(() => { ${FIND}; return seen.length > 0 })()`))
    } catch {
      return false
    }
  }

  /** Opens the mini-player for the tab's video (active by default). Returns false if there is nothing to show. */
  async open(tabId?: string, opts: { auto?: boolean; away?: boolean } = {}): Promise<boolean> {
    const { store } = this.b
    const id = tabId ?? this.b.space.activeTabId
    const view = id ? this.b.views.viewOf(id) : undefined
    if (!id || !store.state.tabs[id] || store.detached.has(id) || !view || view.webContents.isDestroyed() || !this.b.win) return false
    if (this.cur?.tabId === id) return true
    if (this.busy) return false
    this.busy = true
    const wc = view.webContents
    let win: BrowserWindow | null = null
    let ui: UiView | null = null
    let css = ''
    let opened = false
    // Page put back and stylesheet removed (opening is abandoned after OPEN's success).
    const undo = async () => {
      if (wc.isDestroyed()) return
      await wc.executeJavaScript(restore(false), true).catch(() => {})
      if (css) await wc.removeInsertedCSS(css).catch(() => {})
    }
    // Automatic opening abandoned if the user came back: to the tab, or to the main window (`away`).
    const stale = () => wc.isDestroyed() || (opts.away ? !!this.b.win?.isFocused() : !!opts.auto && this.b.model.displayedTabIds().includes(id))
    try {
      // A single mini-player: the previous one is paused (and its pending closing is awaited).
      if (this.cur || this.closing) await this.close('pause')
      const found = (await wc.executeJavaScript(OPEN, true)) as { w: number; h: number; rect: Rectangle } | null
      if (!found) return false
      opened = true
      const ratio = found.w > 0 && found.h > 0 ? clamp(found.w / found.h, 0.4, 3) : 16 / 9
      // The tab came back to display in the meantime (automatic opening): put everything back.
      if (stale()) {
        await undo()
        return false
      }
      // Stylesheet rule: it wins over the sizes the site rewrites inline on the video (e.g. on every resize).
      css = await wc
        .insertCSS(
          'html, body { overflow: hidden !important; } ' +
            '[data-signet-pip] > video { position: absolute !important; inset: 0 !important; left: 0 !important; top: 0 !important; ' +
            'width: 100% !important; height: 100% !important; max-width: none !important; max-height: none !important; ' +
            'object-fit: contain !important; transform: none !important; margin: 0 !important; }'
        )
        .catch(() => '')
      // Same check after the wait: the tab may have been displayed, closed or navigated in the meantime.
      if (stale()) {
        await undo()
        return false
      }
      // Window and controls view created only now (not for a tab that only has sound).
      win = this.createWindow()
      ui = new UiView('#pip')
      const home = this.videoRect(id, view, found.rect)
      const cur: Current = {
        tabId: id,
        wc,
        view,
        win,
        ui,
        css,
        ratio,
        auto: !!opts.auto,
        away: !!opts.away,
        closing: false,
        hover: false,
        home,
        returning: false,
        timers: [],
        resizing: null,
        off: () => {},
        lastJson: ''
      }
      this.cur = cur
      opened = false
      this.attach(cur)
      this.b.broadcast.changed()
      return true
    } catch {
      ui?.destroy()
      if (win && !win.isDestroyed()) win.destroy()
      if (opened && this.cur?.win !== win) await undo()
      return false
    } finally {
      this.busy = false
    }
  }

  /** Video rectangle (page CSS pixels) converted to screen coordinates; null if the tab is not visible in the main window. */
  private videoRect(id: string, view: WebContentsView, r: Rectangle | undefined): Rectangle | null {
    const main = this.b.win
    if (!r || !main || main.isDestroyed() || !main.isVisible() || main.isMinimized() || !this.b.model.displayedTabIds().includes(id)) return null
    const content = main.getContentBounds()
    const vb = view.getBounds()
    const z = view.webContents.getZoomFactor()
    const x0 = Math.max(0, r.x * z)
    const y0 = Math.max(0, r.y * z)
    const x1 = Math.min(vb.width, (r.x + r.width) * z)
    const y1 = Math.min(vb.height, (r.y + r.height) * z)
    if (x1 - x0 < 40 || y1 - y0 < 24) return null
    return { x: Math.round(content.x + vb.x + x0), y: Math.round(content.y + vb.y + y0), width: Math.round(x1 - x0), height: Math.round(y1 - y0) }
  }

  /** Frameless floating window, above everything and on all desktops, out of the Dock and ⌘Tab. */
  private createWindow(): BrowserWindow {
    const win = new BrowserWindow({
      width: DEFAULT_WIDTH,
      height: Math.round((DEFAULT_WIDTH * 9) / 16),
      show: false,
      frame: false,
      roundedCorners: true,
      hasShadow: true,
      resizable: false, // resizing done by hand (see resize), not by the system
      minimizable: false,
      maximizable: false,
      fullscreenable: false,
      skipTaskbar: true,
      alwaysOnTop: true,
      acceptFirstMouse: true,
      backgroundColor: '#000000',
      ...(process.platform === 'darwin' && { type: 'panel' as const }),
      webPreferences: { preload: preloadPath, sandbox: true, contextIsolation: true, nodeIntegration: false }
    })
    win.setAlwaysOnTop(true, 'floating')
    // `skipTransformProcessType`: do not switch the app to agent mode (the Dock and the main window do not move).
    win.setVisibleOnAllWorkspaces(true, { visibleOnFullScreen: true, skipTransformProcessType: true })
    return win
  }

  /** Places the tab's view and the controls in the window, then starts the tracking. */
  private attach(cur: Current): void {
    const { win, view, ui, wc } = cur
    // The view leaves the main window before entering here: never displayed in both places.
    this.b.layout.detachFromMain(view)
    win.contentView.addChildView(view)
    win.contentView.addChildView(ui.view)
    view.setBorderRadius(RADIUS)
    ui.view.setBorderRadius(RADIUS)
    const place = () => {
      if (win.isDestroyed()) return
      const { width, height } = win.getContentBounds()
      view.setBounds({ x: 0, y: 0, width, height })
      ui.view.setBounds({ x: 0, y: 0, width, height })
    }

    const area = this.initialBounds(cur.ratio)
    // The window is born on the video, in the page, then glides to its place (without animation if motion is reduced).
    const fromVideo = !!cur.home && !reducedMotion()
    win.setBounds(fromVideo ? cur.home! : area)
    place()
    const persist = () => {
      if (win.isDestroyed() || cur.resizing || cur.returning || this.animation) return
      this.save(cur)
    }
    win.on('resize', () => {
      place()
      persist()
    })
    win.on('move', persist)
    // Close requested (⌘W): we put the page back rather than destroy the window as is.
    win.on('close', (e) => {
      if (cur.closing) return
      if (this.quitting) return void this.close('dispose')
      e.preventDefault()
      void this.close('keep')
    })
    win.on('closed', () => {
      if (this.cur === cur && !cur.closing) void this.close('dispose')
    })
    win.on('focus', () => {
      if (!ui.webContents.isDestroyed()) ui.webContents.focus()
    })

    const gone = () => void this.close('dispose')
    // On commit only: a navigation that does not complete (download, 204) leaves the old document in place.
    wc.on('did-navigate', gone)
    wc.once('destroyed', gone)
    wc.once('render-process-gone', gone)
    cur.off = () => {
      if (wc.isDestroyed()) return
      wc.removeListener('did-navigate', gone)
      wc.removeListener('destroyed', gone)
      wc.removeListener('render-process-gone', gone)
    }

    // Without stealing the app's focus or switching desktops: the window shows without being activated.
    win.showInactive()
    if (fromVideo) this.glide(cur, area)
    if (!cur.auto) ui.webContents.focus()
    cur.timers.push(setInterval(() => void this.tick(cur), TICK))
    // The controls view subscribes a little after its load: we replay the state even if identical.
    for (const ms of [300, 1500]) {
      setTimeout(() => {
        cur.lastJson = ''
        void this.tick(cur)
      }, ms)
    }
    void this.tick(cur)
  }

  /** Starting size and position: the remembered ones if still on screen, otherwise at the bottom right of the main screen. */
  private initialBounds(ratio: number): Rectangle {
    const saved = this.b.store.state.pipBounds
    const home = this.b.win && !this.b.win.isDestroyed() ? screen.getDisplayMatching(this.b.win.getBounds()) : screen.getPrimaryDisplay()
    const probe = saved ?? home.workArea
    const wa = saved ? screen.getDisplayMatching(saved).workArea : home.workArea
    const visible = saved ? screen.getAllDisplays().some((d) => rectsOverlap(d.workArea, probe)) : false
    const width = clamp(saved?.width ?? DEFAULT_WIDTH, MIN_WIDTH, Math.max(MIN_WIDTH, Math.round(Math.min(wa.width * 0.6, wa.height * 0.9 * ratio))))
    const height = Math.round(width / ratio)
    if (saved && visible) {
      return {
        x: clamp(saved.x, wa.x, wa.x + wa.width - width),
        y: clamp(saved.y, wa.y, wa.y + wa.height - height),
        width,
        height
      }
    }
    return { x: wa.x + wa.width - MARGIN - width, y: wa.y + wa.height - MARGIN - height, width, height }
  }

  /** True if this `webContents` is the mini-player's controls view. */
  ownsSender(wc: WebContents): boolean {
    return !!this.cur && !this.cur.ui.webContents.isDestroyed() && this.cur.ui.webContents.id === wc.id
  }

  /**
   * Window drag, done here rather than by a system `drag` region: that one also swallows the clicks on the buttons
   * of a web view placed in a window. The cursor is tracked continuously; on release, snapping.
   */
  drag(on: boolean): void {
    const cur = this.cur
    const was = this.dragTimer !== null
    if (this.dragTimer) clearInterval(this.dragTimer)
    this.dragTimer = null
    if (!cur || cur.closing || cur.win.isDestroyed()) return
    if (!on) return was ? this.release(cur) : undefined
    if (cur.returning) return
    // Resuming mid-drag: we start from the window's live position.
    this.stopGlide()
    const start = screen.getCursorScreenPoint()
    const [x0, y0] = cur.win.getPosition()
    this.samples = [{ t: performance.now(), x: start.x, y: start.y }]
    this.dragTimer = setInterval(() => {
      if (cur.win.isDestroyed()) return this.drag(false)
      const p = screen.getCursorScreenPoint()
      cur.win.setPosition(x0 + p.x - start.x, y0 + p.y - start.y)
      this.sample(p)
    }, FRAME)
  }

  /**
   * Resize done here: from the starting rectangle and the ABSOLUTE pointer displacement since the start of the
   * gesture (dx, dy in screen points, like the window bounds: one point = one logical pixel, Retina or not), the grabbed edge or
   * corner stays under the pointer. Video proportions kept, opposite side fixed. Edge anchors: center
   * of the other axis (the window grows on both sides, so stays on screen more often than with a fixed corner).
   */
  resize(phase: 'start' | 'move' | 'end', handle?: string, dx = 0, dy = 0): void {
    const cur = this.cur
    if (!cur || cur.closing || cur.win.isDestroyed()) return
    if (phase === 'start') {
      if (cur.returning) return
      this.drag(false)
      this.stopGlide()
      const start = cur.win.getBounds()
      cur.resizing = { start, area: screen.getDisplayMatching(start).workArea, handle: '', dx: 0, dy: 0, trail: [] }
      return
    }
    const g = cur.resizing
    if (!g) return
    if (phase === 'end') {
      cur.resizing = null
      // Released beyond a limit: spring back to the valid size, fixed anchor, with the speed of the last gesture.
      const live = cur.win.getBounds()
      const target = g.handle ? resizedBounds(g.start, g.handle, g.dx, g.dy, cur.ratio, g.area, false) : live
      if (target.x === live.x && target.y === live.y && target.width === live.width && target.height === live.height) return this.save(cur)
      const now = performance.now()
      const recent = g.trail.filter((s) => now - s.t <= SAMPLE_WINDOW)
      const a = recent[0]
      const z = recent[recent.length - 1]
      const v = { x: 0, y: 0, width: 0, height: 0 }
      if (a && z && z.t - a.t >= 16) {
        for (const k of ['x', 'y', 'width', 'height'] as const) v[k] = clamp(((z.b[k] - a.b[k]) / (z.t - a.t)) * 1000, -MAX_VELOCITY, MAX_VELOCITY)
      }
      // The remembered size is the final one, not the overshoot.
      this.b.store.state.pipBounds = target
      this.b.store.schedule()
      this.glide(cur, target, v)
      return
    }
    if (typeof handle !== 'string' || !/^(n|s|e|w|ne|nw|se|sw)$/.test(handle) || !Number.isFinite(dx) || !Number.isFinite(dy)) return
    // First measurement of the gesture: the window may have been grabbed mid elastic return.
    if (!g.handle) g.start = unbanded(g.start, handle, cur.ratio, g.area)
    g.handle = handle
    g.dx = dx
    g.dy = dy
    const next = resizedBounds(g.start, handle, dx, dy, cur.ratio, g.area, !reducedMotion())
    g.trail.push({ t: performance.now(), b: next })
    if (g.trail.length > 12) g.trail.shift()
    const now = cur.win.getBounds()
    if (next.x === now.x && next.y === now.y && next.width === now.width && next.height === now.height) return
    cur.win.setBounds(next)
    // Views follow right away (the system `resize` event comes later).
    const { width, height } = cur.win.getContentBounds()
    cur.view.setBounds({ x: 0, y: 0, width, height })
    cur.ui.view.setBounds({ x: 0, y: 0, width, height })
  }

  private sample(p: { x: number; y: number }): void {
    const t = performance.now()
    this.samples.push({ t, x: p.x, y: p.y })
    while (this.samples.length > 2 && t - this.samples[0].t > SAMPLE_WINDOW) this.samples.shift()
  }

  /**
   * Release: cursor speed over the last ~100 ms, projected end point (like a native scroll). Near a corner
   * (less than 48 px) the window snaps to 16 px from the edges; otherwise it lands where the momentum takes it, within the work area.
   */
  private release(cur: Current): void {
    if (cur.win.isDestroyed()) return
    this.sample(screen.getCursorScreenPoint())
    const first = this.samples[0]
    const last = this.samples[this.samples.length - 1]
    const dt = last.t - first.t
    const v = { x: 0, y: 0 }
    if (dt >= 16) {
      v.x = clamp(((last.x - first.x) / dt) * 1000, -MAX_VELOCITY, MAX_VELOCITY)
      v.y = clamp(((last.y - first.y) / dt) * 1000, -MAX_VELOCITY, MAX_VELOCITY)
    }
    this.samples = []
    const r = cur.win.getBounds()
    const here = screen.getDisplayMatching(r).workArea
    const px = r.x + clamp(project(v.x), -here.width, here.width)
    const py = r.y + clamp(project(v.y), -here.height, here.height)
    const wa = screen.getDisplayNearestPoint({ x: Math.round(px + r.width / 2), y: Math.round(py + r.height / 2) }).workArea
    const xs = [wa.x + MARGIN, wa.x + wa.width - MARGIN - r.width]
    const ys = [wa.y + MARGIN, wa.y + wa.height - MARGIN - r.height]
    let best: [number, number] | null = null
    let bestD = Infinity
    for (const x of xs) {
      for (const y of ys) {
        const dx = Math.abs(px - x)
        const dy = Math.abs(py - y)
        if (dx < SNAP && dy < SNAP && dx + dy < bestD) {
          best = [x, y]
          bestD = dx + dy
        }
      }
    }
    const x = best ? best[0] : clamp(Math.round(px), wa.x, Math.max(wa.x, wa.x + wa.width - r.width))
    const y = best ? best[1] : clamp(Math.round(py), wa.y, Math.max(wa.y, wa.y + wa.height - r.height))
    if (x === r.x && y === r.y) return this.save(cur)
    this.glide(cur, { ...r, x, y }, v)
  }

  /**
   * Animated move: one critically damped spring per axis (x, y, and width / height if they change), each launched with its own
   * speed. Analytic solution from the clock: timer irregularities are not visible. Reduced motion:
   * the window is set at once. `keep`: remember the end rectangle.
   */
  private glide(
    cur: Current,
    to: Rectangle,
    v: { x: number; y: number; width?: number; height?: number } = { x: 0, y: 0 },
    done?: () => void,
    keep = true
  ): void {
    this.stopGlide()
    this.glideDone = done ?? null
    const win = cur.win
    const finish = () => {
      if (win.isDestroyed()) return
      if (resize) win.setBounds(to)
      else win.setPosition(to.x, to.y)
      if (keep) this.save(cur)
      done?.()
    }
    const from = win.getBounds()
    const resize = from.width !== to.width || from.height !== to.height
    if (reducedMotion()) return finish()
    const omega = (2 * Math.PI) / RESPONSE
    const keys = (resize ? ['x', 'y', 'width', 'height'] : ['x', 'y']) as ('x' | 'y' | 'width' | 'height')[]
    const vel = (k: 'x' | 'y' | 'width' | 'height') => v[k] ?? 0
    const start = performance.now()
    this.animation = setInterval(() => {
      if (win.isDestroyed()) return this.stopGlide()
      const t = (performance.now() - start) / 1000
      const decay = Math.exp(-omega * t)
      const next = { ...to }
      let settled = true
      for (const k of keys) {
        const x0 = from[k] - to[k]
        const c2 = vel(k) + omega * x0
        const x = (x0 + c2 * t) * decay
        const speed = (c2 - omega * (x0 + c2 * t)) * decay
        if (Math.abs(x) >= 0.5 || Math.abs(speed) >= 8) settled = false
        next[k] = Math.round(to[k] + x)
      }
      if (settled || t > 3) {
        this.glideDone = null // finish() calls done itself
        this.stopGlide()
        return finish()
      }
      if (resize) win.setBounds(next)
      else win.setPosition(next.x, next.y)
    }, FRAME)
  }

  private stopGlide(): void {
    if (this.animation) clearInterval(this.animation)
    this.animation = null
    // An interrupted glide still releases whoever is waiting for it.
    const done = this.glideDone
    this.glideDone = null
    done?.()
  }

  private save(cur: Current): void {
    if (cur.win.isDestroyed()) return
    this.b.store.state.pipBounds = cur.win.getBounds()
    this.b.store.schedule()
  }

  /** Re-reads the page and pushes the state to the controls view; closes if the video disappeared. */
  private async tick(cur: Current): Promise<void> {
    if (this.cur !== cur || cur.closing || cur.wc.isDestroyed()) return
    try {
      const r = (await cur.wc.executeJavaScript(READ)) as {
        playing: boolean
        muted: boolean
        currentTime: number
        duration: number
        w: number
        h: number
      } | null
      if (this.cur !== cur || cur.closing) return
      if (!r) return void this.close('keep') // the marked element disappeared: close cleanly
      this.syncRatio(cur, r.w, r.h)
      const tab = this.b.store.state.tabs[cur.tabId]
      let site = ''
      try {
        site = new URL(tab?.url ?? '').hostname.replace(/^www\./i, '')
      } catch {
        site = tab?.url ?? ''
      }
      const state: PipState = {
        tabId: cur.tabId,
        site,
        title: tab?.customTitle || tab?.title || site,
        playing: r.playing,
        muted: r.muted,
        currentTime: r.currentTime,
        duration: r.duration,
        hue: this.b.space.hue
      }
      const json = JSON.stringify(state)
      if (json === cur.lastJson) return
      cur.lastJson = json
      cur.ui.send('pip:state', state)
    } catch {
      /* page navigating: the navigation event will close */
    }
  }

  /** The video's proportions may only be known after startup: we adjust the window. */
  private syncRatio(cur: Current, w: number, h: number): void {
    if (!(w > 0 && h > 0) || cur.win.isDestroyed()) return
    const ratio = clamp(w / h, 0.4, 3)
    if (Math.abs(ratio - cur.ratio) < 0.01) return
    if (cur.resizing) return // no proportion change mid-gesture
    cur.ratio = ratio
    const b = cur.win.getBounds()
    const wa = screen.getDisplayMatching(b).workArea
    const width = Math.max(MIN_WIDTH, Math.min(b.width, Math.floor(wa.height * 0.9 * ratio)))
    const height = Math.round(width / ratio)
    cur.win.setBounds({ x: clamp(b.x, wa.x, wa.x + wa.width - width), y: clamp(b.y, wa.y, wa.y + wa.height - height), width, height })
  }

  /** Controls view command. */
  async command(command: PipCommand, value?: number): Promise<void> {
    const cur = this.cur
    if (!cur || cur.closing) return
    if (command === 'return') return this.returnToTab()
    if (command === 'close') return this.close('keep') // closing lets the video keep playing in its tab
    const script = COMMANDS[command]
    if (!script || cur.wc.isDestroyed()) return
    try {
      await cur.wc.executeJavaScript(script(Number(value)), true)
    } catch {
      /* page en cours de navigation */
    }
    void this.tick(cur)
  }

  /**
   * Closes the mini-player. `pause`: pauses the video; `keep`: playback continues; `dispose`: the page is no longer
   * there (closed, navigation), nothing to put back.
   */
  close(mode: 'pause' | 'keep' | 'dispose'): Promise<void> {
    const cur = this.cur
    if (!cur) return this.closing ?? Promise.resolve()
    if (cur.closing) return this.closing ?? Promise.resolve()
    cur.closing = true
    const p: Promise<void> = this.finish(cur, mode).finally(() => {
      if (this.closing === p) this.closing = null
    })
    this.closing = p
    return p
  }

  private async finish(cur: Current, mode: 'pause' | 'keep' | 'dispose'): Promise<void> {
    for (const t of cur.timers) clearInterval(t)
    cur.off()
    this.drag(false)
    this.stopGlide()
    cur.resizing = null
    if (mode !== 'dispose' && !cur.wc.isDestroyed()) {
      try {
        await cur.wc.executeJavaScript(restore(mode === 'pause'), true)
        if (cur.css) await cur.wc.removeInsertedCSS(cur.css)
      } catch {
        /* page closed in the meantime */
      }
    }
    const { win, view, ui } = cur
    // The view leaves this window before the main one can take it back.
    if (!win.isDestroyed()) {
      try {
        win.contentView.removeChildView(ui.view)
        win.contentView.removeChildView(view)
      } catch {
        /* already removed */
      }
    }
    if (this.cur === cur) this.cur = null
    ui.destroy()
    if (!win.isDestroyed()) win.destroy()
    this.b.broadcast.changed()
  }

  /** "Back to tab": the page returns to the main window, the tab is activated and the window comes to the front. */
  private async returnToTab(): Promise<void> {
    const cur = this.cur
    if (!cur || cur.returning || cur.closing) return
    const id = cur.tabId
    cur.returning = true
    this.drag(false)
    this.stopGlide()
    cur.resizing = null
    // The window first joins the video (playback continues in the window), then the page takes its place back.
    const main = this.b.win
    if (main && !main.isDestroyed() && main.isVisible() && !main.isMinimized() && !reducedMotion()) {
      this.b.window.show()
      const target = cur.home ?? this.fallbackRect(main)
      await new Promise<void>((resolve) => this.glide(cur, target, undefined, resolve, false))
      if (this.cur !== cur || cur.closing) return
    }
    await this.close('keep')
    this.b.tabs.activate(id)
    this.b.window.show()
  }

  /** With no known video position: a small rectangle in the center of the main window. */
  private fallbackRect(main: BrowserWindow): Rectangle {
    const c = main.getContentBounds()
    const width = Math.round(c.width / 3)
    const height = Math.round(width / 1.78)
    return { x: Math.round(c.x + (c.width - width) / 2), y: Math.round(c.y + (c.height - height) / 2), width, height }
  }

  /** Called by the main window on each layout: the tab coming back to display takes its page back. */
  onShown(tabId: string): void {
    if (this.cur?.tabId === tabId && !this.cur.closing) void this.close('keep')
  }

  /** Back in the main window: the mini-player opened when leaving it closes, the video keeps playing in the tab. */
  onReturn(): void {
    const cur = this.cur
    if (cur?.away && !cur.closing && this.b.model.displayedTabIds().includes(cur.tabId)) void this.close('keep')
  }

  /** True for the mini-player window. */
  ownsWindow(win: BrowserWindow): boolean {
    return this.cur?.win === win
  }

  /** The tab's view is destroyed (tab closed, main window closed). */
  onGone(tabId: string): void {
    if (this.cur?.tabId === tabId) void this.close('dispose')
  }
}

function rectsOverlap(a: Rectangle, b: Rectangle): boolean {
  return a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height
}

/** Inverse of `rubberband`: the raw overshoot that produces this displayed overshoot (same formula as unrubberband() in spring.ts). */
const unrubberband = (r: number, dim: number, c = 0.55) => {
  const a = Math.min(Math.abs(r), dim * 0.999)
  return (Math.sign(r) * (a * dim)) / (c * (dim - a))
}

/**
 * "Raw" starting rectangle when the window is grabbed during its elastic return: the displayed size is already
 * resisted, we go back up to the equivalent raw size so as not to apply the resistance twice.
 */
function unbanded(live: Rectangle, handle: string, ratio: number, wa: Rectangle): Rectangle {
  const valid = resizedBounds(live, handle, 0, 0, ratio, wa, false)
  const over = live.width - valid.width
  if (Math.abs(over) < 1) return live
  const rawW = Math.round(valid.width + unrubberband(over, valid.width))
  const rawH = Math.round(rawW / ratio)
  const x = handle.includes('w') ? live.x + live.width - rawW : handle.includes('e') ? live.x : Math.round(live.x + (live.width - rawW) / 2)
  const y = handle.includes('n') ? live.y + live.height - rawH : handle.includes('s') ? live.y : Math.round(live.y + (live.height - rawH) / 2)
  return { x, y, width: rawW, height: rawH }
}

/**
 * New rectangle for a handle dragged by (dx, dy) from the start. A single path for the 8 handles: raw wanted
 * width (edges: follows the pointer exactly; corners: point on the proportion diagonal closest), then limits,
 * then height from the proportions and position from the anchor.
 * Limits: [MIN_WIDTH, 90% of the work area] are elastic (`soft`): beyond them, the window keeps following with an
 * increasing resistance (rubberband) and `soft = false` gives the valid size it returns to on release. The screen
 * edge stays hard: the window never crosses it (the system does not have to push the anchor back); without `soft`, everything is hard.
 */
export function resizedBounds(s: Rectangle, handle: string, dx: number, dy: number, ratio: number, wa: Rectangle, soft = true): Rectangle {
  const east = handle.includes('e')
  const west = handle.includes('w')
  const south = handle.includes('s')
  const north = handle.includes('n')
  const roomX = east ? wa.x + wa.width - s.x : west ? s.x + s.width - wa.x : wa.width
  const roomY = south ? wa.y + wa.height - s.y : north ? s.y + s.height - wa.y : wa.height
  const edge = Math.max(MIN_WIDTH, Math.floor(Math.min(roomX, roomY * ratio)))
  const maxW = Math.min(edge, Math.max(MIN_WIDTH, Math.floor(Math.min(wa.width * 0.9, wa.height * 0.9 * ratio))))
  let raw: number
  if (handle.length === 2) {
    // The opposite corner (anchor) does not move: anchor → pointer vector, projected onto the diagonal.
    const vx = east ? s.width + dx : s.width - dx
    const vy = south ? s.height + dy : s.height - dy
    raw = (ratio * (ratio * vx + vy)) / (ratio * ratio + 1)
  } else if (east || west) {
    raw = east ? s.width + dx : s.width - dx
  } else {
    raw = (south ? s.height + dy : s.height - dy) * ratio
  }
  let w: number
  if (!soft) w = clamp(raw, MIN_WIDTH, maxW)
  else if (raw > maxW) w = Math.min(edge, maxW + rubberband(raw - maxW, maxW))
  else if (raw < MIN_WIDTH) w = MIN_WIDTH + rubberband(raw - MIN_WIDTH, MIN_WIDTH)
  else w = raw
  w = Math.round(w)
  const h = Math.round(w / ratio)
  // Free axis of an edge: centered, shifted if needed to stay within the work area.
  const x = west ? s.x + s.width - w : east ? s.x : clamp(Math.round(s.x + (s.width - w) / 2), wa.x, wa.x + wa.width - w)
  const y = north ? s.y + s.height - h : south ? s.y : clamp(Math.round(s.y + (s.height - h) / 2), wa.y, wa.y + wa.height - h)
  return { x, y, width: w, height: h }
}

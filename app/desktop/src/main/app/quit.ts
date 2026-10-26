import { app, BrowserWindow, globalShortcut, screen } from 'electron'
import type { Input } from 'electron'
import { execFile } from 'node:child_process'
import type { QuitHudState } from '@shared/types'
import { denyWindowOpen } from '../web/guard'
import { loadRoute, preloadPath } from '../core/urls'
import { pushUi } from '../ipc/push'

const IS_MAC = process.platform === 'darwin'
/** Quit: ⌘Q on Mac, Ctrl+Q on Windows and Linux (the same for the menu and the fallback shortcut). */
export const QUIT_ACCELERATOR = IS_MAC ? 'Command+Q' : 'Ctrl+Q'
/** Name (`input.key` in lowercase) of the modifier whose release stops the gauge. */
const QUIT_MODIFIER_KEY = IS_MAC ? 'meta' : 'control'

/** Hold duration, window for the second press, fade durations. */
const HOLD_MS = 900
const AGAIN_MS = 2000
const FADE_MS = 150
const TICK_MS = 30 // check of the threshold and of the release (no more progress sending)
/** Highest gauge share reached by the expected first repeat: the hold lasts longer when the repeat delay is long. */
const WAIT_SHARE = 0.75
/** Repeat delay taken into account at most (key repeat turned off sets a huge value). */
const MAX_WAIT_MS = 1500

/** Delay before warming up the bubble (its page loads the whole UI bundle). */
const HUD_WARM_MS = 5000
const HUD_W = 320
const HUD_H = 120

let enabled: () => boolean = () => true
/** Quit action. */
const doQuit = (): void => app.quit()

let hud: BrowserWindow | null = null
let hudReady = false
let pending: QuitHudState | null = null
let fadeTimer: NodeJS.Timeout | null = null

/** Press in progress (⌘Q held down), bubble waiting for a second press, timers. */
let pressing = false
let armed = false
let pressStart = 0
let holdFor = HOLD_MS
let tick: NodeJS.Timeout | null = null
let hideTimer: NodeJS.Timeout | null = null
/**
 * macOS does not report the release of Q while ⌘ stays held: it is inferred from the repeat rhythm
 * (delay before the first one, then interval, read from the system keyboard settings).
 */
let gotRepeat = false
let lastEvent = 0
let initialMs = 700
let repeatMs = 90
let fallbackOn = false
let appActive = false

/** Confirmation setting changed: the bubble is destroyed if it is no longer needed (it is recreated on demand). */
export function syncQuitHud(): void {
  if (!enabled() && hud && !hud.isDestroyed() && !pressing && !armed) hud.destroy()
}

/** Initializes the quit confirmation; to call before the views are created. */
export function initQuit(isEnabled: () => boolean): void {
  enabled = isEnabled
  // All the app's views: web pages included.
  app.on('web-contents-created', (_e, wc) => {
    wc.on('before-input-event', (event, input) => {
      if (handleInput(input)) event.preventDefault()
    })
  })
  const refresh = () => setTimeout(syncFallback, 0)
  app.on('browser-window-focus', refresh)
  app.on('browser-window-blur', (_e, win) => {
    if (win !== hud) release()
    refresh()
  })
  app.on('browser-window-created', refresh)
  app.on('did-become-active', () => {
    appActive = true
    refresh()
  })
  app.on('did-resign-active', () => {
    appActive = false
    release()
    syncFallback()
  })
  app.on('activate', () => {
    appActive = true
    refresh()
  })
  // `globalShortcut` only exists once the app is ready (a second instance quits before).
  app.on('will-quit', () => app.isReady() && globalShortcut.unregister(QUIT_ACCELERATOR))
  void app.whenReady().then(() => {
    appActive = app.isReady() && BrowserWindow.getAllWindows().some((w) => w.isFocused())
    if (IS_MAC) readKeyboardTimings() // macOS keyboard settings (`defaults`)
    // The bubble is warmed up when idle; if ⌘Q arrives before, `showHud` creates it on demand.
    setTimeout(() => {
      if (!hud && enabled()) createHud()
    }, HUD_WARM_MS).unref()
  })
}

/** Repeat delay and repeat interval (values in fifteenths of a second; absent = default values). */
function readKeyboardTimings(): void {
  const read = (key: string, set: (ms: number) => void) =>
    execFile('defaults', ['read', '-g', key], (err, out) => {
      const n = Number(String(out).trim())
      if (!err && Number.isFinite(n) && n > 0) set(n * 15)
    })
  read('InitialKeyRepeat', (ms) => (initialMs = ms))
  read('KeyRepeat', (ms) => (repeatMs = ms))
}

/** Hold duration: never shorter than HOLD_MS, longer if the first repeat comes late (the gauge must not be nearly full before it). */
function holdDuration(): number {
  return Math.max(HOLD_MS, Math.min(initialMs, MAX_WAIT_MS) / WAIT_SHARE)
}

/** True while a shortcut is being typed in Settings: ⌘Q is a key like any other there. */
let paused = false

/** Suspends (or restores) ⌘Q interception (shortcut being typed, see menu.ts). */
export function pauseQuitShortcut(on: boolean): void {
  paused = on
  if (on) release()
}

/** Quits without confirmation (menu item, explicit click). */
export function quitNow(): void {
  doQuit()
}

/** Handles a key; returns true if it is exactly ⌘Q (or its release) and it must be intercepted. */
function handleInput(input: Input): boolean {
  const key = input.key.toLowerCase()
  if (input.type === 'keyUp') {
    if (key === 'q' || key === QUIT_MODIFIER_KEY) release()
    return false
  }
  if (input.type !== 'keyDown' || key !== 'q' || paused) return false
  // ⌘Q on Mac, Ctrl+Q on Windows and Linux: the modifier alone, with no other.
  const mod = IS_MAC ? input.meta && !input.control : input.control && !input.meta
  if (!mod || input.shift || input.alt) return false
  if (!enabled()) {
    doQuit()
    return true
  }
  press(input.isAutoRepeat)
  return true
}

/** ⌘Q held down: starts the gauge, or quits if the bubble is already waiting for a second press. */
function press(repeat: boolean): void {
  if (repeat) {
    // Repeat of the press in progress: the gauge continues; otherwise (a press whose start we missed) ignored.
    if (pressing) {
      // First repeat: the view lifts the gauge's wait ceiling (sent only once).
      if (!gotRepeat) sendHud({ ...holdState(), repeating: true })
      gotRepeat = true
      lastEvent = Date.now()
    }
    return
  }
  // New press (no visible release while ⌘ stays held): second press if there is a bubble.
  if (armed) {
    confirmQuit()
    return
  }
  pressing = true
  armed = true
  pressStart = Date.now()
  holdFor = holdDuration()
  lastEvent = pressStart
  gotRepeat = false
  clearHide()
  showHud(holdState())
  tick = setInterval(() => {
    const now = Date.now()
    // No repeat at the expected time: the key was released.
    if (now - lastEvent > (gotRepeat ? Math.max(150, repeatMs * 2) : initialMs + 150)) return release()
    // The gauge animates in the view from `pressStart`; main alone keeps the threshold. Without repeats, we cannot tell whether the press is brief: no quit.
    if (gotRepeat && now - pressStart >= holdFor) {
      stopTick()
      pressing = false
      confirmQuit()
    }
  }, TICK_MS)
}

/** Gauge state of the press in progress: start, duration and expected first repeat (the view slows down after it). */
function holdState(): QuitHudState {
  return { progress: 0, holding: true, start: pressStart, duration: holdFor, wait: Math.min(initialMs, MAX_WAIT_MS) }
}

/** Press without hold (fallback with no window): first time bubble, second time quit. */
function pressTwice(): void {
  if (!enabled()) return doQuit()
  if (armed) return confirmQuit()
  armed = true
  showHud({ progress: 0, holding: false })
  scheduleHide()
}

/** Confirmed quit: the bubble closes, then we quit. */
function confirmQuit(): void {
  armed = false
  pressing = false
  stopTick()
  clearHide()
  hideHud()
  doQuit()
}

function release(): void {
  if (!pressing) return
  pressing = false
  stopTick()
  sendHud({ progress: 0, holding: false })
  scheduleHide()
}

function stopTick(): void {
  if (tick) clearInterval(tick)
  tick = null
}

function clearHide(): void {
  if (hideTimer) clearTimeout(hideTimer)
  hideTimer = null
}

function scheduleHide(): void {
  clearHide()
  hideTimer = setTimeout(() => {
    armed = false
    hideHud()
  }, AGAIN_MS)
}

// --- Global fallback shortcut: only when the app is active but no window is focused ---

function syncFallback(): void {
  const focused = BrowserWindow.getAllWindows().some((w) => w !== hud && !w.isDestroyed() && w.isFocused())
  const want = appActive && !focused
  if (want === fallbackOn || !app.isReady()) return
  fallbackOn = want
  if (want) {
    if (!globalShortcut.register(QUIT_ACCELERATOR, pressTwice)) fallbackOn = false
  } else globalShortcut.unregister(QUIT_ACCELERATOR)
}

// --- Bubble ---

function createHud(): void {
  hud = new BrowserWindow({
    width: HUD_W,
    height: HUD_H,
    show: false,
    frame: false,
    transparent: true,
    hasShadow: false,
    resizable: false,
    movable: false,
    minimizable: false,
    maximizable: false,
    fullscreenable: false,
    focusable: false,
    skipTaskbar: true,
    alwaysOnTop: true,
    backgroundColor: '#00000000',
    ...(process.platform === 'darwin' && { type: 'panel' as const }),
    webPreferences: { preload: preloadPath, sandbox: true, contextIsolation: true, nodeIntegration: false }
  })
  hud.setAlwaysOnTop(true, 'screen-saver')
  hud.setVisibleOnAllWorkspaces(true, { visibleOnFullScreen: true, skipTransformProcessType: true })
  hud.setIgnoreMouseEvents(true)
  hud.webContents.on('did-finish-load', () => {
    hudReady = true
    if (pending && hud) pushUi(hud.webContents, 'quit:state', pending)
    setTimeout(() => {
      if (pending && hud && !hud.isDestroyed()) pushUi(hud.webContents, 'quit:state', pending)
    }, 150)
  })
  hud.on('closed', () => {
    hud = null
    hudReady = false
  })
  denyWindowOpen(hud.webContents)
  loadRoute(hud.webContents, '#quit')
}

function sendHud(s: QuitHudState): void {
  pending = s
  if (hud && !hud.isDestroyed() && hudReady) pushUi(hud.webContents, 'quit:state', s)
}

function fade(to: number, done?: () => void): void {
  if (fadeTimer) clearInterval(fadeTimer)
  const win = hud
  if (!win || win.isDestroyed()) return
  const from = win.getOpacity()
  const t0 = Date.now()
  fadeTimer = setInterval(() => {
    if (win.isDestroyed()) return void (fadeTimer && clearInterval(fadeTimer))
    const k = Math.min(1, (Date.now() - t0) / FADE_MS)
    win.setOpacity(from + (to - from) * k)
    if (k >= 1) {
      if (fadeTimer) clearInterval(fadeTimer)
      fadeTimer = null
      done?.()
    }
  }, 16)
}

/** Horizontally centered on the focused window (or the main screen), at the first third of its height. */
function place(): void {
  if (!hud || hud.isDestroyed()) return
  const ref =
    BrowserWindow.getAllWindows()
      .find((w) => w !== hud && w.isFocused())
      ?.getBounds() ?? screen.getPrimaryDisplay().workArea
  hud.setPosition(Math.round(ref.x + ref.width / 2 - HUD_W / 2), Math.round(ref.y + ref.height / 3 - HUD_H / 2))
}

function showHud(s: QuitHudState): void {
  if (!hud || hud.isDestroyed()) createHud()
  sendHud(s)
  if (!hud || hud.isDestroyed()) return
  if (!hud.isVisible()) {
    place()
    hud.setOpacity(0)
    hud.showInactive()
  }
  fade(1)
}

/** True for the quit HUD window (which does not count as a visible window of the app). */
export function isQuitHud(win: BrowserWindow): boolean {
  return win === hud
}

function hideHud(): void {
  const win = hud
  if (!win || win.isDestroyed() || !win.isVisible()) return
  fade(0, () => {
    if (!win.isDestroyed() && !armed && !pressing) win.hide()
  })
}

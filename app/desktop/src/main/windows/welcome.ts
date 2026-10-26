import { app, BrowserWindow } from 'electron'
import { windowChrome } from '../app/platform'
import type { Browser } from '../browser/browser'
import { loadRoute, preloadPath } from '../core/urls'
import { t } from '../app/i18n'

const WIDTH = 640
const HEIGHT = 580

/** First-launch introduction: a single window, centered on the main window. */
export class WelcomeWindow {
  private win: BrowserWindow | null = null
  private quitting = false

  constructor(private b: Browser) {
    app.on('before-quit', () => (this.quitting = true))
  }

  /** True as long as the introduction has been neither finished nor skipped (fresh state). */
  get pending(): boolean {
    return this.b.store.state.settings.onboarded === false
  }

  open(): void {
    if (this.win && !this.win.isDestroyed()) {
      if (this.win.isMinimized()) this.win.restore()
      this.win.show()
      this.win.focus()
      return
    }
    const parent = this.b.win && !this.b.win.isDestroyed() ? this.b.win : undefined
    const main = parent ? parent.getBounds() : null
    const win = new BrowserWindow({
      // Child window: stays above the main window, even after a click on it (without blocking it).
      parent,
      width: WIDTH,
      height: HEIGHT,
      ...(main && { x: Math.round(main.x + (main.width - WIDTH) / 2), y: Math.round(main.y + (main.height - HEIGHT) / 2) }),
      center: !main,
      resizable: false,
      minimizable: false,
      maximizable: false,
      fullscreenable: false,
      title: t('window.welcome'),
      ...windowChrome({ lights: { x: 16, y: 18 }, vibrancy: true }),
      webPreferences: { preload: preloadPath, sandbox: true, contextIsolation: true, nodeIntegration: false }
    })
    this.win = win
    // Closing the window = skipping the introduction (unless the app is quitting: it will come back at next launch).
    win.on('closed', () => {
      this.win = null
      if (!this.quitting) this.markDone()
    })
    loadRoute(win.webContents, '#welcome')
  }

  /** Finished or skipped: will not come back, and the window closes. */
  finish(): void {
    this.markDone()
    if (this.win && !this.win.isDestroyed()) this.win.close()
  }

  private markDone(): void {
    const { settings } = this.b.store.state
    if (settings.onboarded === true) return
    settings.onboarded = true
    this.b.broadcast.changed()
  }

  /** Window title in the current language. */
  retitle(): void {
    if (this.win && !this.win.isDestroyed()) this.win.setTitle(t('window.welcome'))
  }
}

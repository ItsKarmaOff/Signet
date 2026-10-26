import { app, BrowserWindow, dialog, type MessageBoxOptions } from 'electron'
import type { AppUpdater } from 'electron-updater'
import type { Browser } from '../browser/browser'
import { t } from '../app/i18n'

/** Public repository: its GitHub releases are the update feed (see `publish` in electron-builder.mjs). */
export const REPO_URL = 'https://github.com/ItsKarmaOff/Signet'

/** First automatic check a little after launch, then every 6 hours. */
const FIRST_CHECK_MS = 30_000
const CHECK_EVERY_MS = 6 * 60 * 60_000

/**
 * Whether this copy can download and install an update by itself.
 * - Windows (NSIS installer) and Linux (AppImage): yes.
 * - macOS: no. macOS only installs an update over an app signed with an Apple Developer ID; until then,
 *   Signet only says a new version exists and opens its release page.
 * - Linux deb and rpm packages: no, they are installed by the system's package manager (admin password). Signet opens
 *   the release page, like on macOS. Same outside any package (run from an unpacked folder).
 */
function canInstall(): boolean {
  return process.platform === 'win32' || (process.platform === 'linux' && !!process.env.APPIMAGE)
}

/** Updates: automatic checks (Settings › About) and Help › Check for Updates. */
export class Updates {
  private timer: NodeJS.Timeout | null = null
  private updater: AppUpdater | null = null
  private checking = false
  /** Version being downloaded, then downloaded and waiting for a restart. */
  private downloading: string | null = null
  private downloaded: string | null = null
  /** Version already offered in this session: automatic checks do not ask about it again. */
  private offered: string | null = null

  constructor(private readonly b: Browser) {}

  /** (Re)schedules the automatic checks, following the setting. Packaged app only. */
  sync(): void {
    if (this.timer) clearTimeout(this.timer)
    this.timer = null
    if (!app.isPackaged || this.b.store.state.settings.checkUpdates === false) return
    this.schedule(FIRST_CHECK_MS)
  }

  private schedule(delay: number): void {
    this.timer = setTimeout(() => {
      void this.check(false).finally(() => this.schedule(CHECK_EVERY_MS))
    }, delay)
  }

  /** Looks for a new version. `manual`: asked by the user, so every outcome is shown (up to date, error…). */
  async check(manual: boolean): Promise<void> {
    if (!app.isPackaged) {
      if (manual) await this.message({ message: t('updates.devOnly'), detail: t('updates.devOnlyDetail') })
      return
    }
    if (this.downloaded) {
      if (manual) await this.offerRestart(this.downloaded)
      return
    }
    if (this.downloading) {
      if (manual) await this.message({ message: t('updates.downloading', { version: this.downloading }), detail: t('updates.downloadingDetail') })
      return
    }
    if (this.checking) return
    this.checking = true
    try {
      const updater = await this.load()
      const result = await updater.checkForUpdates()
      const version = result?.isUpdateAvailable ? result.updateInfo.version : null
      if (!version) {
        if (manual) await this.message({ message: t('updates.upToDate'), detail: t('updates.upToDateDetail', { version: app.getVersion() }) })
        return
      }
      if (!manual && this.offered === version) return
      this.offered = version
      if (canInstall()) await this.download(updater, version, manual)
      else await this.offerDownload(version)
    } catch (err) {
      console.warn('[updates] check failed:', err)
      this.offered = null
      if (manual) await this.message({ type: 'warning', message: t('updates.failed'), detail: t('updates.failedDetail') })
    } finally {
      this.checking = false
    }
  }

  /** Windows and Linux: downloads in the background, then offers to restart. */
  private async download(updater: AppUpdater, version: string, manual: boolean): Promise<void> {
    this.downloading = version
    // Not awaited: the download goes on while the message is open.
    if (manual) void this.message({ message: t('updates.downloading', { version }), detail: t('updates.downloadingDetail') })
    try {
      await updater.downloadUpdate()
    } finally {
      this.downloading = null
    }
    this.downloaded = version
    await this.offerRestart(version)
  }

  /** Restart now, or the update is installed at the next quit (`autoInstallOnAppQuit`). */
  private async offerRestart(version: string): Promise<void> {
    const { response } = await this.message({
      message: t('updates.ready', { version }),
      detail: t('updates.readyDetail'),
      buttons: [t('updates.restart'), t('updates.later')],
      defaultId: 0,
      cancelId: 1
    })
    // Silent install, then Signet starts again.
    if (response === 0) setImmediate(() => this.updater?.quitAndInstall(true, true))
  }

  /** macOS and Linux deb/rpm: the new version is downloaded by hand from its release page. */
  private async offerDownload(version: string): Promise<void> {
    const { response } = await this.message({
      message: t('updates.available', { version }),
      detail: t('updates.availableDetail', { current: app.getVersion() }),
      buttons: [t('updates.download'), t('updates.later')],
      defaultId: 0,
      cancelId: 1
    })
    if (response === 0) this.openPage(`${REPO_URL}/releases/tag/v${version}`)
  }

  /** Opens a page in a new tab of the main window (Help menu, release page). */
  openPage(url: string): void {
    this.b.window.show()
    this.b.tabs.open(url, { activate: true })
  }

  /** electron-updater, loaded on the first check (it reads app-update.yml, written by electron-builder). */
  private async load(): Promise<AppUpdater> {
    if (this.updater) return this.updater
    // electron-updater is a CommonJS module whose `autoUpdater` is a getter: loaded with import() in the built app,
    // it is only reachable through `default` (the named export is undefined there).
    const mod: { autoUpdater?: AppUpdater; default?: { autoUpdater?: AppUpdater } } = await import('electron-updater')
    const autoUpdater = mod.default?.autoUpdater ?? mod.autoUpdater
    if (!autoUpdater) throw new Error('electron-updater: autoUpdater not found')
    autoUpdater.autoDownload = false
    autoUpdater.autoInstallOnAppQuit = true
    // Errors are handled where `checkForUpdates` and `downloadUpdate` are awaited.
    autoUpdater.on('error', () => {})
    this.updater = autoUpdater
    return autoUpdater
  }

  private message(options: Omit<MessageBoxOptions, 'buttons'> & { buttons?: string[] }) {
    const opts: MessageBoxOptions = { type: 'info', buttons: [t('updates.ok')], ...options }
    const win = BrowserWindow.getFocusedWindow() ?? (this.b.win && !this.b.win.isDestroyed() ? this.b.win : null)
    return win ? dialog.showMessageBox(win, opts) : dialog.showMessageBox(opts)
  }
}

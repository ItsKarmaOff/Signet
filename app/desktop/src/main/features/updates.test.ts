import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const env = vi.hoisted(() => ({
  packaged: true,
  /** Button clicked in each message box, in order (missing = 1, "Later"). */
  responses: [] as number[],
  messages: [] as string[]
}))

vi.mock('electron', () => ({
  app: {
    get isPackaged() {
      return env.packaged
    },
    getVersion: () => '1.0.0'
  },
  BrowserWindow: { getFocusedWindow: () => null },
  dialog: {
    showMessageBox: vi.fn(async (opts: { message: string }) => {
      env.messages.push(opts.message)
      return { response: env.responses.shift() ?? 1 }
    })
  }
}))

const updater = vi.hoisted(() => ({
  autoDownload: true,
  autoInstallOnAppQuit: false,
  on: vi.fn(),
  checkForUpdates: vi.fn(),
  downloadUpdate: vi.fn(async () => []),
  quitAndInstall: vi.fn()
}))
vi.mock('electron-updater', () => ({ autoUpdater: updater }))

// Message key, followed by its parameters: enough to tell the messages apart.
vi.mock('../app/i18n', () => ({ t: (key: string, params?: object) => (params ? `${key} ${JSON.stringify(params)}` : key) }))

import { REPO_URL, Updates } from './updates'
import type { Browser } from '../browser/browser'

const realPlatform = process.platform
function setPlatform(platform: NodeJS.Platform, appImage?: string): void {
  Object.defineProperty(process, 'platform', { value: platform })
  if (appImage) process.env.APPIMAGE = appImage
  else delete process.env.APPIMAGE
}

function fakeBrowser(settings: { checkUpdates?: boolean } = {}) {
  const open = vi.fn()
  const show = vi.fn()
  const b = { store: { state: { settings } }, win: null, window: { show }, tabs: { open } } as unknown as Browser
  return { b, open, show }
}

const available = (version: string) => updater.checkForUpdates.mockResolvedValue({ isUpdateAvailable: true, updateInfo: { version } })
const upToDate = () => updater.checkForUpdates.mockResolvedValue({ isUpdateAvailable: false, updateInfo: { version: '1.0.0' } })
/** Lets the promises started without await (restart after a download) finish. */
const flush = () => new Promise((r) => setImmediate(r))

beforeEach(() => {
  env.packaged = true
  env.responses = []
  env.messages = []
  vi.clearAllMocks()
  setPlatform('darwin')
})
afterEach(() => {
  vi.useRealTimers()
  setPlatform(realPlatform)
})

describe('Updates: manual check', () => {
  it('up to date: says so', async () => {
    upToDate()
    await new Updates(fakeBrowser().b).check(true)
    expect(env.messages).toEqual(['updates.upToDate'])
  })

  it('run from source: never asks electron-updater', async () => {
    env.packaged = false
    await new Updates(fakeBrowser().b).check(true)
    expect(updater.checkForUpdates).not.toHaveBeenCalled()
    expect(env.messages).toEqual(['updates.devOnly'])
  })

  it('error: shown when asked, silent for an automatic check', async () => {
    updater.checkForUpdates.mockRejectedValue(new Error('404'))
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    const updates = new Updates(fakeBrowser().b)
    await updates.check(false)
    expect(env.messages).toEqual([])
    await updates.check(true)
    expect(env.messages).toEqual(['updates.failed'])
    warn.mockRestore()
  })
})

describe('Updates: macOS (no install)', () => {
  it('offers the release page, opened in a new tab', async () => {
    available('1.1.0')
    env.responses = [0]
    const { b, open, show } = fakeBrowser()
    await new Updates(b).check(false)
    expect(env.messages[0]).toContain('updates.available')
    expect(updater.downloadUpdate).not.toHaveBeenCalled()
    expect(show).toHaveBeenCalled()
    expect(open).toHaveBeenCalledWith(`${REPO_URL}/releases/tag/v1.1.0`, { activate: true })
  })

  it('automatic checks offer the same version only once per session', async () => {
    available('1.1.0')
    const updates = new Updates(fakeBrowser().b)
    await updates.check(false)
    await updates.check(false)
    expect(env.messages).toHaveLength(1)
    // Asked by the user: offered again.
    await updates.check(true)
    expect(env.messages).toHaveLength(2)
  })
})

describe('Updates: Windows and Linux AppImage (install)', () => {
  it('Windows: downloads, then restarts to install', async () => {
    setPlatform('win32')
    available('1.1.0')
    env.responses = [0]
    await new Updates(fakeBrowser().b).check(false)
    expect(updater.downloadUpdate).toHaveBeenCalledOnce()
    expect(env.messages[0]).toContain('updates.ready')
    await flush()
    expect(updater.quitAndInstall).toHaveBeenCalledWith(true, true)
  })

  it('"Later": no restart, and a new check offers the restart again', async () => {
    setPlatform('linux', '/tmp/Signet.AppImage')
    available('1.1.0')
    const updates = new Updates(fakeBrowser().b)
    await updates.check(false)
    await flush()
    expect(updater.quitAndInstall).not.toHaveBeenCalled()
    await updates.check(true)
    expect(updater.checkForUpdates).toHaveBeenCalledOnce()
    expect(env.messages.filter((m) => m.startsWith('updates.ready'))).toHaveLength(2)
  })

  it('Linux outside an AppImage: release page only', async () => {
    setPlatform('linux')
    available('1.1.0')
    await new Updates(fakeBrowser().b).check(false)
    expect(updater.downloadUpdate).not.toHaveBeenCalled()
    expect(env.messages[0]).toContain('updates.available')
  })
})

describe('Updates: automatic checks', () => {
  it('first check 30 s after launch, then every 6 hours', async () => {
    vi.useFakeTimers()
    upToDate()
    new Updates(fakeBrowser().b).sync()
    await vi.advanceTimersByTimeAsync(29_000)
    expect(updater.checkForUpdates).not.toHaveBeenCalled()
    await vi.advanceTimersByTimeAsync(1_000)
    expect(updater.checkForUpdates).toHaveBeenCalledOnce()
    await vi.advanceTimersByTimeAsync(6 * 60 * 60_000)
    expect(updater.checkForUpdates).toHaveBeenCalledTimes(2)
  })

  it('turned off in Settings, or run from source: no check', async () => {
    vi.useFakeTimers()
    new Updates(fakeBrowser({ checkUpdates: false }).b).sync()
    env.packaged = false
    new Updates(fakeBrowser().b).sync()
    await vi.advanceTimersByTimeAsync(7 * 60 * 60_000)
    expect(updater.checkForUpdates).not.toHaveBeenCalled()
  })

  it('turning the setting off stops the planned checks', async () => {
    vi.useFakeTimers()
    const settings: { checkUpdates?: boolean } = {}
    const updates = new Updates(fakeBrowser(settings).b)
    updates.sync()
    settings.checkUpdates = false
    updates.sync()
    await vi.advanceTimersByTimeAsync(60_000)
    expect(updater.checkForUpdates).not.toHaveBeenCalled()
  })
})

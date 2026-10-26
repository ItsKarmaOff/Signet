import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { QuitHudState } from '@shared/types'

const sent: QuitHudState[] = []
const quit = vi.fn()
const handlers = new Map<string, (...a: unknown[]) => void>()

vi.mock('electron', () => {
  class BrowserWindow {
    static getAllWindows = () => []
    webContents = {
      on: (e: string, h: () => void) => e === 'did-finish-load' && h(),
      isDestroyed: () => false,
      send: (_c: string, s: QuitHudState) => sent.push(s)
    }
    isDestroyed = () => false
    isVisible = () => true
    isFocused = () => false
    getOpacity = () => 1
    setOpacity = vi.fn()
    setPosition = vi.fn()
    setAlwaysOnTop = vi.fn()
    setVisibleOnAllWorkspaces = vi.fn()
    setIgnoreMouseEvents = vi.fn()
    showInactive = vi.fn()
    hide = vi.fn()
    destroy = vi.fn()
    on = vi.fn()
  }
  return {
    app: {
      on: (e: string, h: (...a: unknown[]) => void) => handlers.set(e, h),
      quit: () => quit(),
      isReady: () => true,
      whenReady: () => new Promise(() => {})
    },
    BrowserWindow,
    globalShortcut: { register: vi.fn(), unregister: vi.fn() },
    screen: { getPrimaryDisplay: () => ({ workArea: { x: 0, y: 0, width: 1000, height: 800 } }) }
  }
})
vi.mock('../web/guard', () => ({ denyWindowOpen: vi.fn() }))
vi.mock('../core/urls', () => ({ loadRoute: vi.fn(), preloadPath: '' }))

type Key = { type: string; key: string; meta?: boolean; control?: boolean; isAutoRepeat?: boolean }

async function setup(platform: 'darwin' | 'linux') {
  vi.resetModules()
  Object.defineProperty(process, 'platform', { value: platform })
  const mod = await import('./quit')
  mod.initQuit(() => true)
  const wcCreated = handlers.get('web-contents-created')!
  let listener!: (e: unknown, i: Key) => void
  wcCreated({}, { on: (_n: string, l: typeof listener) => (listener = l) })
  const mac = platform === 'darwin'
  const key = (type: string, k: string, extra: Partial<Key> = {}) =>
    listener({ preventDefault: () => {} }, { type, key: k, meta: mac, control: !mac, ...extra })
  return {
    down: (repeat = false) => key('keyDown', 'q', { isAutoRepeat: repeat }),
    up: (k = 'q') => key('keyUp', k, { meta: false, control: false }),
    modUp: () => key('keyUp', mac ? 'Meta' : 'Control', { meta: false, control: false })
  }
}

const holding = () => sent.at(-1)?.holding
const realPlatform = process.platform

beforeEach(() => {
  vi.useFakeTimers()
  sent.length = 0
  quit.mockClear()
})
afterEach(() => {
  vi.useRealTimers()
  Object.defineProperty(process, 'platform', { value: realPlatform })
})

describe('hold to quit: macOS release inferred from the key repeat', () => {
  it('release before the first repeat stops within the fallback delay + margin', async () => {
    const k = await setup('darwin')
    k.down()
    expect(holding()).toBe(true)
    vi.advanceTimersByTime(400)
    expect(holding()).toBe(true)
    vi.advanceTimersByTime(80 + 40)
    expect(holding()).toBe(false)
    expect(quit).not.toHaveBeenCalled()
  })

  it('the measured first repeat delay is used for the next press', async () => {
    const k = await setup('darwin')
    k.down()
    vi.advanceTimersByTime(250)
    k.down(true)
    k.up()
    expect(holding()).toBe(false)
    // New press: released ~250 + margin after, well before the 400 ms fallback + margin.
    vi.advanceTimersByTime(3000)
    k.down()
    expect(holding()).toBe(true)
    vi.advanceTimersByTime(250 + 80 + 40)
    expect(holding()).toBe(false)
  })

  it('a first repeat later than guessed resumes the press and is learned', async () => {
    const k = await setup('darwin')
    k.down()
    vi.advanceTimersByTime(600)
    expect(holding()).toBe(false)
    k.down(true)
    expect(holding()).toBe(true)
    expect(sent.at(-1)?.repeating).toBe(true)
    for (let i = 0; i < 40; i++) {
      vi.advanceTimersByTime(60)
      k.down(true)
    }
    expect(quit).toHaveBeenCalledTimes(1)
  })

  it('a long first repeat resumes without quitting at once: the hold is sized for the real delay', async () => {
    const k = await setup('darwin')
    k.down()
    vi.advanceTimersByTime(1200)
    expect(holding()).toBe(false)
    k.down(true)
    // Hold re-sized from the measured 1200 ms delay (1200 + 600 = 1800 ms from the press).
    vi.advanceTimersByTime(60)
    k.down(true)
    expect(quit).not.toHaveBeenCalled()
    expect(holding()).toBe(true)
    for (let i = 0; i < 14; i++) {
      vi.advanceTimersByTime(60)
      k.down(true)
    }
    expect(quit).toHaveBeenCalledTimes(1)
  })

  it('a tap sends a hold sized after the expected repeat, with no repeat flag', async () => {
    const k = await setup('darwin')
    k.down()
    expect(sent.at(-1)).toMatchObject({ holding: true, wait: 400, duration: 1000 })
    expect(sent.at(-1)?.repeating).toBeUndefined()
    vi.advanceTimersByTime(300)
    k.up()
    expect(quit).not.toHaveBeenCalled()
  })

  it('does not quit before the new threshold, quits after it', async () => {
    const k = await setup('darwin')
    k.down()
    vi.advanceTimersByTime(400)
    for (let i = 0; i < 9; i++) {
      k.down(true)
      vi.advanceTimersByTime(60)
    }
    expect(quit).not.toHaveBeenCalled()
    for (let i = 0; i < 3; i++) {
      k.down(true)
      vi.advanceTimersByTime(60)
    }
    expect(quit).toHaveBeenCalledTimes(1)
  })

  it('holding with repeats still quits at the threshold', async () => {
    const k = await setup('darwin')
    k.down()
    vi.advanceTimersByTime(400)
    for (let i = 0; i < 30; i++) {
      k.down(true)
      vi.advanceTimersByTime(50)
      expect(holding()).toBe(true)
    }
    expect(quit).toHaveBeenCalledTimes(1)
  })

  it('a quick tap keeps the bubble for the second press, which quits', async () => {
    const k = await setup('darwin')
    k.down()
    vi.advanceTimersByTime(600)
    expect(holding()).toBe(false)
    k.down()
    expect(quit).toHaveBeenCalledTimes(1)
  })

  it('the release of the modifier stops immediately', async () => {
    const k = await setup('darwin')
    k.down()
    vi.advanceTimersByTime(100)
    k.modUp()
    expect(holding()).toBe(false)
  })
})

describe('hold to quit: Windows and Linux', () => {
  it('keyUp of Q stops immediately', async () => {
    const k = await setup('linux')
    k.down()
    vi.advanceTimersByTime(100)
    expect(holding()).toBe(true)
    k.up('q')
    expect(holding()).toBe(false)
  })

  it('release of Ctrl stops immediately', async () => {
    const k = await setup('linux')
    k.down()
    k.modUp()
    expect(holding()).toBe(false)
  })
})

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { EventEmitter } from 'node:events'

const focusedWin = {}
vi.mock('electron', () => ({
  BrowserWindow: class {
    static getFocusedWindow = () => focusedWin
  }
}))

import { SiteColor, dominantColor } from './sitecolor'

/** BGRA row from [r, g, b, a] pixels. */
const row = (...px: [number, number, number, number][]) => Buffer.from(px.flatMap(([r, g, b, a]) => [b, g, r, a]))

describe('dominantColor', () => {
  it('couleur unie', () => {
    expect(dominantColor(row([10, 20, 30, 255], [10, 20, 30, 255]), 2)).toBe('#0a141e')
  })
  it('the most frequent color wins', () => {
    expect(dominantColor(row([255, 0, 0, 255], [0, 0, 255, 255], [0, 0, 255, 255]), 3)).toBe('#0000ff')
  })
  it('ignores transparent pixels', () => {
    expect(dominantColor(row([255, 0, 0, 0], [0, 255, 0, 255]), 2)).toBe('#00ff00')
    expect(dominantColor(row([255, 0, 0, 0]), 1)).toBeNull()
  })
  it('averages close shades in the same bucket', () => {
    expect(dominantColor(row([0x10, 0x10, 0x10, 255], [0x1e, 0x1e, 0x1e, 255]), 2)).toBe('#171717')
  })
  it('empty buffer or shorter than the width', () => {
    expect(dominantColor(Buffer.alloc(0), 10)).toBeNull()
    expect(dominantColor(row([1, 2, 3, 255]), 10)).toBe('#010203')
  })
})

/** SiteColor wired to a fake browser: one displayed tab whose top color is `paint.color`. */
function setup() {
  const paint = { color: [10, 20, 30] as [number, number, number] }
  const wc = Object.assign(new EventEmitter(), {
    isDestroyed: () => false,
    capturePage: vi.fn(async () => ({
      isEmpty: () => false,
      toBitmap: () => row([...paint.color, 255]),
      getSize: () => ({ width: 1 })
    }))
  })
  const tab: { themeColor?: string } = {}
  const patchTab = vi.fn((_id: string, p: { themeColor: string }) => Object.assign(tab, p))
  const view = { webContents: wc, getBounds: () => ({ width: 100, height: 100 }) }
  const b = {
    win: focusedWin,
    store: { state: { tabs: { t: tab } }, detached: new Set<string>() },
    views: { viewOf: () => view },
    model: { displayedTabIds: () => ['t'] },
    broadcast: { patchTab },
    little: { tabIdOf: () => undefined },
    webApps: { tabIdOf: () => undefined }
  }
  Object.assign(focusedWin, { isDestroyed: () => false, isVisible: () => true, isMinimized: () => false })
  const sc = new SiteColor(b as never)
  sc.attach('t', wc as never)
  sc.onShown(['t'])
  return { sc, wc, paint, tab, patchTab }
}
const ms = (n: number) => vi.advanceTimersByTimeAsync(n)
const click = (wc: EventEmitter) => wc.emit('before-mouse-event', {}, { type: 'mouseUp' })

describe('SiteColor', () => {
  beforeEach(() => vi.useFakeTimers())
  afterEach(() => vi.useRealTimers())

  it('a navigation bursts and broadcasts the color', async () => {
    const { sc, wc, tab } = setup()
    wc.emit('did-navigate')
    await ms(100)
    expect(tab.themeColor).toBe('#0a141e')
    sc.dispose()
  })

  it('a click in slow mode brings back fast samples and the new color shows quickly', async () => {
    const { sc, wc, paint, tab } = setup()
    await ms(60_000) // stable: slow mode
    paint.color = [200, 100, 50]
    click(wc)
    await ms(400)
    expect(tab.themeColor).toBe('#c86432')
    sc.dispose()
  })

  it('typing does not start a burst per key', async () => {
    const { sc, wc } = setup()
    await ms(60_000)
    wc.capturePage.mockClear()
    for (let i = 0; i < 10; i++) wc.emit('before-input-event', {}, { type: 'keyDown' })
    await ms(100)
    expect(wc.capturePage).toHaveBeenCalledTimes(1)
    sc.dispose()
  })

  it('a color change in slow mode without interaction shows within the slow interval', async () => {
    const { sc, paint, tab } = setup()
    await ms(60_000)
    paint.color = [200, 100, 50]
    await ms(6500)
    expect(tab.themeColor).toBe('#c86432')
    sc.dispose()
  })

  it('an animated header ends up sampled slowly', async () => {
    const { sc, wc, paint } = setup()
    let i = 0
    wc.capturePage.mockImplementation(async () => {
      paint.color = [(i++ * 16) % 256, 0, 0]
      return { isEmpty: () => false, toBitmap: () => row([...paint.color, 255]), getSize: () => ({ width: 1 }) }
    })
    await ms(120_000)
    wc.capturePage.mockClear()
    await ms(60_000)
    expect(wc.capturePage.mock.calls.length).toBeLessThanOrEqual(13)
    sc.dispose()
  })

  it('a destroyed tab leaves no pending timer', async () => {
    const { sc, wc } = setup()
    wc.emit('did-navigate')
    wc.emit('destroyed')
    sc.dispose()
    expect(vi.getTimerCount()).toBe(1) // the onShown timeout, harmless
    await ms(1000)
    expect(vi.getTimerCount()).toBe(0)
  })
})

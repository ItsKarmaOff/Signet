import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// previewHover keeps module state (timers, "hot", blocked): we reload it on each test.
type Mod = typeof import('./previewHover')
let mod: Mod
let signet: { showTabPreview: ReturnType<typeof vi.fn>; hideTabPreview: ReturnType<typeof vi.fn> }
let listeners: Record<string, () => void>

beforeEach(async () => {
  vi.useFakeTimers()
  vi.setSystemTime(0)
  signet = { showTabPreview: vi.fn(), hideTabPreview: vi.fn() }
  listeners = {}
  vi.stubGlobal('window', {
    signet,
    addEventListener: (name: string, fn: () => void) => (listeners[name] = fn)
  })
  vi.resetModules()
  mod = await import('./previewHover')
})
afterEach(() => {
  vi.useRealTimers()
  vi.unstubAllGlobals()
})

function el(opts: { chip?: boolean; input?: boolean; connected?: boolean } = {}) {
  return {
    isConnected: opts.connected ?? true,
    querySelector: (sel: string) => (opts.input && sel === 'input' ? {} : null),
    getBoundingClientRect: () => ({ left: 10, top: 20, width: 100, height: 30 }),
    classList: { contains: (c: string) => c === 'tchip' && !!opts.chip }
  }
}
const enter = (h: ReturnType<Mod['previewHandlers']>, target: ReturnType<typeof el>, buttons = 0) => h.onMouseEnter({ buttons, currentTarget: target } as never)

describe('previewHandlers', () => {
  it('shows the card after 600 ms of stillness, with the row rectangle', () => {
    const h = mod.previewHandlers('t1')
    enter(h, el())
    vi.advanceTimersByTime(599)
    expect(signet.showTabPreview).not.toHaveBeenCalled()
    vi.advanceTimersByTime(1)
    expect(signet.showTabPreview).toHaveBeenCalledWith('t1', { x: 10, y: 20, width: 100, height: 30 }, false)
  })

  it('flags a chip of the top row (tchip class)', () => {
    const h = mod.previewHandlers('t1')
    enter(h, el({ chip: true }))
    vi.advanceTimersByTime(600)
    expect(signet.showTabPreview.mock.calls[0][2]).toBe(true)
  })

  it('does nothing if a button is pressed', () => {
    const h = mod.previewHandlers('t1')
    enter(h, el(), 1)
    vi.advanceTimersByTime(2000)
    expect(signet.showTabPreview).not.toHaveBeenCalled()
  })

  it('shows nothing during a rename or if the row is no longer in the page', () => {
    const h = mod.previewHandlers('t1')
    enter(h, el({ input: true }))
    vi.advanceTimersByTime(700)
    enter(h, el({ connected: false }))
    vi.advanceTimersByTime(700)
    expect(signet.showTabPreview).not.toHaveBeenCalled()
  })

  it('leaving before the delay cancels the display', () => {
    const h = mod.previewHandlers('t1')
    enter(h, el())
    vi.advanceTimersByTime(300)
    h.onMouseLeave()
    vi.advanceTimersByTime(2000)
    expect(signet.showTabPreview).not.toHaveBeenCalled()
  })

  it('a displayed card moves to the next tab without delay', () => {
    const a = mod.previewHandlers('a')
    const b = mod.previewHandlers('b')
    enter(a, el())
    vi.advanceTimersByTime(600)
    a.onMouseLeave()
    vi.advanceTimersByTime(100) // less than the grace (140 ms)
    enter(b, el())
    expect(signet.showTabPreview).toHaveBeenCalledTimes(2)
    expect(signet.showTabPreview.mock.calls[1][0]).toBe('b')
    expect(signet.hideTabPreview).not.toHaveBeenCalled()
  })

  it('removes the card after the 140 ms grace', () => {
    const h = mod.previewHandlers('t1')
    enter(h, el())
    vi.advanceTimersByTime(600)
    h.onMouseLeave()
    vi.advanceTimersByTime(139)
    expect(signet.hideTabPreview).not.toHaveBeenCalled()
    vi.advanceTimersByTime(1)
    expect(signet.hideTabPreview).toHaveBeenCalledTimes(1)
  })

  it('after removal, the next hover within 500 ms is immediate, then the delay returns', () => {
    const h = mod.previewHandlers('t1')
    enter(h, el())
    vi.advanceTimersByTime(600)
    h.onMouseLeave()
    vi.advanceTimersByTime(140) // removal, hot window until +500 ms
    vi.advanceTimersByTime(300)
    enter(h, el())
    expect(signet.showTabPreview).toHaveBeenCalledTimes(2)
    h.onMouseLeave()
    vi.advanceTimersByTime(140 + 600) // hot window expired, card removed
    signet.showTabPreview.mockClear()
    vi.advanceTimersByTime(1000)
    enter(h, el())
    expect(signet.showTabPreview).not.toHaveBeenCalled()
    vi.advanceTimersByTime(600)
    expect(signet.showTabPreview).toHaveBeenCalledTimes(1)
  })

  it('a click (pointerdown) hides the card and blocks until the next hover', () => {
    const h = mod.previewHandlers('t1')
    enter(h, el())
    vi.advanceTimersByTime(600)
    h.onPointerDown()
    expect(signet.hideTabPreview).toHaveBeenCalledTimes(1)
    // The timer of a hover already started is blocked by the click.
    h.onMouseLeave()
    vi.advanceTimersByTime(1000)
    enter(h, el()) // new hover: unblocks
    vi.advanceTimersByTime(600)
    expect(signet.showTabPreview).toHaveBeenCalledTimes(2)
  })

  it('a drag blocks the display of an already armed timer', () => {
    const h = mod.previewHandlers('t1')
    enter(h, el())
    h.onDragStartCapture()
    vi.advanceTimersByTime(2000)
    expect(signet.showTabPreview).not.toHaveBeenCalled()
  })
})

describe('hidePreview', () => {
  it('only calls the main process if a card is displayed', () => {
    mod.hidePreview()
    expect(signet.hideTabPreview).not.toHaveBeenCalled()
    const h = mod.previewHandlers('t1')
    enter(h, el())
    vi.advanceTimersByTime(600)
    mod.hidePreview()
    expect(signet.hideTabPreview).toHaveBeenCalledTimes(1)
    mod.hidePreview()
    expect(signet.hideTabPreview).toHaveBeenCalledTimes(1)
  })

  it("is wired to the window's blur and wheel", () => {
    expect(listeners.blur).toBe(mod.hidePreview)
    expect(listeners.wheel).toBe(mod.hidePreview)
  })
})

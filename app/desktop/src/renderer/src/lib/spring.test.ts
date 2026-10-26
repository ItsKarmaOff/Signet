import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'

// spring.ts reads `window.matchMedia` at import: we mock `window` before loading it.
let mod: typeof import('./spring')
beforeAll(async () => {
  vi.stubGlobal('window', { matchMedia: () => ({ matches: false }) })
  mod = await import('./spring')
})

describe('rubberband / unrubberband', () => {
  it('is 0 without overshoot or without dimension', () => {
    expect(mod.rubberband(0, 100)).toBe(0)
    expect(mod.rubberband(50, 0)).toBe(0)
    expect(mod.rubberband(50, -10)).toBe(0)
  })

  it('follows the formula d·dim·c / (dim + c·d), keeping the sign', () => {
    expect(mod.rubberband(100, 400)).toBeCloseTo((100 * 400 * 0.55) / (400 + 0.55 * 100), 10)
    expect(mod.rubberband(-100, 400)).toBeCloseTo(-mod.rubberband(100, 400), 10)
  })

  it('is increasing, below the dimension and below the raw overshoot', () => {
    let prev = 0
    for (const d of [10, 50, 200, 1000, 100000]) {
      const r = mod.rubberband(d, 300)
      expect(r).toBeGreaterThan(prev)
      expect(r).toBeLessThan(300)
      expect(r).toBeLessThan(d)
      prev = r
    }
  })

  it('unrubberband is its inverse', () => {
    for (const d of [-250, -10, 1, 40, 300, 900]) {
      expect(mod.unrubberband(mod.rubberband(d, 500), 500)).toBeCloseTo(d, 6)
    }
  })

  it('unrubberband bounds the input just under the dimension (no infinity)', () => {
    expect(Number.isFinite(mod.unrubberband(10_000, 100))).toBe(true)
    expect(mod.unrubberband(0, 100)).toBe(0)
  })
})

describe('project', () => {
  it('projects a fling like native scrolling', () => {
    expect(mod.project(1000)).toBeCloseTo(499, 6) // 1 × 0,998 / 0,002
    expect(mod.project(-1000)).toBeCloseTo(-499, 6)
    expect(mod.project(0)).toBe(0)
  })

  it('un taux plus faible freine plus vite', () => {
    expect(mod.project(1000, 0.99)).toBeLessThan(mod.project(1000))
  })
})

describe('spring', () => {
  let now = 0
  let queue: ((t: number) => void)[] = []
  beforeEach(() => {
    now = 0
    queue = []
    vi.spyOn(performance, 'now').mockImplementation(() => now)
    vi.stubGlobal('requestAnimationFrame', (cb: (t: number) => void) => queue.push(cb))
    vi.stubGlobal('cancelAnimationFrame', () => {
      queue = []
    })
  })
  afterEach(() => {
    vi.restoreAllMocks()
    vi.unstubAllGlobals()
    vi.stubGlobal('window', { matchMedia: () => ({ matches: false }) })
  })

  /** Advances frame by frame (16 ms) until the end or `max` frames. */
  function run(max = 600) {
    let frames = 0
    while (queue.length && frames < max) {
      const cb = queue.shift()!
      now += 16
      cb(now)
      frames++
    }
    return frames
  }

  function play(opts: { from: number; to: number; velocity?: number; response?: number; damping?: number }) {
    const values: number[] = []
    const vels: number[] = []
    const onDone = vi.fn()
    const stop = mod.spring({
      ...opts,
      onUpdate: (v, vel) => {
        values.push(v)
        vels.push(vel)
      },
      onDone
    })
    return { values, vels, onDone, stop }
  }

  it('critical: goes from from to to without overshooting and ends exactly on to', () => {
    const s = play({ from: 0, to: 100 })
    run()
    expect(s.onDone).toHaveBeenCalledTimes(1)
    expect(s.values.at(-1)).toBe(100)
    expect(s.vels.at(-1)).toBe(0)
    for (let i = 1; i < s.values.length; i++) expect(s.values[i]).toBeGreaterThanOrEqual(s.values[i - 1])
    expect(Math.max(...s.values)).toBeLessThanOrEqual(100)
  })

  it('critical: also works downward', () => {
    const s = play({ from: 100, to: 0 })
    run()
    expect(s.values.at(-1)).toBe(0)
    expect(Math.min(...s.values)).toBeGreaterThanOrEqual(0)
  })

  /** Plays a lone spring from a clean state: emitted values and number of frames. */
  function alone(opts: { from: number; to: number; velocity?: number; response?: number }) {
    queue = []
    now = 0
    const s = play(opts)
    const frames = run()
    return { values: s.values, frames }
  }

  it('a shorter period finishes in fewer frames', () => {
    expect(alone({ from: 0, to: 100, response: 0.2 }).frames).toBeLessThan(alone({ from: 0, to: 100, response: 0.8 }).frames)
  })

  it('an initial velocity toward `to` gets there faster', () => {
    const calm = alone({ from: 0, to: 100 })
    const flick = alone({ from: 0, to: 100, velocity: 3000 })
    expect(flick.values[2]).toBeGreaterThan(calm.values[2])
  })

  it('underdamped: overshoots to then settles on it', () => {
    const s = play({ from: 0, to: 100, damping: 0.6, velocity: 2000 })
    run()
    expect(Math.max(...s.values)).toBeGreaterThan(100)
    expect(s.values.at(-1)).toBe(100)
    expect(s.onDone).toHaveBeenCalledTimes(1)
  })

  it('stop interrompt l’animation sans appeler onDone', () => {
    const s = play({ from: 0, to: 100 })
    run(2)
    const n = s.values.length
    s.stop()
    run()
    expect(s.values.length).toBe(n)
    expect(s.onDone).not.toHaveBeenCalled()
  })

  it('from == to with no velocity: finishes on the first frame', () => {
    const s = play({ from: 50, to: 50 })
    run()
    expect(s.values).toEqual([50])
    expect(s.onDone).toHaveBeenCalledTimes(1)
  })
})

describe('reducedMotion', () => {
  it('is the "prefers-reduced-motion" media query', () => {
    expect(mod.reducedMotion.matches).toBe(false)
  })
})

/** Critical spring (no bounce), solved analytically: exact regardless of the time between two frames. */

/** Rubber-band resistance beyond a limit: `d` = overshoot, `dim` = reference dimension. */
export function rubberband(d: number, dim: number, c = 0.55): number {
  if (d === 0 || dim <= 0) return 0
  const sign = Math.sign(d)
  const a = Math.abs(d)
  return (sign * (a * dim * c)) / (dim + c * a)
}

/** Inverse of `rubberband`: the raw overshoot that produces this displayed overshoot. */
export function unrubberband(r: number, dim: number, c = 0.55): number {
  const sign = Math.sign(r)
  const a = Math.min(Math.abs(r), dim * 0.999)
  return (sign * (a * dim)) / (c * (dim - a))
}

/** Distance a fling (px/s) would travel while decelerating like native scrolling. */
export function project(velocity: number, rate = 0.998): number {
  return (velocity / 1000) * (rate / (1 - rate))
}

export interface SpringOptions {
  from: number
  to: number
  /** px/s */
  velocity?: number
  /** Period of the undamped spring, in seconds. */
  response?: number
  /** Damping ratio: 1 = critical (default, no bounce), < 1 = slight bounce (reserved for momentum flings). */
  damping?: number
  onUpdate: (value: number, velocity: number) => void
  onDone: () => void
}

/** Starts the animation; the returned function interrupts it (the last value sent stays valid). */
export function spring({ from, to, velocity = 0, response = 0.35, damping = 1, onUpdate, onDone }: SpringOptions): () => void {
  const omega = (2 * Math.PI) / response
  const x0 = from - to
  const c2 = velocity + omega * x0
  const under = damping < 1
  const decayRate = damping * omega
  const wd = omega * Math.sqrt(Math.max(1e-6, 1 - damping * damping))
  const sb = (velocity + decayRate * x0) / wd
  const start = performance.now()
  let frame = 0
  let stopped = false

  const tick = (now: number) => {
    if (stopped) return
    const t = (now - start) / 1000
    let x: number
    let v: number
    if (under) {
      // Sous-amorti : solution analytique x = e^(-ζωt)(x0·cos + sb·sin).
      const decay = Math.exp(-decayRate * t)
      const cos = Math.cos(wd * t)
      const sin = Math.sin(wd * t)
      x = decay * (x0 * cos + sb * sin)
      v = decay * (-decayRate * (x0 * cos + sb * sin) + wd * (sb * cos - x0 * sin))
    } else {
      const decay = Math.exp(-omega * t)
      x = (x0 + c2 * t) * decay
      v = (c2 - omega * (x0 + c2 * t)) * decay
    }
    if (Math.abs(x) < 0.3 && Math.abs(v) < 8) {
      onUpdate(to, 0)
      onDone()
      return
    }
    onUpdate(to + x, v)
    frame = requestAnimationFrame(tick)
  }
  frame = requestAnimationFrame(tick)
  return () => {
    stopped = true
    cancelAnimationFrame(frame)
  }
}

/** "Reduce animations" preference: a single, live query (`.matches` stays up to date), not to be recreated in hot paths. */
export const reducedMotion = window.matchMedia('(prefers-reduced-motion: reduce)')

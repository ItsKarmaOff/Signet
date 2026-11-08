// Critical spring (no bounce), solved analytically: exact regardless of the time between two frames.
// Classic script: exposes `SignetSpring` (and module.exports for tests).
;(function (root) {
  'use strict'

  /** Rubber-band resistance beyond a limit: `d` = overshoot, `dim` = reference dimension. */
  function rubberband(d, dim, c = 0.55) {
    if (d === 0 || dim <= 0) return 0
    const sign = Math.sign(d)
    const a = Math.abs(d)
    return (sign * (a * dim * c)) / (dim + c * a)
  }

  /** Distance a fling (px/s) would travel while decelerating like native scrolling. */
  function project(velocity, rate = 0.998) {
    return (velocity / 1000) * (rate / (1 - rate))
  }

  /** Starts the animation; the returned function interrupts it (the last value sent stays valid). */
  function spring({ from, to, velocity = 0, response = 0.35, damping = 1, onUpdate, onDone }) {
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

    const tick = (now) => {
      if (stopped) return
      const t = (now - start) / 1000
      let x
      let v
      if (under) {
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

  const api = { rubberband, project, spring }
  root.SignetSpring = api
  if (typeof module !== 'undefined' && module.exports) module.exports = api
})(typeof window !== 'undefined' ? window : globalThis)

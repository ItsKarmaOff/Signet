import { systemPreferences } from 'electron'
import type { Rect } from '@shared/types'

/** Start (entry) and end (exit) scale of the Peek's view; the overlay card does the same on the CSS side. */
export const PEEK_SCALE = 0.95
/** Start scale when the Peek comes from a link: the card springs from the clicked point. */
export const PEEK_ORIGIN_SCALE = 0.25
const FRAME = 8

const reducedMotion = (): boolean => {
  try {
    return systemPreferences.getAnimationSettings().prefersReducedMotion
  } catch {
    return false
  }
}

/** Intermediate rectangle between `a` (q = 0) and `b` (q = 1). */
export function lerpRect(a: Rect, b: Rect, q: number): Rect {
  const l = (x: number, y: number): number => Math.round(x + (y - x) * q)
  return { x: l(a.x, b.x), y: l(a.y, b.y), width: l(a.width, b.width), height: l(a.height, b.height) }
}

/** `r` shrunk around its center, or around `origin` (same point as the card's CSS transform-origin). */
export function scaledRect(r: Rect, scale: number, origin?: { x: number; y: number } | null): Rect {
  if (scale === 1) return r
  if (origin) {
    return {
      x: Math.round(origin.x + (r.x - origin.x) * scale),
      y: Math.round(origin.y + (r.y - origin.y) * scale),
      width: Math.max(1, Math.round(r.width * scale)),
      height: Math.max(1, Math.round(r.height * scale))
    }
  }
  const width = Math.round(r.width * scale)
  const height = Math.round(r.height * scale)
  return { x: r.x + Math.round((r.width - width) / 2), y: r.y + Math.round((r.height - height) / 2), width, height }
}

/** Animated scale of the Peek's view: critically damped spring (same analytical solution as Pip.glide), no momentum. */
export class PeekMotion {
  scale = 1
  private timer: NodeJS.Timeout | null = null

  /** Sets the scale at once (no animation). */
  set(scale: number): void {
    this.stop()
    this.scale = scale
  }

  /** Goes toward `target` in about `response` seconds; `apply` sets the bounds on each frame. Reduced motion: at once. */
  glide(target: number, response: number, apply: () => void, done?: () => void): void {
    this.stop()
    if (reducedMotion()) {
      this.scale = target
      apply()
      return done?.()
    }
    const from = this.scale
    const omega = (2 * Math.PI) / response
    const start = performance.now()
    this.timer = setInterval(() => {
      const t = (performance.now() - start) / 1000
      const x0 = from - target
      const x = (x0 + omega * x0 * t) * Math.exp(-omega * t)
      const end = Math.abs(x) < 0.002 || t > 1
      if (end) {
        this.stop()
        this.scale = target
      } else this.scale = target + x
      apply()
      if (end) done?.()
    }, FRAME)
  }

  stop(): void {
    if (this.timer) clearInterval(this.timer)
    this.timer = null
  }
}

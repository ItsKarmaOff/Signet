import type { Rect } from '@signet/shared/types'

// Sidebar slide (toggle): the bar moves out past its edge while the pages widen, and back.
// Main drives it: on each step it moves the native pages and sends the main UI the position for its
// bar and page chrome. A stalled step (macOS relaying the window out) holds both sides together.

/** Response (s) of the slide spring (critically damped: no bounce). */
export const SLIDE_RESPONSE = 0.32

/**
 * Lead (ms) of the page chrome over the native pages while they widen (closing). An HTML change
 * reaches the screen about a frame after a native move: ahead, the card (in the page's color) always
 * covers a bit more than the page. While they narrow (opening), the natural lag does the same.
 */
export const SLIDE_UI_LEAD_MS = 24

/** Horizontal span of the content area. */
export interface Span {
  x: number
  width: number
}

/** The spring of one slide segment: from `from` (with `velocity`, per second) at `start` (ms) toward `target`. */
export interface SlideMotion {
  start: number
  from: number
  velocity: number
  /** 1: shown, 0: hidden. */
  target: 0 | 1
}

/** What the main UI needs to play the slide (sent with the state while it lasts). */
export interface SlidePlan {
  /** Content area as laid out (sidebar hidden). */
  from: Span
  /** Sidebar width, and the gap kept when it is hidden. */
  width: number
  gap: number
  right: boolean
  motion: SlideMotion
}

/** Frame of the slide: the bar's shift (px) and the area left to the pages. */
export interface SidebarSlide {
  progress: number
  sidebar: number
  from: Span
  to: Span
}

/** Position (1 shown, 0 hidden) and speed of the slide at `now` (ms); `done` once settled. */
export function slideState(m: SlideMotion, now: number): { progress: number; velocity: number; done: boolean } {
  const omega = (2 * Math.PI) / SLIDE_RESPONSE
  const t = Math.max(0, (now - m.start) / 1000)
  const x0 = m.from - m.target
  const c = m.velocity + omega * x0
  const decay = Math.exp(-omega * t)
  const x = (x0 + c * t) * decay
  const v = (c - omega * (x0 + c * t)) * decay
  if (Math.abs(x) < 0.001 && Math.abs(v) < 0.01) return { progress: m.target, velocity: 0, done: true }
  return { progress: m.target + x, velocity: v, done: false }
}

/** Frame of the slide at progress `p`. */
export function slideAt(plan: SlidePlan, p: number): SidebarSlide {
  const { from, width, gap, right } = plan
  const taken = Math.round((width - gap) * p)
  const shift = Math.round((1 - p) * width)
  return {
    progress: p,
    sidebar: right ? shift : 0 - shift, // 0 - 0: no -0 at rest
    from,
    to: { x: right ? from.x : from.x + taken, width: from.width - taken }
  }
}

/** `r` (laid out in `from`) stretched into `to`; `keepWidth` only moves it (right edge followed). */
export function slideRect<R extends Rect>(r: R, s: SidebarSlide, keepWidth = false): R {
  const k = s.from.width > 0 ? s.to.width / s.from.width : 1
  const at = (x: number): number => Math.round(s.to.x + (x - s.from.x) * k)
  if (keepWidth) return { ...r, x: at(r.x + r.width) - r.width }
  const x = at(r.x)
  return { ...r, x, width: Math.max(0, at(r.x + r.width) - x) }
}

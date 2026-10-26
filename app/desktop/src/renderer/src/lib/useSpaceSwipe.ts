import { useEffect, useLayoutEffect, useRef } from 'react'
import type { RefObject } from 'react'
import { project, reducedMotion as reduced, rubberband, spring, unrubberband } from './spring'

/** Silence that marks the end of a gesture. */
const END_DELAY = 90
/** Silence that marks the end of the momentum of an already settled gesture. */
const MOMENTUM_END = 120
/** Window over which velocity is measured. */
const VELOCITY_WINDOW = 100
/** Horizontal movement (px) that triggers the switch when animations are reduced. */
const REDUCED_THRESHOLD = 80
/** Cumulative movement (px) from which the gesture axis is decided. */
const LOCK_AT = 8
/** Velocity (px/s) above which a fling bounces slightly. */
const FLICK = 1500

interface Options {
  /** Element that receives the `wheel` events. */
  rootRef: RefObject<HTMLElement | null>
  /** Element whose width is that of one page. */
  clipRef: RefObject<HTMLElement | null>
  /** Selector (inside clipRef) of the track that receives `--swipe-x`. */
  track: string
  /** Selector (inside rootRef) of the element that receives `--swipe-pos` (fractional gesture position), if any. */
  pos?: string
  /**
   * The track holds all the pages: a page change coming from elsewhere (not from the gesture) slides with a spring
   * from the displayed position, interruptible by a new gesture. Otherwise, the track recenters at once.
   */
  animateJumps?: boolean
  spaceIds: string[]
  activeId: string
  onSwitch: (spaceId: string) => void
}

const editable = (el: EventTarget | null) => el instanceof Element && !!el.closest('input, textarea, select, [contenteditable="true"]')
/** Events already taken by a nested swipe (e.g. the icon row in the sidebar): the parent ignores them. */
const handled = new WeakSet<Event>()

/**
 * Horizontal swipe between spaces on the trackpad: the track follows the gesture 1:1, rubber-banded at the first and last space,
 * then a critical spring (starting from the gesture velocity) brings it to the neighboring space or back.
 * Everything goes through CSS variables set directly on the DOM: no React render during the gesture.
 */
export function useSpaceSwipe({ rootRef, clipRef, track: trackSel, pos, animateJumps = false, spaceIds, activeId, onSwitch }: Options): void {
  const live = useRef({ spaceIds, activeId, onSwitch, animateJumps })
  live.current = { spaceIds, activeId, onSwitch, animateJumps }
  const reset = useRef<(from?: number) => void>(() => {})
  const last = useRef(activeId)

  // The active page changed: the track recenters in the same render, without a jolt.
  useLayoutEffect(() => {
    const from = live.current.spaceIds.indexOf(last.current)
    last.current = activeId
    reset.current(from)
  }, [activeId])

  useEffect(() => {
    const root = rootRef.current
    if (!root) return

    let raw = 0 // translation requested by the fingers, before resistance
    let shown = 0 // displayed translation
    let samples: { t: number; x: number }[] = []
    let endTimer = 0
    let fallback = 0
    let stop: (() => void) | null = null
    let locked = false // switch requested, waiting for the new state
    let triggered = false // reduced animations: already switched during this gesture
    let spent = false // switch settled mid-gesture: swallow the rest of the trackpad momentum
    let spentTimer = 0
    let spentHist: number[] = []
    // A gesture = events separated by less than END_DELAY. idle: nothing; pending: axis not decided; track: horizontal; ignore: vertical.
    let state: 'idle' | 'pending' | 'track' | 'ignore' = 'idle'
    let accX = 0
    let accY = 0

    const track = () => clipRef.current?.querySelector<HTMLElement>(trackSel) ?? null
    const dots = () => (pos ? root.querySelector<HTMLElement>(pos) : null)
    const width = () => clipRef.current?.clientWidth || root.clientWidth
    const index = () => live.current.spaceIds.indexOf(live.current.activeId)
    const hasPrev = () => index() > 0
    const hasNext = () => index() < live.current.spaceIds.length - 1

    /** Displayed translation for a raw translation: rubber-band resistance beyond the first and last space. */
    const toShown = (r: number) => {
      const w = width()
      const hi = hasPrev() ? w : 0
      const lo = hasNext() ? -w : 0
      if (r > hi) return hi + rubberband(r - hi, w)
      if (r < lo) return lo + rubberband(r - lo, w)
      return r
    }
    /** Inverse of `toShown`: the raw translation that gives this displayed position. */
    const toRaw = (x: number) => {
      const w = width()
      const hi = hasPrev() ? w : 0
      const lo = hasNext() ? -w : 0
      if (x > hi) return hi + unrubberband(x - hi, w)
      if (x < lo) return lo + unrubberband(x - lo, w)
      return x
    }
    let paintW = 0 // width memorized for the duration of the gesture (re-reading clientWidth every frame forces a measurement)
    const paint = (x: number) => {
      shown = x
      const w = paintW || (paintW = width() || 1)
      // Variables set as close as possible to what reads them (track and dots): no style recalculation across the whole bar.
      track()?.style.setProperty('--swipe-x', `${x}px`)
      dots()?.style.setProperty('--swipe-pos', String(index() - x / w))
    }
    const clear = () => {
      root.removeAttribute('data-swiping')
      track()?.style.removeProperty('--swipe-x')
      dots()?.style.removeProperty('--swipe-pos')
      raw = 0
      shown = 0
      paintW = 0
      samples = []
      locked = false
      triggered = false
      window.clearTimeout(fallback)
    }
    reset.current = (from = -1) => {
      stop?.()
      stop = null
      // Same on-screen position, expressed from the new page: -page × width + offset stays constant.
      const x = from < 0 ? 0 : shown + (index() - from) * width()
      clear()
      if (!live.current.animateJumps || reduced.matches || Math.abs(x) < 0.5) return
      root.setAttribute('data-swiping', '')
      raw = x
      paint(x)
      stop = spring({
        from: x,
        to: 0,
        onUpdate: (v) => {
          raw = v
          paint(v)
        },
        onDone: () => {
          stop = null
          clear()
        }
      })
    }

    const velocity = () => {
      const last = samples[samples.length - 1]
      const first = samples.find((s) => last && last.t - s.t <= VELOCITY_WINDOW)
      if (!last || !first || last.t === first.t) return 0
      return ((last.x - first.x) / (last.t - first.t)) * 1000
    }

    const finish = (forced?: number) => {
      if (locked && forced === undefined) return
      if (reduced.matches) {
        raw = 0
        triggered = false
        return
      }
      const w = width()
      const v = velocity()
      // A gesture stopped dead (near-zero velocity) is judged on position alone.
      const projected = raw + project(Math.abs(v) < 40 ? 0 : v)
      let target = 0
      if (projected > w / 2 && hasPrev()) target = w
      else if (projected < -w / 2 && hasNext()) target = -w
      if (forced !== undefined) target = forced

      if (shown === 0 && target === 0) return clear()
      stop = spring({
        from: shown,
        to: target,
        velocity: forced !== undefined ? 0 : v,
        // A fast fling toward the neighbor keeps its momentum: slight bounce. Otherwise it stays critical.
        damping: forced === undefined && target !== 0 && Math.abs(v) > FLICK ? 0.85 : 1,
        onUpdate: paint,
        onDone: () => {
          stop = null
          if (target === 0) return clear()
          // We stay on the neighboring space until the new state arrives.
          locked = true
          const { spaceIds: ids, activeId: id, onSwitch: switchTo } = live.current
          const next = ids[ids.indexOf(id) + (target > 0 ? -1 : 1)]
          fallback = window.setTimeout(() => reset.current(), 600)
          if (next) switchTo(next)
          else reset.current()
        }
      })
    }

    const endSpent = () => {
      spent = false
      state = 'idle'
    }
    const endGesture = () => {
      const was = state
      state = 'idle'
      if (was === 'track') finish()
    }
    const arm = () => {
      window.clearTimeout(endTimer)
      endTimer = window.setTimeout(endGesture, END_DELAY)
    }

    const onWheel = (e: WheelEvent) => {
      if (editable(e.target)) return
      // A single page: nothing to slide here, the gesture goes back to the parent.
      if (live.current.spaceIds.length < 2 || handled.has(e)) return
      const dx = e.deltaMode === 1 ? e.deltaX * 16 : e.deltaX
      const dy = e.deltaMode === 1 ? e.deltaY * 16 : e.deltaY

      if (spent) {
        // Leftover momentum after a switch: ignore it until silence, or until new fingers (|dx| rising).
        if (Math.abs(dx) <= Math.abs(dy)) return
        handled.add(e)
        e.preventDefault()
        window.clearTimeout(spentTimer)
        spentTimer = window.setTimeout(endSpent, MOMENTUM_END)
        spentHist.push(Math.abs(dx))
        if (spentHist.length > 3) spentHist.shift()
        const rising = spentHist.length === 3 && spentHist[0] < spentHist[1] && spentHist[1] < spentHist[2]
        if (!rising) return
        spent = false
        state = 'idle'
        window.clearTimeout(spentTimer)
      }

      if (state === 'idle') {
        state = 'pending'
        accX = accY = 0
      }
      arm()
      if (state === 'ignore') return // vertical gesture: leave it alone
      let moveX = dx
      if (state === 'pending') {
        // Before the decision, accumulate without cancelling anything: vertical scrolling stays intact.
        accX += dx
        accY += dy
        if (Math.abs(accX) + Math.abs(accY) < LOCK_AT) return
        if (Math.abs(accX) <= 1.5 * Math.abs(accY)) {
          state = 'ignore'
          return
        }
        state = 'track'
        moveX = accX
        if (stop) {
          // New gesture during an animation: restart from the displayed position.
          stop()
          stop = null
          raw = toRaw(shown)
          samples = []
        }
      }
      handled.add(e)
      e.preventDefault()
      if (locked) return

      if (reduced.matches) {
        raw -= moveX
        if (!triggered && Math.abs(raw) >= REDUCED_THRESHOLD) {
          const dir = raw < 0 ? 1 : -1
          const next = live.current.spaceIds[index() + dir]
          if (next) {
            triggered = true
            // The space change resets the gesture (clear): swallow the rest of the momentum to avoid chaining.
            spent = true
            spentHist = []
            state = 'ignore'
            window.clearTimeout(spentTimer)
            spentTimer = window.setTimeout(endSpent, MOMENTUM_END)
            live.current.onSwitch(next)
          }
        }
        return
      }

      root.setAttribute('data-swiping', '')
      raw -= moveX
      const w = width()
      // The neighbor is reached: switch right away without waiting for the end of the momentum, which we then swallow.
      if ((raw >= w && hasPrev()) || (raw <= -w && hasNext())) {
        const dir = raw > 0 ? w : -w
        raw = dir
        paint(dir)
        window.clearTimeout(endTimer)
        state = 'ignore'
        spent = true
        spentHist = []
        window.clearTimeout(spentTimer)
        spentTimer = window.setTimeout(endSpent, MOMENTUM_END)
        finish(dir)
        return
      }
      const x = toShown(raw)
      samples.push({ t: e.timeStamp, x })
      if (samples.length > 40) samples.shift()
      paint(x)
    }

    root.addEventListener('wheel', onWheel, { passive: false })
    return () => {
      root.removeEventListener('wheel', onWheel)
      window.clearTimeout(endTimer)
      window.clearTimeout(spentTimer)
      stop?.()
      clear()
    }
  }, [rootRef, clipRef, trackSel, pos])
}

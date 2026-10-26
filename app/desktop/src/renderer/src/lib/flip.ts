import { useLayoutEffect, useRef } from 'react'
import type { RefObject } from 'react'
import { reducedMotion } from './spring'

/**
 * List FLIP: the `[data-flip-id]` elements of a container glide from their displayed position to their new place.
 *
 * - Before: measured during the render that changes the signature (`sig`), hence before React touches the DOM; the measurement
 *   includes ongoing animations (on-screen position).
 * - After: at layout, the new position (ongoing animations included) gives the missing offset; we start an
 *   additive Web Animations animation (`composite: 'add'`) from this offset to nothing, with the `--spring` curve.
 *   Animations already in progress continue: their velocity adds up instead of being cut (smooth reversal).
 * - Nested containers (folders, groups) have their own id: a child only receives its offset relative to the parent.
 * - An element absent from the "before" measurement is new: it animates itself (CSS `.fresh`, `data-flip-fade`).
 * - `data-flip-live` marks an element on which a FLIP animation is running (read by `layoutPos`).
 */

type Pos = { x: number; y: number }

/** Position (relative to the container) of each visible element. */
function measure(root: HTMLElement | null): Map<string, Pos> {
  const out = new Map<string, Pos>()
  if (!root) return out
  const base = root.getBoundingClientRect()
  for (const el of root.querySelectorAll<HTMLElement>('[data-flip-id]')) {
    const r = el.getBoundingClientRect()
    if (r.width === 0 && r.height === 0) continue // display: none (e.g. element in hand)
    out.set(el.dataset.flipId as string, { x: r.left - base.left, y: r.top - base.top })
  }
  return out
}

/** Same values as `--spring` and `--t-slow` (styles.css). */
const SPRING = 'linear(0, 0.17 4%, 0.48 10%, 0.72 17%, 0.87 25%, 0.95 34%, 0.985 45%, 1)'
const DURATION = 320

function slide(el: HTMLElement, x: number, y: number) {
  const anim = el.animate([{ transform: `translate(${x}px, ${y}px)` }, { transform: 'none' }], {
    duration: DURATION,
    easing: SPRING,
    composite: 'add'
  })
  anim.id = 'flip'
  el.setAttribute('data-flip-live', '')
  const end = () => {
    anim.onfinish = anim.oncancel = null
    anim.cancel() // releases the finished animation
    if (!el.getAnimations().some((a) => a.id === 'flip')) el.removeAttribute('data-flip-live')
  }
  anim.onfinish = end
  anim.oncancel = end
}

/**
 * `sig` changes as soon as the order or structure of the elements changes; `scope` (e.g. space id): a change
 * of scope is not animated. `alias`: id that inherits the place of the `gap` element (drop: the element lands in the void).
 */
export function useFlip(ref: RefObject<HTMLElement | null>, sig: string, scope: string, alias: string | null = null) {
  const st = useRef({ sig, scope, first: null as Map<string, Pos> | null, alias })
  // Render: the DOM still shows the old state, this is the time to measure.
  if (st.current.sig !== sig) {
    st.current.first = measure(ref.current)
    st.current.alias = alias
  }

  useLayoutEffect(() => {
    const s = st.current
    const first = s.first
    const sameScope = s.scope === scope
    s.first = null
    s.sig = sig
    s.scope = scope
    const root = ref.current
    if (!root) return
    if (!first || !sameScope || reducedMotion.matches) return

    const base = root.getBoundingClientRect()
    const view = (root.closest('.scroll') ?? root).getBoundingClientRect()
    const els = root.querySelectorAll<HTMLElement>('[data-flip-id]')
    const total = new Map<HTMLElement, Pos>()
    const entering: HTMLElement[] = []
    const rects: DOMRect[] = []
    for (const el of els) rects.push(el.getBoundingClientRect())
    els.forEach((el, i) => {
      const r = rects[i]
      if (r.width === 0 && r.height === 0) return
      const id = el.dataset.flipId as string
      const f = first.get(id) ?? (s.alias === id ? first.get('gap') : undefined)
      if (!f) {
        if (el.dataset.flipFade !== undefined) entering.push(el)
        return
      }
      const dx = f.x - (r.left - base.left)
      const dy = f.y - (r.top - base.top)
      if (Math.abs(dx) < 0.5 && Math.abs(dy) < 0.5) return
      // Outside the visible area both before and after: no need to animate (hundreds of rows may glide).
      const fromTop = base.top + f.y
      const off = (t: number) => t > view.bottom + 80 || t + r.height < view.top - 80
      if (off(fromTop) && off(r.top)) return
      total.set(el, { x: dx, y: dy })
    })

    // Write: offset relative to the nearest animated parent (which already carries part of the movement).
    for (const [el, t] of total) {
      const up = el.parentElement?.closest<HTMLElement>('[data-flip-id]')
      const p = (up && total.get(up)) || { x: 0, y: 0 }
      const x = t.x - p.x
      const y = t.y - p.y
      if (Math.abs(x) < 0.5 && Math.abs(y) < 0.5) continue
      slide(el, x, y)
    }
    for (const el of entering) el.animate([{ opacity: 0 }, { opacity: 1 }], { duration: 140, easing: 'ease-out' })
  }, [ref, sig, scope])
}

/**
 * Position of an element in its container, without the ongoing FLIP offsets (the element's "final" place):
 * this is where the active tab's pill must go.
 */
export function layoutPos(el: HTMLElement, root: HTMLElement): Pos {
  const r = el.getBoundingClientRect()
  const b = root.getBoundingClientRect()
  let x = r.left - b.left
  let y = r.top - b.top
  for (let n: HTMLElement | null = el; n && n !== root; n = n.parentElement) {
    if (!n.hasAttribute('data-flip-live')) continue
    const m = new DOMMatrixReadOnly(getComputedStyle(n).transform)
    x -= m.m41
    y -= m.m42
  }
  return { x, y }
}

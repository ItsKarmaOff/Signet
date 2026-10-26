import { useCallback, useEffect, useRef, useState } from 'react'
import type { PointerEvent } from 'react'
import type { Divider, Snapshot } from '@shared/types'
import { reducedMotion, rubberband, spring, unrubberband } from '../lib/spring'

/** Minimum size of a pane (same value as `MIN_PANE` in main). */
const MIN_PANE = 200

interface Drag {
  pointerId: number
  /** Pointer position at the start, along the divider axis. */
  start: number
  /** Divider position at the start, along the same axis. */
  from: number
  lengths: number[]
  /** Raw overshoot already taken at grab time (rubber-band in progress), to restart from the displayed position. */
  past0: number
}

/** Current lengths of the displayed split view's shares, from the pane rectangles. */
function currentLengths(s: Snapshot, d: Divider): number[] {
  const vertical = d.orientation === 'vertical'
  const len = (i: number) => (vertical ? s.panes[i].card.width : s.panes[i].card.height)
  const mine = s.dividers.filter((x) => x.splitId === d.splitId)
  // Grid: a single horizontal divider for 3 or more panes; shares = [top row, bottom row].
  if (!vertical && mine.length === 1 && s.panes.length >= 3) return [len(0), len(s.panes.length - 1)]
  return s.panes.map((_, i) => len(i))
}

function Handle({ s, d }: { s: Snapshot; d: Divider }) {
  const vertical = d.orientation === 'vertical'
  const drag = useRef<Drag | null>(null)
  const frame = useRef(0)
  const latest = useRef<number[] | null>(null)
  const stopSpring = useRef<(() => void) | null>(null)
  // Overshoot shown beyond the limits (px) and the matching rest position.
  const over = useRef({ rest: 0, extra: 0 })
  // Smoothed velocity of the displayed handle (px/s) for the release spring.
  const lastMove = useRef({ pos: 0, t: 0, v: 0 })
  // Local position during the drag (and until main's state has caught up).
  const [local, setLocal] = useState<number | null>(null)
  const [active, setActive] = useState(false)
  const escOff = useRef<(() => void) | null>(null)
  const axis = vertical ? d.rect.x : d.rect.y

  // Last shares sent during the drag: release resends them without the "live" flag (that is where we save).
  const sent = useRef<number[] | null>(null)

  const flush = useCallback(
    (end = false) => {
      frame.current = 0
      const sizes = latest.current ?? (end ? sent.current : null)
      latest.current = null
      if (end) sent.current = null
      else if (sizes) sent.current = sizes
      if (sizes) window.signet.setSplitSizes(d.splitId, sizes, !end).catch(() => {})
    },
    [d.splitId]
  )

  useEffect(
    () => () => {
      cancelAnimationFrame(frame.current)
      stopSpring.current?.()
      escOff.current?.()
    },
    []
  )

  // Main's state has caught up with the local position: switch back to it.
  useEffect(() => {
    if (!active && local !== null && Math.abs(axis - local) <= 1) setLocal(null)
  }, [axis, active, local])
  useEffect(() => {
    if (active || local === null) return
    const t = setTimeout(() => setLocal(null), 400)
    return () => clearTimeout(t)
  }, [active, local])

  const down = (e: PointerEvent<HTMLDivElement>) => {
    if (e.button !== 0) return
    e.preventDefault()
    // Grabbed during the rubber-band: the handle is at `local`, not at main's clamped position.
    const springing = stopSpring.current !== null
    stopSpring.current?.()
    stopSpring.current = null
    e.currentTarget.setPointerCapture(e.pointerId)
    const shown = springing && local !== null ? local : axis
    const g: Drag = {
      pointerId: e.pointerId,
      start: vertical ? e.clientX : e.clientY,
      from: axis,
      lengths: currentLengths(s, d),
      past0: springing ? unrubberband(shown - axis, 120) : 0
    }
    drag.current = g
    over.current = { rest: axis, extra: shown - axis }
    lastMove.current = { pos: shown, t: performance.now(), v: 0 }
    setLocal(shown)
    setActive(true)
    // Escape: the starting sizes are restored and the handle gently returns to them.
    const onKey = (k: KeyboardEvent) => {
      if (k.key !== 'Escape' || drag.current !== g) return
      k.preventDefault()
      k.stopPropagation()
      drag.current = null
      escOff.current?.()
      cancelAnimationFrame(frame.current)
      frame.current = 0
      latest.current = null
      sent.current = null
      const sum = g.lengths.reduce((x, y) => x + y, 0)
      window.signet
        .setSplitSizes(
          d.splitId,
          g.lengths.map((l) => l / sum)
        )
        .catch(() => {})
      const pos = lastMove.current.pos
      over.current = { rest: g.from, extra: pos - g.from }
      lastMove.current.t = 0
      release()
    }
    escOff.current?.()
    window.addEventListener('keydown', onKey, true)
    escOff.current = () => window.removeEventListener('keydown', onKey, true)
  }

  const move = (e: PointerEvent<HTMLDivElement>) => {
    const g = drag.current
    if (!g || g.pointerId !== e.pointerId) return
    const delta = (vertical ? e.clientX : e.clientY) - g.start
    const a = g.lengths[d.index]
    const b = g.lengths[d.index + 1]
    const total = a + b
    // If the two shares cannot respect the minimum, do not move.
    const lo = MIN_PANE
    const hi = total - MIN_PANE
    const want = a + delta + g.past0
    const next = total < MIN_PANE * 2 ? a : Math.min(hi, Math.max(lo, want))
    // Main caps the sizes: only the handle follows the finger beyond that, with resistance.
    const past = total < MIN_PANE * 2 || reducedMotion.matches ? 0 : want < lo ? want - lo : want > hi ? want - hi : 0
    const extra = rubberband(past, 120)
    const lengths = g.lengths.slice()
    lengths[d.index] = next
    lengths[d.index + 1] = total - next
    const sum = lengths.reduce((x, y) => x + y, 0)
    latest.current = lengths.map((l) => l / sum)
    over.current = { rest: g.from + (next - a), extra }
    const pos = g.from + (next - a) + extra
    const now = performance.now()
    const dt = now - lastMove.current.t
    if (dt > 0) lastMove.current = { pos, t: now, v: lastMove.current.v * 0.5 + ((pos - lastMove.current.pos) / dt) * 1000 * 0.5 }
    setLocal(pos)
    if (!frame.current) frame.current = requestAnimationFrame(() => flush())
  }

  const up = (e: PointerEvent<HTMLDivElement>) => {
    if (!drag.current || drag.current.pointerId !== e.pointerId) return
    drag.current = null
    escOff.current?.()
    cancelAnimationFrame(frame.current)
    flush(true)
    release()
  }

  /** End of the drag: the handle springs back from its overshoot, then hands control back to main's state. */
  const release = () => {
    const { rest, extra } = over.current
    over.current = { rest: 0, extra: 0 }
    if (!extra) return setActive(false)
    const idle = performance.now() - lastMove.current.t > 100
    stopSpring.current = spring({
      from: rest + extra,
      to: rest,
      velocity: idle ? 0 : lastMove.current.v,
      onUpdate: (v) => setLocal(v),
      onDone: () => {
        stopSpring.current = null
        setActive(false)
      }
    })
  }

  const rect = local === null ? d.rect : vertical ? { ...d.rect, x: local } : { ...d.rect, y: local }
  return (
    <div
      className={`split-divider ${d.orientation}${active ? ' active' : ''}`}
      style={{ left: rect.x, top: rect.y, width: rect.width, height: rect.height }}
      onPointerDown={down}
      onPointerMove={move}
      onPointerUp={up}
      onPointerCancel={up}
      onLostPointerCapture={() => {
        if (drag.current) {
          drag.current = null
          escOff.current?.()
          cancelAnimationFrame(frame.current)
          flush(true)
          release()
        }
      }}
      onDoubleClick={() => window.signet.setSplitSizes(d.splitId, []).catch(() => {})}
    >
      <span className="divider-handle" />
    </div>
  )
}

/** Resizable split view dividers (8 px grab zones between the panes). */
export function Dividers({ s }: { s: Snapshot }) {
  return (
    <>
      {s.dividers.map((d) => (
        <Handle key={`${d.splitId}:${d.index}`} s={s} d={d} />
      ))}
    </>
  )
}

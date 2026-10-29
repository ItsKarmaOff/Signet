import { useEffect, useMemo, useRef, useState, useSyncExternalStore } from 'react'
import type { PointerEvent as ReactPointerEvent } from 'react'
import { flushSync } from 'react-dom'
import { useTranslation } from 'react-i18next'
import { Puzzle } from 'lucide-react'
import { reducedMotion, rubberband } from '../lib/spring'
import { badgeColors } from '../lib/util'

// ───────────────────────── Action state (shared between the bars) ─────────────────────────

const states = new Map<string, BrowserActionState>()
const subscribers = new Map<string, Set<() => void>>()
let fetching = 0

function notify(partition: string): void {
  subscribers.get(partition)?.forEach((fn) => fn())
}

/** Re-reads a partition's state. Only the return of `getState` is reliable: `update` events mix partitions. */
async function refresh(partition: string): Promise<void> {
  const api = window.browserAction
  if (!api) return
  fetching++
  try {
    const next = await api.getState(partition)
    if (JSON.stringify(next) !== JSON.stringify(states.get(partition))) {
      states.set(partition, next)
      notify(partition)
    }
  } catch {
    // partition unavailable: keep the previous state
  } finally {
    // The `update` event from `getState` fires in a microtask: ignore those.
    queueMicrotask(() => {
      fetching--
    })
  }
}

function onUpdate(payload: BrowserActionState): void {
  if (fetching > 0) return
  const json = JSON.stringify(payload)
  // A state already known (other partition or echo) asks for nothing; otherwise re-read the observed partitions.
  if ([...states.values()].some((s) => JSON.stringify(s) === json)) return
  for (const p of subscribers.keys()) void refresh(p)
}

function subscribe(partition: string, fn: () => void): () => void {
  const api = window.browserAction
  let set = subscribers.get(partition)
  if (!set) subscribers.set(partition, (set = new Set()))
  if (subscribers.size === 1 && set.size === 0) api?.addEventListener('update', onUpdate)
  set.add(fn)
  // A single observer per partition (no counter on the extensions side): added at the first subscriber, removed at the last.
  if (set.size === 1) api?.addObserver(partition)
  void refresh(partition)
  return () => {
    set.delete(fn)
    if (set.size === 0) {
      api?.removeObserver(partition)
      subscribers.delete(partition)
    }
    if (subscribers.size === 0) api?.removeEventListener('update', onUpdate)
  }
}

// A stable subscribe function per partition: otherwise React resubscribes (and re-reads the state) on every render.
const subscribeFns = new Map<string, (fn: () => void) => () => void>()
function subscribeTo(partition: string): (fn: () => void) => () => void {
  let f = subscribeFns.get(partition)
  if (!f) subscribeFns.set(partition, (f = (fn) => subscribe(partition, fn)))
  return f
}

function useActionState(partition: string): BrowserActionState | undefined {
  return useSyncExternalStore(subscribeTo(partition), () => states.get(partition))
}

// ───────────────────────── Buttons ─────────────────────────

interface Props {
  partition: string
  order: string[]
  /** Pinned extensions: the only ones shown as buttons; all open from the Extensions button. */
  pinned: string[]
  /** Focuses the bar's tab before an action (menu, popup). */
  focusTab: () => void
  onOrder: (order: string[]) => void
  onMenu: (id: string) => void
}

interface Drag {
  id: string
  from: number
  to: number
  dx: number
  settling: boolean
}

const THRESHOLD = 4

/** Action buttons of the pinned extensions (saved order, horizontal drag and drop), then the Extensions button. */
export function ExtensionButtons({ partition, order, pinned, focusTab, onOrder, onMenu }: Props) {
  const { t } = useTranslation()
  const state = useActionState(partition)
  const [local, setLocal] = useState<string[] | null>(null)
  const [drag, setDrag] = useState<Drag | null>(null)
  const press = useRef<{ id: string; x: number; pointer: number; slots: { left: number; width: number }[]; index: number; started: boolean } | null>(null)
  const dragRef = useRef<Drag | null>(null)
  const skipClick = useRef(false)
  // Pending end of animation: a new press finishes it right away.
  const settleNow = useRef<(() => void) | null>(null)
  const box = useRef<HTMLDivElement>(null)
  dragRef.current = drag

  const orderKey = order.join()
  useEffect(() => setLocal(null), [orderKey])

  // Full order (unpinned included) then displayed buttons.
  const all = useMemo(() => {
    const ids = (state?.actions ?? []).map((a) => a.id)
    const saved = local ?? order
    return [...saved.filter((id) => ids.includes(id)), ...ids.filter((id) => !saved.includes(id))]
  }, [state, order, local])
  const shown = all.filter((id) => pinned.includes(id))

  const cancel = () => {
    const d = dragRef.current
    press.current = null
    if (!d) return
    setDrag({ ...d, to: d.from, dx: 0, settling: true })
    settle(null)
  }
  const settle = (commit: string[] | null) => {
    let timer = 0
    const finish = () => {
      window.clearTimeout(timer)
      settleNow.current = null
      if (commit) {
        setLocal(commit)
        onOrder(commit)
      }
      setDrag(null)
    }
    settleNow.current = finish
    timer = window.setTimeout(finish, reducedMotion.matches ? 0 : 150)
  }

  useEffect(() => {
    if (!drag || drag.settling) return
    const key = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault()
        cancel()
      }
    }
    window.addEventListener('keydown', key, true)
    return () => window.removeEventListener('keydown', key, true)
  })

  const down = (e: ReactPointerEvent<HTMLButtonElement>, id: string, index: number) => {
    if (e.button !== 0) return
    // During the return to place of a previous drag: finish it (saved order), but the grabbed icon stays where it is on screen.
    const liveLeft = e.currentTarget.getBoundingClientRect().left
    if (settleNow.current) flushSync(settleNow.current)
    if (dragRef.current) return
    // The render may have reordered the buttons: the index is re-read from the DOM.
    const found = [...(box.current?.querySelectorAll('button') ?? [])].findIndex((b) => b.dataset.ext === id)
    if (found >= 0) index = found
    const slots = [...(box.current?.querySelectorAll('button') ?? [])].map((b) => {
      const r = b.getBoundingClientRect()
      // Rest position: remove the translation in progress (end animation not finished).
      const tx = new DOMMatrix(getComputedStyle(b).transform).m41
      return { left: r.left - tx, width: r.width }
    })
    // Gap between the rest slot and the position seen on screen (interrupted end animation).
    const offset = found >= 0 ? liveLeft - slots[index].left : 0
    press.current = { id, x: e.clientX - offset, pointer: e.pointerId, slots, index, started: false }
    e.currentTarget.setPointerCapture(e.pointerId)
    if (Math.abs(offset) > 0.5) {
      // The drag resumes right away from the displayed position: no jump.
      press.current.started = true
      let to = index
      slots.forEach((sl, i) => {
        if (Math.abs(sl.left - liveLeft) < Math.abs(slots[to].left - liveLeft)) to = i
      })
      setDrag({ id, from: index, to, dx: offset, settling: false })
    }
  }
  const move = (e: ReactPointerEvent<HTMLButtonElement>) => {
    const p = press.current
    if (!p || p.pointer !== e.pointerId) return
    let dx = e.clientX - p.x
    if (!p.started) {
      if (Math.abs(dx) < THRESHOLD) return
      p.started = true
    }
    // The button stays in the row.
    const first = p.slots[0].left - p.slots[p.index].left
    const last = p.slots[p.slots.length - 1].left - p.slots[p.index].left
    const bounded = Math.max(first, Math.min(last, dx))
    // Slot nearest to the dragged button's center (clamped position, without the rubber-band).
    const center = p.slots[p.index].left + bounded
    // Beyond the row, the button follows the finger with resistance.
    dx = reducedMotion.matches ? bounded : bounded + rubberband(dx - bounded, 80)
    let to = 0
    p.slots.forEach((s, i) => {
      if (Math.abs(s.left - center) < Math.abs(p.slots[to].left - center)) to = i
    })
    setDrag({ id: p.id, from: p.index, to, dx, settling: false })
  }
  const up = (e: ReactPointerEvent<HTMLButtonElement>) => {
    const p = press.current
    if (!p || p.pointer !== e.pointerId) return
    press.current = null
    if (!p.started) return
    skipClick.current = true
    setTimeout(() => (skipClick.current = false), 0)
    const d = dragRef.current
    if (!d) return
    const snapped = p.slots[d.to].left - p.slots[d.from].left
    setDrag({ ...d, dx: snapped, settling: true })
    if (d.to === d.from) return settle(null)
    const next = [...shown]
    next.splice(d.to, 0, next.splice(d.from, 1)[0])
    // Unpinned extensions keep their place in the full order.
    const full = [...all]
    const slotsIdx = full.map((id, i) => (pinned.includes(id) ? i : -1)).filter((i) => i >= 0)
    slotsIdx.forEach((pos, i) => (full[pos] = next[i]))
    settle(full)
  }
  const lost = (e: ReactPointerEvent<HTMLButtonElement>) => {
    if (press.current?.pointer === e.pointerId && dragRef.current && !dragRef.current.settling) cancel()
    else if (press.current?.pointer === e.pointerId) press.current = null
  }

  const activate = (e: React.MouseEvent<HTMLButtonElement>, id: string) => {
    if (skipClick.current) return
    const r = e.currentTarget.getBoundingClientRect()
    focusTab()
    // -1: the window's active tab, already updated by focusTab.
    void window.browserAction?.activate(partition, {
      eventType: 'click',
      extensionId: id,
      tabId: -1,
      alignment: 'bottom left',
      anchorRect: { x: r.left, y: r.top, width: r.width, height: r.height }
    })
  }

  /** Extensions button: menu of all extensions; the chosen one opens under this button. */
  const openList = async (e: React.MouseEvent<HTMLButtonElement>) => {
    const r = e.currentTarget.getBoundingClientRect()
    focusTab()
    const id = await window.signet.showExtensionsMenu()
    if (!id) return
    void window.browserAction?.activate(partition, {
      eventType: 'click',
      extensionId: id,
      tabId: -1,
      alignment: 'bottom left',
      anchorRect: { x: r.left, y: r.top, width: r.width, height: r.height }
    })
  }

  if (!state || state.actions.length === 0) return null
  const tabId = state.activeTabId ?? -1
  const list = (
    <button className="icon ext-list" title={t('toolbar.extensions')} aria-label={t('toolbar.extensions')} onClick={(e) => void openList(e)}>
      <Puzzle size={15} />
    </button>
  )
  if (shown.length === 0) return list
  return (
    <>
      <div ref={box} className={`ext-actions${drag ? ' dragging' : ''}`}>
        {shown.map((id, i) => {
          const a = state.actions.find((x) => x.id === id)!
          const info = { ...a, ...(tabId > -1 ? a.tabs[tabId] : undefined) }
          const mine = drag?.id === id
          return (
            <ActionButton
              key={id}
              id={id}
              info={info}
              partition={partition}
              tabId={tabId}
              mine={mine}
              settling={!!drag?.settling}
              shift={drag && mine ? drag.dx : drag ? slotShift(box.current, drag, i) : 0}
              onPointerDown={(e) => down(e, id, i)}
              onPointerMove={move}
              onPointerUp={up}
              onPointerCancel={lost}
              onLostPointerCapture={lost}
              onClick={(e) => activate(e, id)}
              onContextMenu={(e) => {
                e.preventDefault()
                focusTab()
                onMenu(id)
              }}
            />
          )
        })}
      </div>
      {list}
    </>
  )
}

/** Offset (px) of a neighboring button: dragged button's width plus the gap, in the direction that frees the slot. */
function slotShift(box: HTMLDivElement | null, d: Drag, i: number): number {
  const buttons = box?.querySelectorAll('button')
  if (!buttons || buttons.length < 2) return 0
  const a = buttons[0].getBoundingClientRect()
  const b = buttons[1].getBoundingClientRect()
  const step = b.left - a.left
  if (d.from < d.to && i > d.from && i <= d.to) return -step
  if (d.from > d.to && i >= d.to && i < d.from) return step
  return 0
}

interface ButtonProps {
  id: string
  info: { title?: string; text?: string; color?: unknown; iconModified?: number }
  partition: string
  tabId: number
  mine: boolean
  settling: boolean
  shift: number
  onPointerDown: (e: ReactPointerEvent<HTMLButtonElement>) => void
  onPointerMove: (e: ReactPointerEvent<HTMLButtonElement>) => void
  onPointerUp: (e: ReactPointerEvent<HTMLButtonElement>) => void
  onPointerCancel: (e: ReactPointerEvent<HTMLButtonElement>) => void
  onLostPointerCapture: (e: ReactPointerEvent<HTMLButtonElement>) => void
  onClick: (e: React.MouseEvent<HTMLButtonElement>) => void
  onContextMenu: (e: React.MouseEvent<HTMLButtonElement>) => void
}

function ActionButton({ id, info, partition, tabId, mine, settling, shift, ...handlers }: ButtonProps) {
  // The icon may not be ready at first display: a few more attempts before the letter.
  const [tries, setTries] = useState(0)
  const failed = tries > 3
  const query = new URLSearchParams({ tabId: `${tabId}`, partition })
  if (info.iconModified) query.set('t', `${info.iconModified}`)
  if (tries) query.set('r', `${tries}`)
  const src = `crx://extension-icon/${id}/32/2?${query}`
  const title = info.title ?? ''
  const transform = shift ? `translateX(${shift}px)` : undefined
  return (
    <button data-ext={id} className={`ext-btn${mine ? ' dragged' : ''}${mine && !settling ? ' held' : ''}`} title={title} style={{ transform }} {...handlers}>
      {failed ? (
        <span className="ext-letter">{title.charAt(0)}</span>
      ) : (
        <img src={src} alt="" draggable={false} onError={() => setTimeout(() => setTries((n) => n + 1), 400)} />
      )}
      {info.text && (
        <span className="ext-badge" style={badgeColors(info.color)}>
          {info.text}
        </span>
      )}
    </button>
  )
}

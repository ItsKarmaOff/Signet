import { Monitor, RotateCcwSquare, Smartphone, X } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'
import type { KeyboardEvent, PointerEvent } from 'react'
import { useTranslation } from 'react-i18next'
import type { DevDevice, DevResponsive, Pane } from '@shared/types'
import { DEVICES, MAX_DEVICE, MIN_DEVICE, asCustom, deviceSpec, responsiveProfile } from '@shared/devices'
import { reducedMotion, rubberband, spring, unrubberband } from '../lib/spring'

type Placed = NonNullable<Pane['responsive']>

const clampSize = (n: number) => Math.round(Math.min(MAX_DEVICE, Math.max(MIN_DEVICE, n)))

/** Size field: freely editable, sent on Enter or when leaving the field. */
function SizeInput({ value, label, onCommit }: { value: number; label: string; onCommit: (n: number) => void }) {
  const [draft, setDraft] = useState<string | null>(null)
  const commit = () => {
    if (draft === null) return
    const n = Number(draft)
    setDraft(null)
    if (Number.isFinite(n) && n > 0 && clampSize(n) !== value) onCommit(clampSize(n))
  }
  const key = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Enter') e.currentTarget.blur()
    else if (e.key === 'Escape') {
      setDraft(null)
      e.currentTarget.blur()
    }
  }
  return (
    <input
      className="rs-size"
      type="number"
      min={MIN_DEVICE}
      max={MAX_DEVICE}
      aria-label={label}
      title={label}
      value={draft ?? String(value)}
      onChange={(e) => setDraft(e.target.value)}
      onBlur={commit}
      onKeyDown={key}
    />
  )
}

/** Size handle in the margin around the page: the page is centered, so the width grows on both sides. */
function Handle({
  edge,
  frame,
  r,
  send,
  tabId
}: {
  edge: 'right' | 'bottom' | 'corner'
  frame: Placed & { rect: Pane['rect'] }
  r: DevResponsive
  send: (r: DevResponsive) => void
  tabId: string
}) {
  const { t } = useTranslation()
  const drag = useRef<{ id: number; x: number; y: number; fw: number; fh: number; sc: number; start: DevResponsive } | null>(null)
  const frameRef = useRef(0)
  const latest = useRef<DevResponsive | null>(null)
  const el = useRef<HTMLDivElement>(null)
  /** Tells the main process: the scale is frozen during the drag (the handle follows the pointer 1:1), recomputed on release. */
  const dragging = (on: boolean) => void window.signet.setResponsiveDragging(tabId, on).catch(() => {})
  // Displayed elastic offset of the handle beyond the min / max sizes (px), and its return.
  const extra = useRef({ x: 0, y: 0 })
  const stopBack = useRef<(() => void) | null>(null)
  const escOff = useRef<(() => void) | null>(null)
  useEffect(
    () => () => {
      cancelAnimationFrame(frameRef.current)
      stopBack.current?.()
      escOff.current?.()
      if (drag.current) void window.signet.setResponsiveDragging(tabId, false).catch(() => {})
    },
    [tabId]
  )
  const paintExtra = (x: number, y: number) => {
    extra.current = { x, y }
    if (el.current) el.current.style.transform = x || y ? `translate(${x}px, ${y}px)` : ''
  }
  const f = frame.rect
  const style =
    edge === 'right'
      ? { left: f.x + f.width + 2, top: f.y, width: 10, height: f.height }
      : edge === 'bottom'
        ? { left: f.x, top: f.y + f.height + 2, width: f.width, height: 10 }
        : { left: f.x + f.width + 2, top: f.y + f.height + 2, width: 12, height: 12 }
  /** Elastic return of the handle to its place. */
  const springBack = () => {
    const { x, y } = extra.current
    if (!x && !y) return
    stopBack.current = spring({
      from: 1,
      to: 0,
      onUpdate: (k) => paintExtra(x * k, y * k),
      onDone: () => {
        stopBack.current = null
        paintExtra(0, 0)
      }
    })
  }
  const down = (e: PointerEvent<HTMLDivElement>) => {
    if (e.button !== 0) return
    e.preventDefault()
    // Grabbed during the elastic return: we restart from the handle's displayed position.
    stopBack.current?.()
    stopBack.current = null
    const sc = frame.scale || 1
    const fw = r.width + (edge === 'bottom' ? 0 : (unrubberband(extra.current.x, 80) * 2) / sc)
    const fh = r.height + (edge === 'right' ? 0 : unrubberband(extra.current.y, 80) / sc)
    e.currentTarget.setPointerCapture(e.pointerId)
    const g = { id: e.pointerId, x: e.clientX, y: e.clientY, fw, fh, sc, start: r }
    drag.current = g
    dragging(true)
    // Esc: the starting size is restored.
    const onKey = (k: globalThis.KeyboardEvent) => {
      if (k.key !== 'Escape' || drag.current !== g) return
      k.preventDefault()
      k.stopPropagation()
      drag.current = null
      escOff.current?.()
      cancelAnimationFrame(frameRef.current)
      frameRef.current = 0
      latest.current = null
      send(g.start)
      dragging(false)
      springBack()
    }
    escOff.current?.()
    window.addEventListener('keydown', onKey, true)
    escOff.current = () => window.removeEventListener('keydown', onKey, true)
  }
  const move = (e: PointerEvent<HTMLDivElement>) => {
    const g = drag.current
    if (!g || g.id !== e.pointerId) return
    // Scale frozen at the drag start (main process): the handle follows the pointer 1:1.
    const sc = g.sc
    if (edge !== 'bottom') g.fw += ((e.clientX - g.x) * 2) / sc
    if (edge !== 'right') g.fh += (e.clientY - g.y) / sc
    g.x = e.clientX
    g.y = e.clientY
    const width = edge === 'bottom' ? g.start.width : clampSize(g.fw)
    const height = edge === 'right' ? g.start.height : clampSize(g.fh)
    // Beyond the limits, only the handle follows, with resistance.
    if (!reducedMotion.matches) paintExtra(rubberband(((g.fw - width) * sc) / 2, 80), rubberband((g.fh - height) * sc, 80))
    latest.current = asCustom(g.start, { width, height })
    if (!frameRef.current)
      frameRef.current = requestAnimationFrame(() => {
        frameRef.current = 0
        if (latest.current) send(latest.current)
      })
  }
  const up = (e: PointerEvent<HTMLDivElement>) => {
    if (!drag.current || drag.current.id !== e.pointerId) return
    drag.current = null
    escOff.current?.()
    cancelAnimationFrame(frameRef.current)
    frameRef.current = 0
    if (latest.current) send(latest.current)
    latest.current = null
    dragging(false) // the main process readjusts the page to the pane
    springBack()
  }
  return (
    <div
      ref={el}
      className={`rs-handle ${edge}`}
      style={style}
      title={t('develop.resizeDevice')}
      onPointerDown={down}
      onPointerMove={move}
      onPointerUp={up}
      onPointerCancel={up}
      onLostPointerCapture={up}
    >
      <span />
    </div>
  )
}

/** Responsive mode of a pane: background around the page, device bar, size handles. */
export function ResponsiveFrame({ tabId, pane }: { tabId: string; pane: Pane }) {
  const { t } = useTranslation()
  const r = pane.responsive!
  const placed = { ...r, rect: pane.rect }
  const current: DevResponsive = { device: r.device, width: r.width, height: r.height, base: r.base, mobile: r.mobile }
  const mobile = responsiveProfile(current).mobile
  const send = (next: DevResponsive | null) => void window.signet.setResponsive(tabId, next).catch(() => {})
  const pick = (device: DevDevice) => {
    if (device === 'custom') return send(asCustom(current))
    const spec = deviceSpec(device)
    send({ device, width: spec.width, height: spec.height })
  }
  const label = (id: DevDevice) => deviceSpec(id).name ?? t(`develop.device.${id as 'laptop' | 'desktop' | 'custom'}`)
  const group = (kind: 'phone' | 'tablet' | 'computer') => (
    <optgroup label={t(`develop.group.${kind}`)}>
      {DEVICES.filter((d) => d.kind === kind).map((d) => (
        <option key={d.id} value={d.id}>
          {label(d.id)}
        </option>
      ))}
    </optgroup>
  )
  const percent = Math.round(r.scale * 100)
  const a = r.area
  return (
    <>
      <div className="rs-area" style={{ left: a.x, top: a.y, width: a.width, height: a.height }} />
      <div className="rs-bar" style={{ left: r.bar.x, top: r.bar.y, width: r.bar.width, height: r.bar.height }}>
        <select className="rs-device" value={r.device} aria-label={t('develop.deviceLabel')} onChange={(e) => pick(e.target.value as DevDevice)}>
          {group('phone')}
          {group('tablet')}
          {group('computer')}
          <option value="custom">{label('custom')}</option>
        </select>
        <SizeInput value={r.width} label={t('develop.width')} onCommit={(width) => send(asCustom(current, { width }))} />
        <span className="rs-by" aria-hidden />
        <SizeInput value={r.height} label={t('develop.height')} onCommit={(height) => send(asCustom(current, { height }))} />
        <button className="icon sm" title={t('develop.rotate')} onClick={() => send({ ...current, width: r.height, height: r.width })}>
          <RotateCcwSquare size={15} />
        </button>
        {r.device === 'custom' && (
          <button
            className={`icon sm${mobile ? ' on' : ''}`}
            title={t('develop.mobile')}
            aria-pressed={mobile}
            onClick={() => send({ ...asCustom(current), mobile: !mobile })}
          >
            {mobile ? <Smartphone size={15} /> : <Monitor size={15} />}
          </button>
        )}
        {percent < 100 && (
          <span className="rs-scale" title={t('develop.scale', { percent })}>
            {t('develop.scaleShort', { percent })}
          </span>
        )}
        <button className="icon sm" title={t('develop.exitResponsive')} onClick={() => send(null)}>
          <X size={15} />
        </button>
      </div>
      <Handle edge="right" frame={placed} r={current} send={send} tabId={tabId} />
      <Handle edge="bottom" frame={placed} r={current} send={send} tabId={tabId} />
      <Handle edge="corner" frame={placed} r={current} send={send} tabId={tabId} />
    </>
  )
}

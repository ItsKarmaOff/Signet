import { useRef, useState } from 'react'
import type { DragEvent } from 'react'
import type { OverlayPayload, Rect } from '@shared/types'
import { DRAG_FAVORITE_MIME, DRAG_MIME } from '../lib/util'

type Zone = 'left' | 'right' | 'top' | 'bottom' | 'center'
type Payload = Extract<OverlayPayload, { mode: 'dropzones' }>

const EDGE = 0.25

/** Hysteresis (px): the current zone is kept until the pointer has crossed its boundary by this margin. */
const HYST = 12

/**
 * Target zone: the nearest edge if the pointer is in its 25% band, otherwise the center.
 * `prev`: displayed zone; it is left only once the boundary is crossed by `HYST` px (no flicker).
 */
export function zoneAt(x: number, y: number, w: number, h: number, edges: boolean, prev: Zone | null = null): Zone {
  if (!edges || w <= 0 || h <= 0) return 'center'
  // Distance (px) to each edge, and band thickness.
  const near: [Zone, number, number][] = [
    ['left', x, w * EDGE],
    ['right', w - x, w * EDGE],
    ['top', y, h * EDGE],
    ['bottom', h - y, h * EDGE]
  ]
  const inBand = (z: Zone, margin: number) => near.some(([n, d, band]) => n === z && d < band + margin)
  // Current zone kept until we have clearly left its band (or clearly entered another's).
  if (prev && prev !== 'center' && inBand(prev, HYST)) return prev
  const [zone, d, band] = near.reduce((a, b) => (b[1] < a[1] ? b : a))
  if (prev === 'center') return d < band - HYST ? zone : 'center'
  return d < band ? zone : 'center'
}

/** Where the tab will land: the half of the focused pane on the targeted side; the whole content at the center. */
function previewRect(zone: Zone, whole: Rect, pane: Rect): Rect {
  if (zone === 'center') return whole
  const hw = Math.round(pane.width / 2)
  const hh = Math.round(pane.height / 2)
  if (zone === 'left') return { ...pane, width: hw }
  if (zone === 'right') return { ...pane, x: pane.x + pane.width - hw, width: hw }
  if (zone === 'top') return { ...pane, height: hh }
  return { ...pane, y: pane.y + pane.height - hh, height: hh }
}

/** Overlay for dragging a tab over the page: receives the sidebar's HTML5 drag events. */
export function DropZones({ payload }: { payload: Payload }) {
  const box = useRef<HTMLDivElement>(null)
  const [zone, setZone] = useState<Zone | null>(null)
  const zoneRef = useRef<Zone | null>(null)
  const [favorite, setFavorite] = useState(false)
  const { area } = payload

  // Edges require a split view of fewer than 4 panes around a tab that is not a favorite (computed on the main side at drag start).
  const focused = payload.splitCard
  const edges = !favorite && !!focused

  const accepts = (e: DragEvent) => Array.from(e.dataTransfer.types).includes(DRAG_MIME)
  const locate = (e: DragEvent): Zone => {
    const r = box.current?.getBoundingClientRect()
    return r ? zoneAt(e.clientX - r.left, e.clientY - r.top, r.width, r.height, edges, zoneRef.current) : 'center'
  }

  const over = (e: DragEvent) => {
    if (!accepts(e)) return
    e.preventDefault()
    e.dataTransfer.dropEffect = 'move'
    setFavorite(Array.from(e.dataTransfer.types).includes(DRAG_FAVORITE_MIME))
    const z = locate(e)
    zoneRef.current = z
    setZone((p) => (p === z ? p : z))
  }

  const drop = (e: DragEvent) => {
    if (!accepts(e)) return
    e.preventDefault()
    const z = locate(e)
    zoneRef.current = null
    setZone(null)
    try {
      const src = JSON.parse(e.dataTransfer.getData(DRAG_MIME)) as { kind: string; tabId?: string }
      if (src.kind === 'tab' && src.tabId) window.signet.dropOnContent(src.tabId, z).catch(() => {})
      else window.signet.endTabDrag().catch(() => {})
    } catch {
      window.signet.endTabDrag().catch(() => {})
    }
  }

  const whole: Rect = { x: 0, y: 0, width: area.width, height: area.height }
  const pane: Rect = edges && focused ? { x: focused.x - area.x, y: focused.y - area.y, width: focused.width, height: focused.height } : whole
  const shown = zone ? previewRect(zone, whole, pane) : null

  return (
    <div
      ref={box}
      className="dropzones"
      onDragEnter={over}
      onDragOver={over}
      onDragLeave={(e) => {
        if (!e.relatedTarget || !box.current?.contains(e.relatedTarget as Node)) {
          zoneRef.current = null
          setZone(null)
        }
      }}
      onDrop={drop}
    >
      {shown && <div className="drop-preview" style={{ transform: `translate(${shown.x}px, ${shown.y}px)`, width: shown.width, height: shown.height }} />}
    </div>
  )
}

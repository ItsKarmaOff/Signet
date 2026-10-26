import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import type { DragEvent, PointerEvent as ReactPointerEvent, ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { Delete, ImagePlus, Minus, MousePointer2, Pencil, Plus, Trash2, Type } from 'lucide-react'
import type { EaselDoc, EaselItem, NotesEnv } from '@shared/notes'
import { EASEL_MAX } from '@shared/notes'
import { cleanHtml, docIcon, setTabLook } from './notesHtml'
import { project, reducedMotion, rubberband, spring } from '../lib/spring'
import './notes.css'

type Tool = 'select' | 'text' | 'pen'
type View = EaselDoc['view']

/** Pen and text colors (the first follows the theme). */
const COLORS = ['var(--text)', '#e5484d', '#f5a524', '#30a46c', '#3e63dd', '#8e4ec6']
const PEN_SIZE = 3
const SAVE_DELAY = 600
/** Maximum number of undo steps kept. */
const HISTORY_MAX = 100
/** Longest side of an added image (px): beyond it, it is downscaled before entering the Easel. */
const IMAGE_MAX = 1600
const ZOOM_MIN = 0.1
const ZOOM_MAX = 8
/** Threshold (screen px) before an element starts to move: a click or double-click does not shift it. */
const MOVE_THRESHOLD = 4
/** Window (ms) over which the release velocity of the view pan is measured. */
const VELOCITY_WINDOW = 100
/** Wheel silence (ms) after which the zoom returns within its limits. */
const WHEEL_SETTLE = 100

const uid = () => crypto.randomUUID()
/** Pointer capture, with no effect if the pointer is no longer active (the gesture continues without it). */
const capture = (el: Element, id: number) => {
  try {
    el.setPointerCapture(id)
  } catch {
    // unknown pointer
  }
}
const clamp = (n: number, min: number, max: number) => Math.min(max, Math.max(min, n))

/** Smoothed path (curves through segment midpoints) of a stroke. */
function pathOf(points: number[]): string {
  if (points.length < 4) return points.length >= 2 ? `M${points[0]} ${points[1]}l0.01 0` : ''
  let d = `M${points[0]} ${points[1]}`
  for (let i = 2; i < points.length - 2; i += 2) {
    const mx = (points[i] + points[i + 2]) / 2
    const my = (points[i + 1] + points[i + 3]) / 2
    d += `Q${points[i]} ${points[i + 1]} ${mx} ${my}`
  }
  return `${d}L${points[points.length - 2]} ${points[points.length - 1]}`
}

/** Image read, downscaled to IMAGE_MAX on a side (WebP): data URL and size. */
async function readImage(file: Blob): Promise<{ src: string; width: number; height: number } | null> {
  const url = URL.createObjectURL(file)
  try {
    const img = new Image()
    img.src = url
    await img.decode()
    const k = Math.min(1, IMAGE_MAX / Math.max(img.naturalWidth, img.naturalHeight))
    const width = Math.max(1, Math.round(img.naturalWidth * k))
    const height = Math.max(1, Math.round(img.naturalHeight * k))
    const canvas = document.createElement('canvas')
    canvas.width = width
    canvas.height = height
    canvas.getContext('2d')!.drawImage(img, 0, 0, width, height)
    return { src: canvas.toDataURL('image/webp', 0.9), width, height }
  } catch {
    return null
  } finally {
    URL.revokeObjectURL(url)
  }
}

/** Route `#easel/<id>`: free-form board of texts, images and drawings. */
export function Easel() {
  const { t } = useTranslation()
  const [ready, setReady] = useState(false)
  const [hue, setHue] = useState(250)
  const [title, setTitle] = useState('')
  const [items, setItems] = useState<EaselItem[]>([])
  const [view, setView] = useState<View>({ x: 0, y: 0, zoom: 1 })
  const [tool, setTool] = useState<Tool>('select')
  const [color, setColor] = useState(COLORS[0])
  const [selected, setSelected] = useState<string | null>(null)
  const [editing, setEditing] = useState<string | null>(null)
  const [draft, setDraft] = useState<number[] | null>(null)
  const [panning, setPanning] = useState(false)
  const [tooBig, setTooBig] = useState(false)
  const [confirmDelete, setConfirmDelete] = useState(false)
  const port = useRef<HTMLDivElement>(null)
  const fileInput = useRef<HTMLInputElement>(null)
  const viewRef = useRef(view)
  viewRef.current = view
  const itemsRef = useRef(items)
  itemsRef.current = items
  /** Bytes of images added but not yet rendered (reset on every render). */
  const addedSize = useRef(0)
  addedSize.current = 0
  /** Running springs (view glide, zoom return): a new gesture interrupts them. */
  const glide = useRef<Array<() => void>>([])
  const stopGlide = () => {
    glide.current.forEach((stop) => stop())
    glide.current = []
  }
  /** Latest points of the view pan, for the release velocity. */
  const history = useRef<Array<{ t: number; x: number; y: number }>>([])
  /** Raw zoom (no limits) of the current wheel gesture, and its anchor point. */
  const rawZoom = useRef<{ z: number; px: number; py: number; timer: number } | null>(null)

  // Loading.
  useEffect(() => {
    let alive = true
    void window.signetNotes?.load().then((e: NotesEnv) => {
      if (!alive) return
      setHue(e.hue)
      document.documentElement.style.setProperty('--h', String(e.hue))
      if (e.doc?.kind === 'easel') {
        setTitle(e.doc.title)
        // Cleaned texts; a text left empty (page closed while typing) is not kept.
        const blank = (html: string) => !html.replace(/<br\s*\/?>|&nbsp;|\s/g, '')
        const loaded = e.doc.items
          .map((i) => (i.type === 'text' ? { ...i, html: cleanHtml(i.html ?? '') } : i))
          .filter((i) => i.type !== 'text' || !blank(i.html ?? ''))
        itemsRef.current = loaded
        setItems(loaded)
        setView(e.doc.view)
      }
      setReady(true)
    })
    return () => {
      alive = false
    }
  }, [])

  // Tab title and icon.
  useEffect(() => {
    if (ready) setTabLook(title.trim() || t('easel.untitled'), docIcon('easel', hue))
  }, [ready, title, hue, t])

  // Deferred save of any change.
  useEffect(() => {
    if (!ready) return
    const timer = setTimeout(() => {
      void window.signetNotes?.save({ title: title.trim(), items, view }).then((ok) => setTooBig(!ok))
    }, SAVE_DELAY)
    return () => clearTimeout(timer)
  }, [ready, title, items, view])

  // Page closing: immediate save (text being typed included).
  const latest = useRef({ ready, title, items, view })
  latest.current = { ready, title, items, view }
  useEffect(() => {
    const flush = () => {
      const l = latest.current
      if (!l.ready) return
      const live = document.querySelector<HTMLElement>('.easel-text-edit')
      const id = live?.dataset.itemId
      const items = live && id ? l.items.map((i) => (i.id === id ? { ...i, html: cleanHtml(live.innerHTML) } : i)) : l.items
      void window.signetNotes?.save({ title: l.title.trim(), items, view: l.view })
    }
    const onHide = () => {
      if (document.visibilityState === 'hidden') flush()
    }
    window.addEventListener('pagehide', flush)
    document.addEventListener('visibilitychange', onHide)
    return () => {
      window.removeEventListener('pagehide', flush)
      document.removeEventListener('visibilitychange', onHide)
    }
  }, [])

  const toBoard = (clientX: number, clientY: number) => {
    const r = port.current!.getBoundingClientRect()
    const v = viewRef.current
    return { x: (clientX - r.left - v.x) / v.zoom, y: (clientY - r.top - v.y) / v.zoom }
  }
  const center = () => {
    const r = port.current!.getBoundingClientRect()
    return toBoard(r.left + r.width / 2, r.top + r.height / 2)
  }
  /** History: snapshots of the board (unchanged elements shared by reference). */
  const past = useRef<EaselItem[][]>([])
  const future = useRef<EaselItem[][]>([])
  /** Key of the last change: a same gesture or a same input creates only one entry. */
  const lastKey = useRef<string | null>(null)
  /** Applies a board change with a history entry (merged if the key is that of the previous change). */
  const change = (fn: (list: EaselItem[]) => EaselItem[], key?: string) => {
    const prev = itemsRef.current
    const next = fn(prev)
    if (next === prev) return
    if (!key || key !== lastKey.current) {
      past.current.push(prev)
      if (past.current.length > HISTORY_MAX) past.current.shift()
    }
    lastKey.current = key ?? null
    future.current = []
    itemsRef.current = next
    setItems(next)
  }
  /** Same content (same references): an identical snapshot changes nothing. */
  const sameList = (a: EaselItem[], b: EaselItem[]) => a.length === b.length && a.every((x, i) => x === b[i])
  const travel = (from: EaselItem[][], to: EaselItem[][]) => {
    const cur = itemsRef.current
    let target: EaselItem[] | undefined
    while ((target = from.pop()) && sameList(target, cur));
    if (!target) return
    to.push(cur)
    lastKey.current = null
    itemsRef.current = target
    setItems(target)
    // Selection and input: only what still exists.
    const ids = new Set(target.map((i) => i.id))
    setSelected((s) => (s && ids.has(s) ? s : null))
    setEditing((s) => (s && ids.has(s) ? s : null))
  }
  const undo = () => travel(past.current, future.current)
  const redo = () => travel(future.current, past.current)
  const patch = (id: string, p: Partial<EaselItem>, key?: string) => change((list) => list.map((i) => (i.id === id ? { ...i, ...p } : i)), key)
  const removeItem = (id: string) => {
    change((list) => list.filter((i) => i.id !== id))
    setSelected((s) => (s === id ? null : s))
    setEditing((s) => (s === id ? null : s))
  }

  /** Zoom around a screen point (the point under the pointer does not move). */
  const zoomAt = (clientX: number, clientY: number, k: number) => {
    const r = port.current!.getBoundingClientRect()
    const px = clientX - r.left
    const py = clientY - r.top
    stopGlide()
    rawZoom.current = null
    setView((v) => {
      const zoom = clamp(v.zoom * k, ZOOM_MIN, ZOOM_MAX)
      return { zoom, x: px - (px - v.x) * (zoom / v.zoom), y: py - (py - v.y) * (zoom / v.zoom) }
    })
  }
  /** Applies a zoom level around a viewport point (no limits: the caller decides). */
  const applyZoom = (px: number, py: number, zoom: number) =>
    setView((v) => ({ zoom, x: px - (px - v.x) * (zoom / v.zoom), y: py - (py - v.y) * (zoom / v.zoom) }))
  /** Wheel / pinch zoom: rubber-band resistance beyond the limits, spring back after a short silence. */
  const zoomWheel = (clientX: number, clientY: number, k: number) => {
    if (reducedMotion.matches) return zoomAt(clientX, clientY, k)
    stopGlide()
    const r = port.current!.getBoundingClientRect()
    const prev = rawZoom.current
    const raw = prev ?? { z: viewRef.current.zoom, px: 0, py: 0, timer: 0 }
    window.clearTimeout(raw.timer)
    raw.px = clientX - r.left
    raw.py = clientY - r.top
    raw.z = clamp(raw.z * k, ZOOM_MIN / 3, ZOOM_MAX * 3)
    // Rubber band on a logarithmic scale: a zoom is multiplicative.
    const limit = raw.z > ZOOM_MAX ? ZOOM_MAX : raw.z < ZOOM_MIN ? ZOOM_MIN : raw.z
    const shown = limit * Math.exp(rubberband(Math.log(raw.z / limit), 0.5))
    applyZoom(raw.px, raw.py, shown)
    raw.timer = window.setTimeout(() => {
      const z = viewRef.current.zoom
      const to = clamp(z, ZOOM_MIN, ZOOM_MAX)
      rawZoom.current = null
      if (to === z) return
      glide.current = [
        spring({
          from: z,
          to,
          response: 0.3,
          onUpdate: (v) => applyZoom(raw.px, raw.py, v),
          onDone: () => {}
        })
      ]
    }, WHEEL_SETTLE)
    rawZoom.current = raw
  }
  const zoomCenter = (k: number) => {
    const r = port.current!.getBoundingClientRect()
    zoomAt(r.left + r.width / 2, r.top + r.height / 2, k)
  }

  // Two-finger scroll: pan; pinch or ⌘ + scroll: zoom (active listener so it can be prevented).
  useEffect(() => {
    const el = port.current
    if (!el) return
    const onWheel = (e: WheelEvent) => {
      if (editing && (e.target as HTMLElement).closest('.easel-text.editing')) return
      e.preventDefault()
      if (e.ctrlKey || e.metaKey) zoomWheel(e.clientX, e.clientY, Math.exp(-e.deltaY * 0.01))
      else {
        stopGlide()
        setView((v) => ({ ...v, x: v.x - e.deltaX, y: v.y - e.deltaY }))
      }
    }
    el.addEventListener('wheel', onWheel, { passive: false })
    return () => el.removeEventListener('wheel', onWheel)
  })

  /** Adds an image (pasted, dropped or chosen) centered on a board point. */
  const addImage = async (file: Blob, at: { x: number; y: number }) => {
    const img = await readImage(file)
    if (!img) return
    // Cumulative size: several images from a single drop go through before the next render.
    if (JSON.stringify(itemsRef.current).length + addedSize.current + img.src.length > EASEL_MAX * 0.95) return setTooBig(true)
    addedSize.current += img.src.length
    const w = Math.min(img.width, 640)
    const h = Math.round((w * img.height) / img.width)
    const item: EaselItem = { id: uid(), type: 'image', x: at.x - w / 2, y: at.y - h / 2, w, h, src: img.src }
    change((list) => [...list, item])
    setSelected(item.id)
    setTool('select')
  }
  const addText = (at: { x: number; y: number }, html = '') => {
    const item: EaselItem = { id: uid(), type: 'text', x: at.x, y: at.y - 12, w: 260, h: 40, html, color: color === COLORS[0] ? undefined : color }
    // Text key: the typing that follows merges into the addition.
    change((list) => [...list, item], `text:${item.id}`)
    setSelected(item.id)
    setEditing(html ? null : item.id)
    setTool('select')
  }

  // Paste: image or text, at the center of the view (except while typing a text).
  useEffect(() => {
    const onPaste = (e: ClipboardEvent) => {
      const target = e.target instanceof HTMLElement ? e.target : null
      if (editing || target?.closest('input, [contenteditable="true"]')) return
      const file = [...(e.clipboardData?.files ?? [])].find((f) => f.type.startsWith('image/'))
      if (file) {
        e.preventDefault()
        void addImage(file, center())
        return
      }
      const text = e.clipboardData?.getData('text/plain')?.trim()
      if (text) {
        e.preventDefault()
        addText(center(), text.replace(/[&<>]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;' })[c] as string).replace(/\n/g, '<br>'))
      }
    }
    document.addEventListener('paste', onPaste)
    return () => document.removeEventListener('paste', onPaste)
  })

  // Keyboard: tools (V, T, P), delete the chosen element, Escape.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      // Escape during a move or a resize: the element returns to its original place.
      const d = drag.current
      if (e.key === 'Escape' && d && (d.kind === 'move' || d.kind === 'resize')) {
        e.preventDefault()
        // The cancelled gesture leaves no history entry.
        if (lastKey.current === `g:${d.item}`) {
          const before = past.current.pop()
          lastKey.current = null
          if (before) {
            itemsRef.current = before
            setItems(before)
          }
        } else patch(d.item, d.kind === 'move' ? { x: d.ox, y: d.oy } : { w: d.ow, h: d.oh })
        drag.current = null
        return
      }
      const target = e.target instanceof HTMLElement ? e.target : null
      if (target?.closest('input, [contenteditable="true"]')) {
        if (e.key === 'Escape') target.blur()
        return
      }
      // Undo / redo (text input keeps its native undo, above).
      if ((e.metaKey || e.ctrlKey) && !e.altKey && !e.isComposing) {
        const k = e.key.toLowerCase()
        if (k === 'z' || k === 'y') {
          e.preventDefault()
          if (k === 'y' || e.shiftKey) redo()
          else undo()
        }
        return
      }
      if (e.metaKey || e.ctrlKey || e.altKey) return
      if ((e.key === 'Backspace' || e.key === 'Delete') && selected) {
        e.preventDefault()
        removeItem(selected)
      } else if (e.key === 'Escape') {
        setSelected(null)
        setTool('select')
      } else if (e.key === 'v') setTool('select')
      else if (e.key === 't') setTool('text')
      else if (e.key === 'p') setTool('pen')
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  })

  // ───────── Pointer on the background: drawing, text, view pan ─────────
  const drag = useRef<
    | { kind: 'pan'; id: number; sx: number; sy: number; vx: number; vy: number }
    | { kind: 'draw'; id: number; points: number[] }
    | { kind: 'move'; id: number; item: string; sx: number; sy: number; ox: number; oy: number; active: boolean }
    | { kind: 'resize'; id: number; item: string; sx: number; ow: number; oh: number; keep: boolean }
    | null
  >(null)

  const onBackgroundDown = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (e.button !== 0) return
    const p = toBoard(e.clientX, e.clientY)
    if (tool === 'text') {
      // Without this, the default click action gives focus back to the background right after: the empty text, left at once, disappeared.
      e.preventDefault()
      addText(p)
      return
    }
    capture(e.currentTarget, e.pointerId)
    if (tool === 'pen') {
      drag.current = { kind: 'draw', id: e.pointerId, points: [p.x, p.y] }
      setDraft([p.x, p.y])
      return
    }
    setSelected(null)
    stopGlide()
    history.current = [{ t: performance.now(), x: view.x, y: view.y }]
    drag.current = { kind: 'pan', id: e.pointerId, sx: e.clientX, sy: e.clientY, vx: view.x, vy: view.y }
    setPanning(true)
  }
  const onMove = (e: ReactPointerEvent<HTMLDivElement>) => {
    const d = drag.current
    if (!d || d.id !== e.pointerId) return
    const z = viewRef.current.zoom
    if (d.kind === 'pan') {
      const x = d.vx + e.clientX - d.sx
      const y = d.vy + e.clientY - d.sy
      const now = performance.now()
      history.current = [...history.current.filter((h) => now - h.t <= VELOCITY_WINDOW), { t: now, x, y }]
      setView((v) => ({ ...v, x, y }))
    } else if (d.kind === 'draw') {
      const p = toBoard(e.clientX, e.clientY)
      const n = d.points.length
      // Points too close are ignored: the stroke stays light.
      if (Math.hypot(p.x - d.points[n - 2], p.y - d.points[n - 1]) * z < 2) return
      d.points.push(p.x, p.y)
      setDraft([...d.points])
    } else if (d.kind === 'move') {
      // The move only starts after a few pixels: a click does not shift the element.
      if (!d.active && Math.hypot(e.clientX - d.sx, e.clientY - d.sy) < MOVE_THRESHOLD) return
      d.active = true
      patch(d.item, { x: d.ox + (e.clientX - d.sx) / z, y: d.oy + (e.clientY - d.sy) / z }, `g:${d.item}`)
    } else if (d.kind === 'resize') {
      const w = Math.max(d.keep ? 20 : 80, d.ow + (e.clientX - d.sx) / z)
      patch(d.item, d.keep ? { w, h: (w * d.oh) / d.ow } : { w }, `g:${d.item}`)
    }
  }
  const onUp = (e: ReactPointerEvent<HTMLDivElement>) => {
    const d = drag.current
    if (!d || d.id !== e.pointerId) return
    drag.current = null
    setPanning(false)
    if (d.kind === 'pan' && !reducedMotion.matches) {
      // Glide: one spring per axis, launched at the release velocity.
      const now = performance.now()
      const h = history.current.filter((p) => now - p.t <= VELOCITY_WINDOW)
      const first = h[0]
      const last = h[h.length - 1]
      const dt = first && last ? (last.t - first.t) / 1000 : 0
      // Finger held still before letting go: no momentum.
      if (dt > 0.015 && now - last.t < 60) {
        const vx = (last.x - first.x) / dt
        const vy = (last.y - first.y) / dt
        const v = viewRef.current
        glide.current = (['x', 'y'] as const).map((axis) =>
          spring({
            from: v[axis],
            to: v[axis] + project(axis === 'x' ? vx : vy),
            velocity: axis === 'x' ? vx : vy,
            response: 0.6,
            onUpdate: (value) => setView((cur) => ({ ...cur, [axis]: value })),
            onDone: () => {}
          })
        )
      }
    }
    if (d.kind === 'draw') {
      setDraft(null)
      const xs = d.points.filter((_, i) => i % 2 === 0)
      const ys = d.points.filter((_, i) => i % 2 === 1)
      const pad = PEN_SIZE
      const x = Math.min(...xs) - pad
      const y = Math.min(...ys) - pad
      const w = Math.max(...xs) - Math.min(...xs) + pad * 2
      const h = Math.max(...ys) - Math.min(...ys) + pad * 2
      const points = d.points.map((v, i) => (i % 2 === 0 ? v - x : v - y))
      change((list) => [...list, { id: uid(), type: 'stroke', x, y, w, h, vw: w, vh: h, points, color, size: PEN_SIZE }])
    }
  }

  const onItemDown = (e: ReactPointerEvent<HTMLDivElement>, item: EaselItem) => {
    if (tool !== 'select' || e.button !== 0) return // pen and text: the background handles it (we draw on top)
    e.stopPropagation()
    stopGlide()
    setSelected(item.id)
    if (editing === item.id) return // input: the text cursor keeps control
    capture(port.current!, e.pointerId)
    lastKey.current = null
    drag.current = { kind: 'move', id: e.pointerId, item: item.id, sx: e.clientX, sy: e.clientY, ox: item.x, oy: item.y, active: false }
  }
  const onResizeDown = (e: ReactPointerEvent<HTMLDivElement>, item: EaselItem) => {
    e.stopPropagation()
    capture(port.current!, e.pointerId)
    lastKey.current = null
    drag.current = { kind: 'resize', id: e.pointerId, item: item.id, sx: e.clientX, ow: item.w, oh: item.h, keep: item.type !== 'text' }
  }

  const onDrop = (e: DragEvent<HTMLDivElement>) => {
    const files = [...e.dataTransfer.files].filter((f) => f.type.startsWith('image/'))
    if (!files.length) return
    e.preventDefault()
    const p = toBoard(e.clientX, e.clientY)
    files.forEach((f, i) => void addImage(f, { x: p.x + i * 24, y: p.y + i * 24 }))
  }

  const pickColor = (c: string) => {
    setColor(c)
    const item = items.find((i) => i.id === selected)
    if (item && item.type !== 'image') patch(item.id, { color: c === COLORS[0] && item.type === 'text' ? undefined : c })
  }

  const btn = (label: string, icon: ReactNode, run: () => void, on = false, disabled = false) => (
    <button type="button" className={`note-tool${on ? ' on' : ''}`} title={label} aria-label={label} aria-pressed={on} disabled={disabled} onClick={run}>
      {icon}
    </button>
  )

  const grid = 24 * view.zoom
  return (
    <div className="easel-page">
      <div className="easel-head">
        <input
          className="easel-title"
          value={title}
          placeholder={t('easel.untitled')}
          onChange={(e) => setTitle(e.target.value)}
          aria-label={t('easel.title')}
        />
        {confirmDelete ? (
          <span className="note-confirm">
            {t('easel.deleteConfirm')}
            <button type="button" onClick={() => setConfirmDelete(false)}>
              {t('common.cancel')}
            </button>
            <button type="button" className="danger" onClick={() => void window.signetNotes?.remove()}>
              {t('notes.delete')}
            </button>
          </span>
        ) : (
          btn(t('easel.deleteEasel'), <Trash2 size={15} />, () => setConfirmDelete(true))
        )}
      </div>
      <div className="easel-bar" role="toolbar" aria-label={t('easel.toolbar')}>
        {btn(t('easel.select'), <MousePointer2 size={15} />, () => setTool('select'), tool === 'select')}
        {btn(t('easel.text'), <Type size={15} />, () => setTool('text'), tool === 'text')}
        {btn(t('easel.pen'), <Pencil size={15} />, () => setTool('pen'), tool === 'pen')}
        {btn(t('easel.image'), <ImagePlus size={15} />, () => fileInput.current?.click())}
        <span className="note-sep" />
        {COLORS.map((c) => (
          <button
            key={c}
            type="button"
            className={`easel-color${c === color ? ' on' : ''}`}
            style={{ background: c }}
            title={t('easel.color')}
            aria-label={t('easel.color')}
            aria-pressed={c === color}
            onClick={() => pickColor(c)}
          />
        ))}
        <span className="note-sep" />
        {btn(t('easel.zoomOut'), <Minus size={15} />, () => zoomCenter(1 / 1.25))}
        <button type="button" className="easel-zoom" title={t('easel.zoomReset')} onClick={() => zoomCenter(1 / viewRef.current.zoom)}>
          {Math.round(view.zoom * 100)} %
        </button>
        {btn(t('easel.zoomIn'), <Plus size={15} />, () => zoomCenter(1.25))}
        <span className="note-sep" />
        {/* Delete the chosen element: only when there is one, and not with the trash (the top one deletes the whole Easel). */}
        {selected && btn(t('easel.deleteItem'), <Delete size={15} />, () => removeItem(selected))}
        {tooBig && <span className="note-warn">{t('easel.tooBig')}</span>}
        <input
          ref={fileInput}
          type="file"
          accept="image/*"
          multiple
          hidden
          onChange={(e) => {
            const at = center()
            ;[...(e.target.files ?? [])].forEach((f, i) => void addImage(f, { x: at.x + i * 24, y: at.y + i * 24 }))
            e.target.value = ''
          }}
        />
      </div>
      <div
        ref={port}
        className={`easel-port tool-${tool}${panning ? ' panning' : ''}`}
        style={{ backgroundSize: `${grid}px ${grid}px`, backgroundPosition: `${view.x}px ${view.y}px` }}
        onPointerDown={onBackgroundDown}
        onPointerMove={onMove}
        onPointerUp={onUp}
        onPointerCancel={onUp}
        onDoubleClick={(e) => {
          if (tool === 'select' && (e.target === e.currentTarget || (e.target as HTMLElement).classList.contains('easel-board')))
            addText(toBoard(e.clientX, e.clientY))
        }}
        onDragOver={(e) => e.dataTransfer.types.includes('Files') && e.preventDefault()}
        onDrop={onDrop}
      >
        <div className="easel-board" style={{ transform: `translate(${view.x}px, ${view.y}px) scale(${view.zoom})` }}>
          {items.map((item) => (
            <EaselItemView
              key={item.id}
              item={item}
              selected={selected === item.id}
              editing={editing === item.id}
              onDown={(e) => onItemDown(e, item)}
              onResize={(e) => onResizeDown(e, item)}
              onEdit={() => {
                if (item.type !== 'text') return
                lastKey.current = null
                setEditing(item.id)
              }}
              onLive={(html) => patch(item.id, { html }, `text:${item.id}`)}
              onCommit={(html) => {
                setEditing(null)
                const typed = lastKey.current === `text:${item.id}`
                lastKey.current = null
                if (!html.replace(/<br\s*\/?>|&nbsp;|\s/g, '')) {
                  // Text emptied while typing: it goes in the same history entry.
                  if (typed) {
                    itemsRef.current = itemsRef.current.filter((i) => i.id !== item.id)
                    setItems(itemsRef.current)
                    setSelected((s) => (s === item.id ? null : s))
                  } else removeItem(item.id)
                } else if (html !== item.html) patch(item.id, { html })
              }}
            />
          ))}
          {draft && (
            <svg className="easel-draft" width="1" height="1" overflow="visible">
              <path d={pathOf(draft)} fill="none" style={{ stroke: color }} strokeWidth={PEN_SIZE} strokeLinecap="round" strokeLinejoin="round" />
            </svg>
          )}
        </div>
        {ready && items.length === 0 && !draft && <div className="easel-empty">{t('easel.empty')}</div>}
      </div>
    </div>
  )
}

interface ItemProps {
  item: EaselItem
  selected: boolean
  editing: boolean
  onDown: (e: ReactPointerEvent<HTMLDivElement>) => void
  onResize: (e: ReactPointerEvent<HTMLDivElement>) => void
  onEdit: () => void
  onCommit: (html: string) => void
  onLive: (html: string) => void
}

/** One board element: text (editable by double-click), image or stroke; size handle when chosen. */
function EaselItemView({ item, selected, editing, onDown, onResize, onEdit, onCommit, onLive }: ItemProps) {
  const text = useRef<HTMLDivElement>(null)
  // Switch to input: content set once, cursor at the end.
  useLayoutEffect(() => {
    const el = text.current
    if (!editing || !el) return
    el.innerHTML = item.html ?? ''
    el.focus()
    const sel = getSelection()
    sel?.selectAllChildren(el)
    sel?.collapseToEnd()
    // eslint-disable-next-line react-hooks/exhaustive-deps -- only when entering input
  }, [editing])

  const style = { left: item.x, top: item.y, width: item.w, ...(item.type !== 'text' && { height: item.h }) }
  return (
    <div
      className={`easel-item easel-${item.type}${selected ? ' selected' : ''}${editing ? ' editing' : ''}`}
      style={style}
      onPointerDown={onDown}
      onDoubleClick={onEdit}
    >
      {item.type === 'text' &&
        (editing ? (
          <div
            ref={text}
            className="easel-text-edit"
            contentEditable
            suppressContentEditableWarning
            data-item-id={item.id}
            style={{ color: item.color }}
            onInput={(e) => onLive(cleanHtml(e.currentTarget.innerHTML))}
            onBlur={(e) => onCommit(cleanHtml(e.currentTarget.innerHTML))}
            onPaste={(e) => {
              const html = e.clipboardData.getData('text/html')
              if (!html) return
              e.preventDefault()
              document.execCommand('insertHTML', false, cleanHtml(html))
            }}
          />
        ) : (
          <div className="easel-text-view" style={{ color: item.color }} dangerouslySetInnerHTML={{ __html: item.html ?? '' }} />
        ))}
      {item.type === 'image' && <img src={item.src} alt="" draggable={false} />}
      {item.type === 'stroke' && (
        <svg width="100%" height="100%" viewBox={`0 0 ${item.vw ?? item.w} ${item.vh ?? item.h}`} preserveAspectRatio="none">
          <path
            d={pathOf(item.points ?? [])}
            fill="none"
            style={{ stroke: item.color ?? 'currentColor' }}
            strokeWidth={item.size ?? PEN_SIZE}
            strokeLinecap="round"
            strokeLinejoin="round"
          />
        </svg>
      )}
      {selected && !editing && <div className="easel-handle" onPointerDown={onResize} />}
    </div>
  )
}

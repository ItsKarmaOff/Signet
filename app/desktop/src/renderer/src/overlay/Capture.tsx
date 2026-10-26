import { useEffect, useRef, useState } from 'react'
import type { PointerEvent as ReactPointerEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { Check, Copy, Crop, Download, FileImage, Monitor, PenTool, X } from 'lucide-react'
import type { CaptureTarget, OverlayPayload, Rect } from '@shared/types'

type Payload = Extract<OverlayPayload, { mode: 'capture' }>
type Kind = CaptureTarget['kind']

/** Minimum size of an area (px): below it, it is a plain click. */
const MIN = 4

/**
 * Screenshot tool, placed on the tab's web view: drag to choose an area (or take the
 * visible part, or the whole page), then Copy (Enter) or Save. The rest of the window is dimmed.
 */
export function Capture({ payload }: { payload: Payload }) {
  const { t } = useTranslation()
  const { rect, tabId, hue } = payload
  const [kind, setKind] = useState<Kind>('region')
  const [sel, setSel] = useState<Rect | null>(null)
  const [busy, setBusy] = useState(false)
  const [done, setDone] = useState(false)
  const [failed, setFailed] = useState(false)
  const drag = useRef<{ x: number; y: number; id: number } | null>(null)

  const ready = kind !== 'region' || (sel !== null && sel.width >= MIN && sel.height >= MIN)
  const target = (): CaptureTarget => (kind === 'region' && sel ? { kind: 'region', rect: sel } : { kind: kind === 'full' ? 'full' : 'visible' })

  const take = async (action: 'copy' | 'save' | 'easel') => {
    if (!ready || busy) return
    setBusy(true)
    setFailed(false)
    const res = await window.signet.takeCapture(tabId, target(), action).catch(() => ({ ok: false, png: undefined }))
    if (action !== 'copy') {
      // Success: the main process closed the tool (save dialog, or new Easel). Failure: we hand control back.
      if (!res.ok) {
        setBusy(false)
        setFailed(true)
      }
      return
    }
    let ok = false
    if (res.ok && res.png) {
      try {
        // Manual decoding: the interface security policy forbids fetch() on a data: address.
        const bytes = Uint8Array.from(atob(res.png.slice(res.png.indexOf(',') + 1)), (c) => c.charCodeAt(0))
        const blob = new Blob([bytes], { type: 'image/png' })
        await navigator.clipboard.write([new ClipboardItem({ 'image/png': blob })])
        ok = true
      } catch (error) {
        console.error('Capture : presse-papiers', error instanceof Error ? `${error.name}: ${error.message}` : error)
      }
    }
    setBusy(false)
    if (!ok) {
      setFailed(true)
      return
    }
    setDone(true)
    setTimeout(() => void window.signet.closeOverlay(), 650)
  }

  // Enter: copy.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Enter') {
        e.preventDefault()
        void take('copy')
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  })

  /** Point in the web view, clamped to its bounds. */
  const local = (e: ReactPointerEvent) => ({
    x: Math.min(rect.width, Math.max(0, e.clientX - rect.x)),
    y: Math.min(rect.height, Math.max(0, e.clientY - rect.y))
  })
  const down = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (e.button !== 0 || done) return
    e.currentTarget.setPointerCapture(e.pointerId)
    const p = local(e)
    drag.current = { ...p, id: e.pointerId }
    setKind('region')
    setSel({ x: p.x, y: p.y, width: 0, height: 0 })
  }
  const move = (e: ReactPointerEvent<HTMLDivElement>) => {
    const d = drag.current
    if (!d || d.id !== e.pointerId) return
    const p = local(e)
    setSel({ x: Math.min(d.x, p.x), y: Math.min(d.y, p.y), width: Math.abs(p.x - d.x), height: Math.abs(p.y - d.y) })
  }
  const up = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (drag.current?.id !== e.pointerId) return
    drag.current = null
    setSel((s) => (s && s.width >= MIN && s.height >= MIN ? s : null))
  }

  // Highlighted area: the selection, or the whole view for "visible part" and "whole page".
  const hole = kind === 'region' ? sel : { x: 0, y: 0, width: rect.width, height: rect.height }
  const pick = (k: Kind) => {
    setKind(k)
    if (k !== 'region') setSel(null)
  }

  return (
    <div className="capture" style={{ ['--h' as string]: hue }}>
      {/* Dim layer over the whole window, cut out at the chosen spot (giant drop shadow around the hole). */}
      {!hole || hole.width < MIN ? (
        <div className="capture-veil" />
      ) : (
        <div
          className={`capture-hole${kind !== 'region' ? ' whole' : ''}`}
          style={{ left: rect.x + hole.x, top: rect.y + hole.y, width: hole.width, height: hole.height }}
        >
          {kind === 'region' && (
            <span className="capture-size">
              {Math.round(hole.width)} × {Math.round(hole.height)}
            </span>
          )}
          {kind === 'full' && <span className="capture-size">{t('capture.fullPageHint')}</span>}
        </div>
      )}
      <div
        className="capture-area"
        style={{ left: rect.x, top: rect.y, width: rect.width, height: rect.height }}
        onPointerDown={down}
        onPointerMove={move}
        onPointerUp={up}
        onPointerCancel={up}
      >
        {kind === 'region' && !sel && <div className="capture-hint">{t('capture.dragHint')}</div>}
      </div>
      <div className="capture-bar" style={{ left: rect.x + rect.width / 2, top: rect.y + rect.height - 16 }} role="toolbar" aria-label={t('capture.title')}>
        {done ? (
          <span className="capture-done" role="status">
            <Check size={15} strokeWidth={2.4} /> {t('capture.copied')}
          </span>
        ) : (
          <>
            <div className="capture-seg" role="radiogroup">
              <button role="radio" aria-checked={kind === 'region'} className={kind === 'region' ? 'on' : ''} onClick={() => pick('region')}>
                <Crop size={14} /> {t('capture.region')}
              </button>
              <button role="radio" aria-checked={kind === 'visible'} className={kind === 'visible' ? 'on' : ''} onClick={() => pick('visible')}>
                <Monitor size={14} /> {t('capture.visible')}
              </button>
              <button role="radio" aria-checked={kind === 'full'} className={kind === 'full' ? 'on' : ''} onClick={() => pick('full')}>
                <FileImage size={14} /> {t('capture.full')}
              </button>
            </div>
            <span className="capture-sep" />
            {failed && (
              <span className="capture-done" role="alert">
                {t('capture.failed')}
              </span>
            )}
            <button className="capture-btn primary" disabled={!ready || busy} onClick={() => void take('copy')} title={t('capture.copyTitle')}>
              <Copy size={14} /> {t('capture.copy')}
            </button>
            <button className="capture-btn" disabled={!ready || busy} onClick={() => void take('save')}>
              <Download size={14} /> {t('capture.save')}
            </button>
            <button className="capture-btn" disabled={!ready || busy} onClick={() => void take('easel')} title={t('capture.easelTitle')}>
              <PenTool size={14} /> {t('capture.easel')}
            </button>
            <button
              className="capture-btn icon-only"
              onClick={() => void window.signet.closeOverlay()}
              title={t('capture.cancel')}
              aria-label={t('capture.cancel')}
            >
              <X size={15} />
            </button>
          </>
        )}
      </div>
    </div>
  )
}

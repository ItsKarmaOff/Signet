import { useEffect, useRef, useState } from 'react'
import type { PointerEvent as ReactPointerEvent } from 'react'
import { ArrowUpLeft, Pause, Play, RotateCcw, RotateCw, Volume2, VolumeX, X } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import type { PipState } from '@shared/types'
import { useHue } from '../lib/util'

const HANDLES = ['n', 's', 'e', 'w', 'ne', 'nw', 'se', 'sw'] as const

/**
 * Invisible resize handle. The gesture is computed from the pointer's ABSOLUTE (screen) positions: start
 * stored once, then total movement since that start, batched per frame; main applies the geometry.
 */
function Grip({ handle }: { handle: (typeof HANDLES)[number] }) {
  const g = useRef<{ x: number; y: number; dx: number; dy: number; raf: number } | null>(null)
  const flush = () => {
    const s = g.current
    if (!s) return
    s.raf = 0
    window.signetPip?.resize('move', handle, s.dx, s.dy)
  }
  const stop = (e: ReactPointerEvent<HTMLDivElement>) => {
    const s = g.current
    if (!s) return
    g.current = null
    if (s.raf) cancelAnimationFrame(s.raf)
    window.signetPip?.resize('move', handle, e.screenX - s.x, e.screenY - s.y)
    window.signetPip?.resize('end')
  }
  return (
    <div
      className={`pip-grip pip-grip-${handle}`}
      onPointerDown={(e) => {
        if (e.button !== 0) return
        e.stopPropagation()
        e.currentTarget.setPointerCapture(e.pointerId)
        g.current = { x: e.screenX, y: e.screenY, dx: 0, dy: 0, raf: 0 }
        window.signetPip?.resize('start')
      }}
      onPointerMove={(e) => {
        const s = g.current
        if (!s) return
        s.dx = e.screenX - s.x
        s.dy = e.screenY - s.y
        if (!s.raf) s.raf = requestAnimationFrame(flush)
      }}
      onPointerUp={stop}
      onPointerCancel={stop}
      onLostPointerCapture={stop}
    />
  )
}

/** Route #pip: mini-player controls, laid over the tab's page (transparent background). */
export function Pip() {
  const { t } = useTranslation()
  const [st, setSt] = useState<PipState | null>(null)
  /** Fraction while dragging on the bar (the real position follows with a delay). */
  const [scrub, setScrub] = useState<number | null>(null)
  const sentAt = useRef(0)
  const scrubbing = useRef(false)
  useHue(st?.hue)

  useEffect(() => window.signet.onPipState(setSt), [])

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.metaKey || e.ctrlKey || e.altKey) return
      const signet = window.signet
      if (e.key === ' ') void signet.pipCommand('toggle')
      else if (e.key === 'ArrowLeft') void signet.pipCommand('back')
      else if (e.key === 'ArrowRight') void signet.pipCommand('forward')
      else if (e.key === 'm' || e.key === 'M') void signet.pipCommand('mute')
      else if (e.key === 'Escape') void signet.pipCommand('close')
      else return
      e.preventDefault()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [])

  const signet = window.signet
  const finite = !!st && Number.isFinite(st.duration) && st.duration > 0
  const progress = scrub ?? (finite ? Math.min(1, Math.max(0, st.currentTime / st.duration)) : 0)

  const seekTo = (e: ReactPointerEvent<HTMLDivElement>, final: boolean) => {
    if (!st || !finite) return
    const r = e.currentTarget.getBoundingClientRect()
    const f = Math.min(1, Math.max(0, (e.clientX - r.left) / r.width))
    setScrub(f)
    const now = Date.now()
    if (final || now - sentAt.current > 60) {
      sentAt.current = now
      void signet.pipCommand('seek', f * st.duration)
    }
  }

  return (
    <div
      className="pip"
      // Hover handled here: the view receives cursor enter and leave even when the window is not active.
      onPointerEnter={() => document.documentElement.classList.add('hover')}
      onPointerLeave={() => document.documentElement.classList.remove('hover')}
      onPointerDown={(e) => {
        // Drag the window from anywhere outside the buttons and the bar.
        if (e.button !== 0 || (e.target as HTMLElement).closest('button, .pip-bar')) return
        e.currentTarget.setPointerCapture(e.pointerId)
        window.signetPip?.drag(true)
      }}
      onPointerUp={() => window.signetPip?.drag(false)}
      onPointerCancel={() => window.signetPip?.drag(false)}
    >
      <div className="pip-scrim" />
      {HANDLES.map((h) => (
        <Grip key={h} handle={h} />
      ))}
      <div className="pip-top">
        <button className="pip-pill pip-square" title={t('pip.return')} aria-label={t('pip.return')} onClick={() => signet.pipCommand('return')}>
          <ArrowUpLeft size={16} strokeWidth={2.2} />
        </button>
        <div className="pip-site">{st?.site}</div>
        <button className="pip-pill pip-close" onClick={() => signet.pipCommand('close')}>
          <X size={14} strokeWidth={2.4} />
          <span>{t('pip.close')}</span>
        </button>
      </div>
      <div className="pip-center">
        <button className="pip-skip" title={t('pip.back')} aria-label={t('pip.back')} onClick={() => signet.pipCommand('back')}>
          <RotateCcw size={30} strokeWidth={1.8} />
          <span>15</span>
        </button>
        <button
          className="pip-play"
          title={st?.playing ? t('pip.pause') : t('pip.play')}
          aria-label={st?.playing ? t('pip.pause') : t('pip.play')}
          onClick={() => signet.pipCommand('toggle')}
        >
          {st?.playing ? <Pause size={40} fill="currentColor" strokeWidth={1.5} /> : <Play size={40} fill="currentColor" strokeWidth={1.5} />}
        </button>
        <button className="pip-skip" title={t('pip.forward')} aria-label={t('pip.forward')} onClick={() => signet.pipCommand('forward')}>
          <RotateCw size={30} strokeWidth={1.8} />
          <span>15</span>
        </button>
      </div>
      <div className="pip-bottom">
        <div
          className={`pip-bar${finite ? '' : ' live'}`}
          role="slider"
          aria-label={t('pip.seek')}
          aria-valuenow={Math.round(progress * 100)}
          onPointerDown={(e) => {
            e.currentTarget.setPointerCapture(e.pointerId)
            scrubbing.current = true
            seekTo(e, false)
          }}
          onPointerMove={(e) => e.currentTarget.hasPointerCapture(e.pointerId) && seekTo(e, false)}
          onPointerUp={(e) => {
            scrubbing.current = false
            seekTo(e, true)
            e.currentTarget.releasePointerCapture(e.pointerId)
            setTimeout(() => setScrub(null), 400)
          }}
          // Gesture interrupted (cancelled by the system, capture lost): the bar takes the real position back.
          onPointerCancel={() => {
            scrubbing.current = false
            setScrub(null)
          }}
          onLostPointerCapture={() => {
            if (!scrubbing.current) return
            scrubbing.current = false
            setScrub(null)
          }}
        >
          <div className="pip-track">
            <div className="pip-fill" style={{ transform: `scaleX(${Math.min(1, Math.max(0, progress))})` }} />
          </div>
        </div>
        <button
          className="pip-mute"
          title={st?.muted ? t('pip.unmute') : t('pip.mute')}
          aria-label={st?.muted ? t('pip.unmute') : t('pip.mute')}
          onClick={() => signet.pipCommand('mute')}
        >
          {st?.muted ? <VolumeX size={18} /> : <Volume2 size={18} />}
        </button>
      </div>
    </div>
  )
}

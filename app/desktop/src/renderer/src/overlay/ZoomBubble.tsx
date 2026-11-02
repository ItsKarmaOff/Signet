import { useEffect, useRef } from 'react'
import { useTranslation } from 'react-i18next'
import { Minus, Plus } from 'lucide-react'
import type { OverlayPayload } from '@shared/types'
import { ZOOM_PERCENTS, formatZoomPercent } from '@shared/pagesettings'

type Bubble = Extract<OverlayPayload, { mode: 'zoom' }>

/** Time without interaction before the bubble goes away. */
export const ZOOM_BUBBLE_MS = 2000
/** Shadow margin around the bubble (= PW_MARGIN in main/browser/overlay.ts). */
const MARGIN = 24
const STEP = 0.5

/**
 * Zoom bubble (route #overlay, mode `zoom`) at the top right of the page, like Chrome's. The overlay covers only the bubble:
 * the page stays usable. It hides itself after a while, unless the pointer is on it; a new payload (zoom changed) restarts the wait.
 */
export function ZoomBubble({ payload }: { payload: Bubble }) {
  const { t, i18n } = useTranslation()
  const { tabId, percent, defaultPercent } = payload
  const hovering = useRef(false)
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null)
  const arm = () => {
    if (timer.current) clearTimeout(timer.current)
    timer.current = setTimeout(() => {
      timer.current = null
      if (!hovering.current) void window.signet.closeOverlay()
    }, ZOOM_BUBBLE_MS)
  }
  useEffect(() => {
    arm()
    return () => {
      if (timer.current) clearTimeout(timer.current)
    }
  }, [payload])
  const zoom = (delta: number) => void window.signet.zoomPage(tabId, delta)
  const value = formatZoomPercent(percent, i18n.language)
  return (
    <div className="site-layer">
      <div
        className="site-panel zoom-bubble"
        style={{ left: MARGIN, top: MARGIN, width: 248, transformOrigin: 'top right' }}
        role="group"
        aria-label={t('zoom.label', { value })}
        onPointerEnter={() => (hovering.current = true)}
        onPointerLeave={() => {
          hovering.current = false
          arm()
        }}
      >
        <button
          className="site-btn-s zoom-step"
          title={t('zoom.out')}
          aria-label={t('zoom.out')}
          disabled={percent <= ZOOM_PERCENTS[0]}
          onClick={() => zoom(-STEP)}
        >
          <Minus size={14} />
        </button>
        <span className="zoom-value" aria-live="polite">
          {value}
        </span>
        <button
          className="site-btn-s zoom-step"
          title={t('zoom.in')}
          aria-label={t('zoom.in')}
          disabled={percent >= ZOOM_PERCENTS[ZOOM_PERCENTS.length - 1]}
          onClick={() => zoom(STEP)}
        >
          <Plus size={14} />
        </button>
        <button className="site-btn-s zoom-reset" disabled={percent === defaultPercent} onClick={() => zoom(0)}>
          {t('zoom.reset')}
        </button>
      </div>
    </div>
  )
}

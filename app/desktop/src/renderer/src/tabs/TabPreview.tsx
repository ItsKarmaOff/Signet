import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Moon, Volume2 } from 'lucide-react'
import type { TabPreviewPayload } from '@shared/types'
import { domainOf } from '../lib/util'

/**
 * Route #preview: preview card of a hovered tab (thumbnail, title, address, state), placed by main next to the
 * row. It appears from the bar's side (6 px slide + fade) and leaves the same way.
 */
export function TabPreview() {
  const { t } = useTranslation()
  const [p, setP] = useState<TabPreviewPayload | null>(null)
  const [on, setOn] = useState(false)

  useEffect(() => {
    const signet = window.signet
    let frame = 0
    const offShow = signet.onTabPreview((next) => {
      cancelAnimationFrame(frame)
      setP(next)
      // Appearance: start from the hidden state, then switch on the next frame (the CSS transition does the rest).
      if (next.fresh) {
        setOn(false)
        frame = requestAnimationFrame(() => (frame = requestAnimationFrame(() => setOn(true))))
      } else setOn(true)
    })
    const offHide = signet.onTabPreviewHide(() => {
      cancelAnimationFrame(frame)
      setOn(false)
    })
    return () => {
      cancelAnimationFrame(frame)
      offShow()
      offHide()
    }
  }, [])

  if (!p) return null
  const domain = domainOf(p.url) || p.url
  return (
    <div className={`tab-preview from-${p.from}${on ? ' on' : ''}`} role="tooltip">
      {p.thumb && (
        <div className="tp-thumb">
          <img src={p.thumb} alt="" draggable={false} />
        </div>
      )}
      <div className="tp-text">
        <div className="tp-title">
          {p.favicon && <img className="tp-favicon" src={p.favicon} alt="" draggable={false} />}
          <span>{p.title}</span>
        </div>
        <div className="tp-meta">
          <span className="tp-domain">{domain}</span>
          {p.audible && (
            <span className="tp-badge">
              <Volume2 size={12} /> {t('preview.playing')}
            </span>
          )}
          {p.asleep && (
            <span className="tp-badge">
              <Moon size={12} /> {t('preview.asleep')}
            </span>
          )}
        </div>
      </div>
    </div>
  )
}

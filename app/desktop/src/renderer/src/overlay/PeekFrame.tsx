import { Maximize2, X } from 'lucide-react'
import type { OverlayPayload } from '@shared/types'
import { useTranslation } from 'react-i18next'
import type { CSSProperties } from 'react'

type Peek = Extract<OverlayPayload, { mode: 'peek' }>

/** Scrim, card shadow and buttons; the web view sits on top of the card. `closing`: animated exit (the main process keeps the view while it fades out). */
export function PeekFrame({ payload, closing }: { payload: Peek; closing: boolean }) {
  const { t } = useTranslation()
  const { card, title, url, origin } = payload
  const barTop = card.y - 36
  // From a link: the card and the bar grow around the clicked point; otherwise around the card center.
  const cardOrigin = origin ? `${origin.x - card.x}px ${origin.y - card.y}px` : undefined
  const barOrigin = origin ? `${origin.x - card.x}px ${origin.y - barTop}px` : `50% ${card.y + card.height / 2 - barTop}px`
  return (
    <div className={`peek${closing ? ' closing' : ''}`} style={{ '--peek-s': origin ? 0.25 : 0.95 } as CSSProperties}>
      <div className="veil dark" onMouseDown={() => window.signet.closeOverlay()} />
      <div className="peek-card" style={{ left: card.x, top: card.y, width: card.width, height: card.height, transformOrigin: cardOrigin }} />
      <div className="peek-bar" style={{ left: card.x, top: barTop, width: card.width, transformOrigin: barOrigin }}>
        <span className="peek-title">{title || url}</span>
        <button className="round" title={t('peek.openInTab')} onClick={() => window.signet.expandPeek()}>
          <Maximize2 size={14} />
        </button>
        <button className="round" title={t('common.close')} onClick={() => window.signet.closeOverlay()}>
          <X size={15} />
        </button>
      </div>
    </div>
  )
}

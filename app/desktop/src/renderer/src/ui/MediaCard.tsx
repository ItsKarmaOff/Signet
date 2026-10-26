import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Pause, Play, SkipBack, SkipForward, Volume2, VolumeX } from 'lucide-react'
import type { MediaInfo, TabInfo } from '@shared/types'
import { Favicon } from './Favicon'
import { useExit } from '../lib/useExit'

/** Sidebar media card: the tab playing elsewhere, with its controls. It fades out before leaving the page. */
export function MediaCard({ media, tabs }: { media: MediaInfo | null; tabs: Record<string, TabInfo> }) {
  const { value, closing } = useExit(media)
  if (!value) return null
  return (
    <div className={`media-slot${closing ? ' closing' : ''}`}>
      <div className="media-slot-in">
        <MediaCardView key={value.tabId} media={value} tab={tabs[value.tabId]} closing={closing} />
      </div>
    </div>
  )
}

function MediaCardView({ media, tab, closing }: { media: MediaInfo; tab?: TabInfo; closing: boolean }) {
  const { t } = useTranslation()
  const signet = window.signet
  const [failed, setFailed] = useState<string | null>(null)
  const stop = (e: { stopPropagation(): void }) => e.stopPropagation()
  const cover = media.artwork && failed !== media.artwork ? media.artwork : undefined

  return (
    <div className={`media-card${closing ? ' closing' : ''}`} role="group" aria-label={t('media.label')} onClick={() => signet.activateTab(media.tabId)}>
      <div className="media-art">
        {cover ? <img className="cover" src={cover} alt="" draggable={false} onError={() => setFailed(cover)} /> : <Favicon src={tab?.favicon} size={18} />}
      </div>
      <div className="media-text">
        <b title={media.title}>{media.title}</b>
        <span>{media.artist || media.site}</span>
      </div>
      <div className="media-ctrl">
        {media.canSkip && (
          <button className="icon" title={t('media.previous')} onClick={(e) => (stop(e), signet.mediaSkip(media.tabId, 'previous'))}>
            <SkipBack size={13} />
          </button>
        )}
        <button className="icon" title={media.playing ? t('media.pause') : t('media.play')} onClick={(e) => (stop(e), signet.mediaToggle(media.tabId))}>
          {media.playing ? <Pause size={14} /> : <Play size={14} />}
        </button>
        {media.canSkip && (
          <button className="icon" title={t('media.next')} onClick={(e) => (stop(e), signet.mediaSkip(media.tabId, 'next'))}>
            <SkipForward size={13} />
          </button>
        )}
        <button
          className={`icon${media.muted ? ' on' : ''}`}
          title={media.muted ? t('media.unmute') : t('media.mute')}
          onClick={(e) => (stop(e), signet.toggleMute(media.tabId))}
        >
          {media.muted ? <VolumeX size={13} /> : <Volume2 size={13} />}
        </button>
      </div>
    </div>
  )
}

/** Compact version of the media card for the tab row: cover art or favicon, play / pause. */
export function MediaChip({ media, tabs }: { media: MediaInfo | null; tabs: Record<string, TabInfo> }) {
  const { value, closing } = useExit(media)
  if (!value) return null
  return (
    <div className={`media-slot h${closing ? ' closing' : ''}`}>
      <div className="media-slot-in">
        <MediaChipView key={value.tabId} media={value} tab={tabs[value.tabId]} closing={closing} />
      </div>
    </div>
  )
}

function MediaChipView({ media, tab, closing }: { media: MediaInfo; tab?: TabInfo; closing: boolean }) {
  const { t } = useTranslation()
  const signet = window.signet
  const [failed, setFailed] = useState<string | null>(null)
  const cover = media.artwork && failed !== media.artwork ? media.artwork : undefined
  const label = media.artist ? `${media.title} - ${media.artist}` : media.title
  return (
    <div
      className={`media-chip${closing ? ' closing' : ''}`}
      role="group"
      aria-label={t('media.label')}
      title={label}
      onClick={() => signet.activateTab(media.tabId)}
    >
      <span className="media-chip-art">
        {cover ? <img className="cover" src={cover} alt="" draggable={false} onError={() => setFailed(cover)} /> : <Favicon src={tab?.favicon} size={16} />}
      </span>
      <button
        className="icon"
        title={media.playing ? t('media.pause') : t('media.play')}
        onClick={(e) => (e.stopPropagation(), signet.mediaToggle(media.tabId))}
      >
        {media.playing ? <Pause size={14} /> : <Play size={14} />}
      </button>
    </div>
  )
}

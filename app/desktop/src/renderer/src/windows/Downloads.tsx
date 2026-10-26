import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { Download, File, FileArchive, FileAudio, FileImage, FileText, FileVideo, RotateCw, Search, X } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { OS_CONTEXT } from '@signet/shared/i18n'
import type { TFunction } from 'i18next'
import type { DownloadInfo } from '@shared/types'

const KEY = 'signet.downloads.seen'
export const live = (d: DownloadInfo) => d.status === 'progressing' || d.status === 'paused'

/** Size with the language's units (Intl), base 1024. */
export function formatBytes(n: number, locale: string): string {
  const units = ['byte', 'kilobyte', 'megabyte', 'gigabyte', 'terabyte']
  let v = n
  let i = 0
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024
    i++
  }
  return new Intl.NumberFormat(locale, { style: 'unit', unit: units[i], unitDisplay: 'short', maximumFractionDigits: i === 0 ? 0 : 1 }).format(v)
}

function subtitle(d: DownloadInfo, t: TFunction, locale: string): string {
  const fmt = (n: number) => formatBytes(n, locale)
  switch (d.status) {
    case 'progressing':
    case 'paused': {
      const base = d.totalBytes > 0 ? t('downloads.progress', { received: fmt(d.receivedBytes), total: fmt(d.totalBytes) }) : fmt(d.receivedBytes)
      return d.status === 'paused' ? t('downloads.paused', { size: base }) : base
    }
    case 'completed':
      return t('downloads.completed', { size: fmt(d.totalBytes || d.receivedBytes) })
    case 'cancelled':
      return t('downloads.cancelled')
    default:
      return t('downloads.interrupted')
  }
}

function iconFor(name: string) {
  const ext = name.split('.').pop()?.toLowerCase() ?? ''
  if (['png', 'jpg', 'jpeg', 'gif', 'webp', 'svg', 'heic', 'avif', 'bmp', 'tiff'].includes(ext)) return FileImage
  if (['zip', 'gz', 'tgz', 'tar', 'rar', '7z', 'dmg', 'pkg', 'bz2', 'xz'].includes(ext)) return FileArchive
  if (['mp4', 'mov', 'mkv', 'avi', 'webm', 'm4v'].includes(ext)) return FileVideo
  if (['mp3', 'wav', 'flac', 'aac', 'm4a', 'ogg', 'aiff'].includes(ext)) return FileAudio
  if (['pdf', 'txt', 'md', 'doc', 'docx', 'rtf', 'csv', 'xls', 'xlsx', 'ppt', 'pptx', 'pages', 'numbers', 'key', 'json'].includes(ext)) return FileText
  return File
}

/** Name truncated in the middle: the start is abbreviated, the end (extension) stays visible. */
function MidName({ name }: { name: string }) {
  const cut = Math.max(name.length - 8, 0)
  return (
    <span className="mid" title={name}>
      <span className="a">{name.slice(0, cut)}</span>
      <span className="b">{name.slice(cut)}</span>
    </span>
  )
}

/** Finished downloads the user has not seen yet (local state, persisted when possible). */
export function useUnseenDownloads(downloads: DownloadInfo[], open: boolean): boolean {
  const read = (): Set<string> | null => {
    try {
      const raw = localStorage.getItem(KEY)
      return raw ? new Set(JSON.parse(raw) as string[]) : null
    } catch {
      return null
    }
  }
  const [seen, setSeen] = useState<Set<string>>(() => read() ?? new Set(downloads.filter((d) => d.status === 'completed').map((d) => d.id)))

  const mark = () => {
    const missing = downloads.filter((d) => d.status === 'completed' && !seen.has(d.id))
    if (missing.length === 0) return
    const next = new Set(seen)
    for (const d of missing) next.add(d.id)
    setSeen(next)
    try {
      localStorage.setItem(KEY, JSON.stringify([...next].slice(-200)))
    } catch {
      /* storage unavailable: the in-memory state is enough */
    }
  }
  // Open: everything that finishes in the meantime counts as seen.
  useEffect(() => {
    if (open) mark()
  })
  // Sync with the other instance of the sidebar (window / floating).
  useEffect(() => {
    const on = (e: StorageEvent) => {
      if (e.key !== KEY) return
      const v = read()
      if (v) setSeen(v)
    }
    window.addEventListener('storage', on)
    return () => window.removeEventListener('storage', on)
  }, [])

  return !open && downloads.some((d) => d.status === 'completed' && !seen.has(d.id))
}

/** Detects the arrival of a new download: an id absent from the previous snapshot (the first snapshot does not count). */
export function useNewDownload(downloads: DownloadInfo[]): { key: number; d: DownloadInfo } | null {
  const known = useRef<Set<string> | null>(null)
  const [fresh, setFresh] = useState<{ key: number; d: DownloadInfo } | null>(null)
  useEffect(() => {
    const before = known.current
    known.current = new Set(downloads.map((d) => d.id))
    if (!before) return
    const added = downloads.find((d) => !before.has(d.id) && live(d))
    if (added) setFresh((f) => ({ key: (f?.key ?? 0) + 1, d: added }))
  }, [downloads])
  return fresh
}

/** Dot that leaves the center of the web card and arcs to the downloads button. */
/** `left` and `right`: margins of the pages area (the sidebar on one side, the window margin on the other). */
export function DownloadFlight({ d, left, right = 8, onLand }: { d: DownloadInfo; left: number; right?: number; onLand: () => void }) {
  const [path, setPath] = useState<string | null>(null)
  useLayoutEffect(() => {
    const btn = document.querySelector('.dl-btn')?.getBoundingClientRect()
    const x1 = btn ? btn.left + btn.width / 2 : left - 28
    const y1 = btn ? btn.top + btn.height / 2 : 22
    const x0 = left + (window.innerWidth - left - right) / 2
    const y0 = 44 + (window.innerHeight - 52) / 2
    setPath(`path('M ${x0} ${y0} Q ${x0} ${Math.max(y1 - 40, -10)} ${x1} ${y1}')`)
  }, [left, right])
  if (!path) return null
  const Icon = iconFor(d.filename)
  return (
    <div className="dl-flight" style={{ offsetPath: path }} onAnimationEnd={onLand}>
      <Icon size={13} />
    </div>
  )
}

/** Bottom bar button: progress ring and "unseen" dot (the rendering is in Sidebar). */
export function progressOf(downloads: DownloadInfo[]): { active: boolean; fraction: number | null } {
  const running = downloads.filter(live)
  if (running.length === 0) return { active: false, fraction: null }
  if (running.some((d) => d.totalBytes <= 0)) return { active: true, fraction: null }
  const total = running.reduce((a, d) => a + d.totalBytes, 0)
  const got = running.reduce((a, d) => a + d.receivedBytes, 0)
  return { active: true, fraction: total > 0 ? Math.min(1, got / total) : null }
}

/** A download row (sidebar panel and Library). */
export function DownloadRow({ d }: { d: DownloadInfo }) {
  const { t, i18n } = useTranslation()
  const signet = window.signet
  const Icon = iconFor(d.filename)
  const running = live(d)
  const done = d.status === 'completed'
  const pct = d.totalBytes > 0 ? Math.min(100, (d.receivedBytes / d.totalBytes) * 100) : null
  const Main = done ? 'button' : 'div'
  return (
    <div className={`dl-row${done ? ' done' : ''}${d.status === 'cancelled' || d.status === 'interrupted' ? ' dead' : ''}`}>
      {/* Finished: the main area is a real button (keyboard, screen reader) that opens the file. */}
      <Main className="dl-main" {...(done ? { type: 'button' as const, onClick: () => void signet.openDownload(d.id) } : {})}>
        <Icon size={18} className="dl-ico" />
        <div className="dl-text">
          <MidName name={d.filename} />
          <span className="dl-sub">{subtitle(d, t, i18n.language)}</span>
          {running && (
            <span className={`dl-bar${pct === null ? ' indeterminate' : ''}`}>
              <i style={pct === null ? undefined : { transform: `scaleX(${pct / 100})` }} />
            </span>
          )}
        </div>
      </Main>
      {(d.status === 'interrupted' || d.status === 'cancelled') && (
        <button
          className="mini"
          title={t('common.retry')}
          aria-label={t('downloads.retry', { name: d.filename })}
          onClick={() => void signet.retryDownload(d.id)}
        >
          <RotateCw size={13} />
        </button>
      )}
      {done && (
        <button className="mini" title={t('downloads.reveal', { context: OS_CONTEXT })} onClick={(e) => (e.stopPropagation(), signet.revealDownload(d.id))}>
          <Search size={13} />
        </button>
      )}
      {running && (
        <button className="mini" title={t('downloads.cancel')} onClick={(e) => (e.stopPropagation(), signet.cancelDownload(d.id))}>
          <X size={13} />
        </button>
      )}
    </div>
  )
}

export function DownloadsPanel({ downloads, onClose }: { downloads: DownloadInfo[]; onClose: () => void }) {
  const { t } = useTranslation()
  const signet = window.signet
  const ref = useRef<HTMLDivElement>(null)

  // Click outside (outside the open button) or Escape.
  useEffect(() => {
    const down = (e: PointerEvent) => {
      const t = e.target as Element | null
      if (!t?.closest('.dl-panel, .dl-btn')) onClose()
    }
    const key = (e: KeyboardEvent) => e.key === 'Escape' && onClose()
    document.addEventListener('pointerdown', down, true)
    document.addEventListener('keydown', key)
    return () => {
      document.removeEventListener('pointerdown', down, true)
      document.removeEventListener('keydown', key)
    }
  }, [onClose])

  const clearable = downloads.some((d) => !live(d))

  return (
    <div className="dl-panel" ref={ref}>
      <div className="dl-head">
        <span className="dl-title">{t('downloads.title')}</span>
        <button className="dl-clear" disabled={!clearable} onClick={() => signet.clearDownloads()}>
          {t('downloads.clear')}
        </button>
      </div>
      <div className="dl-list">
        {downloads.length === 0 && (
          <div className="empty-state dl-empty">
            <Download size={28} strokeWidth={1.5} />
            <span className="hint">{t('downloads.empty')}</span>
          </div>
        )}
        {downloads.map((d) => (
          <DownloadRow key={d.id} d={d} />
        ))}
      </div>
    </div>
  )
}

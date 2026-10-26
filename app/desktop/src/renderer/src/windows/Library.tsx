import { memo, useCallback, useEffect, useMemo, useRef, useState } from 'react'
import type { ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { Archive, Download, History as HistoryIcon, Layers, Pencil, RotateCcw, Search, UserRound, X } from 'lucide-react'
import { InlineConfirm } from '../ui/InlineConfirm'
import { HISTORY_QUERY_MAX } from '@shared/types'
import type { ArchivedEntry, HistoryEntry, Item, LibrarySection, Snapshot, Space } from '@shared/types'
import { DownloadRow, live } from './Downloads'
import { Favicon } from '../ui/Favicon'
import { domainOf, themeBackground, useHue, useSnapshot } from '../lib/util'

const SECTIONS = [
  { id: 'history', label: 'library.section.history', icon: HistoryIcon },
  { id: 'archive', label: 'library.section.archive', icon: Archive },
  { id: 'downloads', label: 'library.section.downloads', icon: Download },
  { id: 'spaces', label: 'library.section.spaces', icon: Layers }
] as const

const PAGE = 100
const DAY = 86_400_000

const startOfDay = (ts: number) => {
  const d = new Date(ts)
  d.setHours(0, 0, 0, 0)
  return d.getTime()
}

/** Intl formatters cached per language: creating one per row is expensive. */
const cache = new Map<string, Intl.RelativeTimeFormat | Intl.DateTimeFormat>()
function formatter<T extends Intl.RelativeTimeFormat | Intl.DateTimeFormat>(key: string, make: () => T): T {
  let f = cache.get(key)
  if (!f) cache.set(key, (f = make()))
  return f as T
}

/** "Today", "Yesterday" (Intl.RelativeTimeFormat), then "Tuesday 21 October" (with the year if it is not the current year). */
function dayLabel(ts: number, locale: string): string {
  const diff = Math.round((startOfDay(Date.now()) - startOfDay(ts)) / DAY)
  if (diff <= 1) {
    const word = formatter(`rel:${locale}`, () => new Intl.RelativeTimeFormat(locale, { numeric: 'auto' })).format(-Math.max(diff, 0), 'day')
    return word.charAt(0).toLocaleUpperCase(locale) + word.slice(1)
  }
  const sameYear = new Date(ts).getFullYear() === new Date().getFullYear()
  const key = `day:${locale}:${sameYear}`
  return formatter(
    key,
    () => new Intl.DateTimeFormat(locale, { weekday: 'long', day: 'numeric', month: 'long', ...(sameYear ? {} : { year: 'numeric' }) })
  ).format(ts)
}

const timeOf = (ts: number, locale: string) =>
  formatter(`time:${locale}`, () => new Intl.DateTimeFormat(locale, { hour: '2-digit', minute: '2-digit' })).format(ts)

/** Groups a list already sorted by descending day. */
function byDay<T>(items: T[], at: (item: T) => number, locale: string): { key: number; label: string; items: T[] }[] {
  const groups: { key: number; label: string; items: T[] }[] = []
  for (const item of items) {
    const key = startOfDay(at(item))
    const last = groups[groups.length - 1]
    if (last && last.key === key) last.items.push(item)
    else groups.push({ key, label: dayLabel(at(item), locale), items: [item] })
  }
  return groups
}

const tabIdsOf = (i: Item): string[] => (i.kind === 'tab' ? [i.tabId] : i.kind === 'split' ? i.tabIds : i.items.flatMap(tabIdsOf))

/** Value that only follows typing after a short pause. */
function useDebounced<T>(value: T, ms = 160): T {
  const [v, setV] = useState(value)
  useEffect(() => {
    const t = setTimeout(() => setV(value), ms)
    return () => clearTimeout(t)
  }, [value, ms])
  return v
}

/** Increments each time the window returns to the foreground: re-reads the lists. */
function useFocusTick(): number {
  const [tick, setTick] = useState(0)
  useEffect(() => {
    const on = () => setTick((t) => t + 1)
    window.addEventListener('focus', on)
    return () => window.removeEventListener('focus', on)
  }, [])
  return tick
}

/** Route #library: history, archives, downloads and spaces. */
export function Library() {
  const { t } = useTranslation()
  const s = useSnapshot()
  const [section, setSection] = useState<LibrarySection>('history')
  const [query, setQuery] = useState('')
  const [confirm, setConfirm] = useState(false)
  const [rev, setRev] = useState(0)
  const space = s?.spaces.find((x) => x.id === s.activeSpaceId) ?? s?.spaces[0]
  useHue(space?.hue)

  useEffect(() => window.signet.onLibrarySection((sec) => setSection(sec)), [])
  useEffect(() => {
    setQuery('')
    setConfirm(false)
  }, [section])

  const current = SECTIONS.find((x) => x.id === section)!
  const searchable = section === 'history' || section === 'archive'
  const q = useDebounced(query)

  return (
    <div className="settings library">
      <nav className="st-nav">
        <div className="st-nav-top" />
        {SECTIONS.map(({ id, label, icon: Icon }) => (
          <button key={id} data-section={id} className={`st-nav-item${id === section ? ' on' : ''}`} onClick={() => setSection(id)}>
            <Icon size={15} />
            {t(label)}
          </button>
        ))}
      </nav>
      <main className="st-main">
        <header className="st-title lib-title">
          <span className="lib-heading">{t(current.label)}</span>
          {searchable && (
            <label className="lib-search">
              <Search size={13} />
              <input
                value={query}
                placeholder={section === 'history' ? t('library.history.search') : t('library.archive.search')}
                onChange={(e) => setQuery(e.target.value)}
                aria-label={t('library.search')}
              />
              {query && (
                <button className="lib-x" aria-label={t('library.clearSearch')} onClick={() => setQuery('')}>
                  <X size={12} />
                </button>
              )}
            </label>
          )}
          {section === 'history' && <HistoryClear confirm={confirm} setConfirm={setConfirm} onCleared={() => setRev((r) => r + 1)} />}
          {section === 'downloads' && s && (
            <button className="st-btn" disabled={!s.downloads.some((d) => !live(d))} onClick={() => window.signet.clearDownloads()}>
              {t('library.downloads.clear')}
            </button>
          )}
        </header>
        {section === 'history' && <HistoryView query={q} rev={rev} />}
        {section === 'archive' && <ArchiveView query={q} />}
        {section === 'downloads' && s && <DownloadsView s={s} />}
        {section === 'spaces' && s && <SpacesView s={s} />}
      </main>
    </div>
  )
}

// ───────────────────────── Shared elements ─────────────────────────

function Empty({ icon, children }: { icon: ReactNode; children: ReactNode }) {
  return (
    <div className="st-empty lib-empty">
      {icon}
      <p>{children}</p>
    </div>
  )
}

function DayHead({ label }: { label: string }) {
  return <h3 className="lib-day">{label}</h3>
}

interface RowProps {
  favicon?: string
  title: string
  sub: string
  time: string
  onOpen?: () => void
  onRemove: () => void
  action?: ReactNode
}

function LibRow({ favicon, title, sub, time, onOpen, onRemove, action }: RowProps) {
  const { t } = useTranslation()
  return (
    <div className="lib-row" onClick={onOpen}>
      <Favicon src={favicon} size={16} />
      <span className="lib-t">{title}</span>
      <span className="lib-d">{sub}</span>
      <span className="lib-time">{time}</span>
      {action}
      <button
        className="lib-del"
        title={t('library.delete')}
        aria-label={t('library.delete')}
        onClick={(e) => {
          e.stopPropagation()
          onRemove()
        }}
      >
        <X size={13} />
      </button>
    </div>
  )
}

// ───────────────────────── Historique ─────────────────────────

/** "Clear history…" button with in-place confirmation. */
function HistoryClear({ confirm, setConfirm, onCleared }: { confirm: boolean; setConfirm: (c: boolean) => void; onCleared: () => void }) {
  const { t } = useTranslation()
  const [failed, setFailed] = useState(false)
  if (!confirm) {
    return (
      <button className="st-btn" onClick={() => setConfirm(true)}>
        {t('library.history.clear')}
      </button>
    )
  }
  return (
    <div className="lib-confirm">
      <InlineConfirm
        messageClass=""
        message={failed ? t('library.history.failed') : t('library.history.confirm')}
        confirmLabel={t('library.downloads.clear')}
        onCancel={() => (setFailed(false), setConfirm(false))}
        onConfirm={async () => {
          try {
            await window.signet.clearBrowsingData(['history'])
            setFailed(false)
            setConfirm(false)
            onCleared()
          } catch {
            setFailed(true)
          }
        }}
      />
    </div>
  )
}

/** History row: memoized, it only redraws if its entry changes. */
const HistoryRow = memo(function HistoryRow({ e, locale, onRemove }: { e: HistoryEntry; locale: string; onRemove: (url: string) => void }) {
  return (
    <LibRow
      favicon={e.favicon}
      title={e.title || e.url}
      sub={domainOf(e.url)}
      time={timeOf(e.lastVisit, locale)}
      onOpen={() => void window.signet.openLibraryUrl(e.url)}
      onRemove={() => onRemove(e.url)}
    />
  )
})

const HistoryView = memo(function HistoryView({ query, rev }: { query: string; rev: number }) {
  const { t, i18n } = useTranslation()
  const locale = i18n.language
  const signet = window.signet
  const [items, setItems] = useState<HistoryEntry[]>([])
  const [done, setDone] = useState(false)
  const [ready, setReady] = useState(false)
  const gen = useRef(0)
  const busy = useRef(false)
  const count = useRef(0)
  const scroller = useRef<HTMLDivElement>(null)
  const sentinel = useRef<HTMLDivElement>(null)
  const tick = useFocusTick()

  count.current = items.length

  const more = useCallback(
    async (reset: boolean, keep = false) => {
      if (busy.current && !reset) return
      const mine = reset ? ++gen.current : gen.current
      busy.current = true
      // Back to the foreground (keep): re-read as many entries as displayed, in successive queries (main caps at HISTORY_QUERY_MAX).
      const target = reset && keep ? Math.max(PAGE, count.current) : PAGE
      const page: HistoryEntry[] = []
      let full = true
      if (reset) {
        while (page.length < target) {
          const limit = Math.min(HISTORY_QUERY_MAX, target - page.length)
          const part = await signet.queryHistory(query, page.length, limit)
          if (mine !== gen.current) return
          page.push(...part)
          if (part.length < limit) {
            full = false
            break
          }
        }
      } else {
        const part = await signet.queryHistory(query, count.current, PAGE)
        if (mine !== gen.current) return
        page.push(...part)
        full = part.length >= PAGE
      }
      busy.current = false
      setItems((prev) => {
        if (!reset) {
          const seen = new Set(prev.map((e) => e.url))
          return [...prev, ...page.filter((e) => !seen.has(e.url))]
        }
        // Unchanged entries reused (the rows' memo kicks in).
        const old = new Map(prev.map((e) => [e.url, e]))
        return page.map((e) => {
          const o = old.get(e.url)
          return o && o.lastVisit === e.lastVisit && o.title === e.title && o.favicon === e.favicon && o.visits === e.visits ? o : e
        })
      })
      setDone(!full)
      setReady(true)
    },
    [signet, query]
  )

  // New search, return to the foreground or history cleared: start over from the first page.
  const lastKey = useRef('')
  useEffect(() => {
    const key = `${query}\n${rev}`
    void more(true, key === lastKey.current)
    lastKey.current = key
  }, [more, tick, rev, query])

  useEffect(() => {
    const el = sentinel.current
    if (!el || done) return
    const io = new IntersectionObserver((entries) => entries[0]?.isIntersecting && void more(false), { root: scroller.current, rootMargin: '200px' })
    io.observe(el)
    return () => io.disconnect()
  }, [more, done, items.length])

  const remove = useCallback(
    (url: string) => {
      setItems((prev) => prev.filter((e) => e.url !== url))
      void signet.deleteHistory(url)
    },
    [signet]
  )
  const groups = useMemo(() => byDay(items, (e) => e.lastVisit, locale), [items, locale])

  return (
    <div className="lib-body" ref={scroller}>
      {ready && items.length === 0 && <Empty icon={<HistoryIcon size={28} />}>{query ? t('library.noResults') : t('library.history.empty')}</Empty>}
      {groups.map((g) => (
        <section key={g.key}>
          <DayHead label={g.label} />
          {g.items.map((e) => (
            <HistoryRow key={e.url} e={e} locale={locale} onRemove={remove} />
          ))}
        </section>
      ))}
      {!done && <div ref={sentinel} className="lib-sentinel" />}
    </div>
  )
})

// ───────────────────────── Archives ─────────────────────────

/** Archive row: memoized, with stable callbacks per identifier. */
const ArchiveRow = memo(function ArchiveRow({
  e,
  locale,
  onDrop,
  restoreLabel
}: {
  e: ArchivedEntry
  locale: string
  onDrop: (id: string) => void
  restoreLabel: string
}) {
  return (
    <LibRow
      favicon={e.favicon}
      title={e.title || e.url}
      sub={domainOf(e.url)}
      time={timeOf(e.archivedAt, locale)}
      action={
        <button
          className="lib-restore"
          onClick={(ev) => {
            ev.stopPropagation()
            onDrop(e.id)
            void window.signet.restoreArchived(e.id)
          }}
        >
          <RotateCcw size={12} /> {restoreLabel}
        </button>
      }
      onRemove={() => {
        onDrop(e.id)
        void window.signet.deleteArchived(e.id)
      }}
    />
  )
})

const ArchiveView = memo(function ArchiveView({ query }: { query: string }) {
  const { t, i18n } = useTranslation()
  const locale = i18n.language
  const signet = window.signet
  const [items, setItems] = useState<ArchivedEntry[]>([])
  const [ready, setReady] = useState(false)
  const tick = useFocusTick()

  useEffect(() => {
    let alive = true
    void signet
      .listArchive(query)
      .then((list) => {
        if (!alive) return
        setItems(list)
        setReady(true)
      })
      .catch(() => {
        if (alive) setReady(true)
      })
    return () => {
      alive = false
    }
  }, [signet, query, tick])

  const drop = useCallback((id: string) => setItems((prev) => prev.filter((e) => e.id !== id)), [])
  const groups = useMemo(() => byDay(items, (e) => e.archivedAt, locale), [items, locale])
  const restoreLabel = t('library.archive.restore')

  return (
    <div className="lib-body">
      {ready && items.length === 0 && <Empty icon={<Archive size={28} />}>{query ? t('library.noResults') : t('library.archive.empty')}</Empty>}
      {groups.map((g) => (
        <section key={g.key}>
          <DayHead label={g.label} />
          {g.items.map((e) => (
            <ArchiveRow key={e.id} e={e} locale={locale} onDrop={drop} restoreLabel={restoreLabel} />
          ))}
        </section>
      ))}
    </div>
  )
})

// ───────────────────────── Downloads ─────────────────────────

function DownloadsView({ s }: { s: Snapshot }) {
  const { t } = useTranslation()
  return (
    <div className="lib-body">
      {s.downloads.length === 0 ? (
        <Empty icon={<Download size={28} />}>{t('library.downloads.empty')}</Empty>
      ) : (
        <div className="lib-dl">
          {s.downloads.map((d) => (
            <DownloadRow key={d.id} d={d} />
          ))}
        </div>
      )}
    </div>
  )
}

// ───────────────────────── Spaces ─────────────────────────

function SpaceCard({ space, active }: { space: Space; active: boolean }) {
  const { t } = useTranslation()
  const signet = window.signet
  const pinned = space.pinned.flatMap(tabIdsOf).length
  const today = space.today.flatMap(tabIdsOf).length
  return (
    <div
      className={`lib-space${active ? ' on' : ''}`}
      style={{ ['--h' as string]: space.hue }}
      role="button"
      tabIndex={0}
      onClick={() => signet.switchSpace(space.id)}
      onKeyDown={(e) => e.key === 'Enter' && signet.switchSpace(space.id)}
    >
      <span className="lib-space-emoji">{space.icon}</span>
      <div className="lib-space-text">
        <strong>
          <i className="lib-hue" style={space.theme ? { background: themeBackground(space.theme, space.hue) } : undefined} />
          {space.name}
        </strong>
        <span>{t('library.spaces.pinned', { count: pinned })}</span>
        <span>{t('library.spaces.today', { count: today })}</span>
        {space.profile === 'isolated' && (
          <span className="lib-badge">
            <UserRound size={11} /> {t('library.spaces.isolated')}
          </span>
        )}
      </div>
      <button
        className="st-btn lib-edit"
        onClick={(e) => {
          e.stopPropagation()
          void signet.showSpaceMenu(space.id)
        }}
      >
        <Pencil size={12} /> {t('library.spaces.edit')}
      </button>
    </div>
  )
}

function SpacesView({ s }: { s: Snapshot }) {
  const { t } = useTranslation()
  return (
    <div className="lib-body">
      <div className="lib-spaces">
        {s.spaces.map((sp) => (
          <SpaceCard key={sp.id} space={sp} active={sp.id === s.activeSpaceId} />
        ))}
      </div>
      <p className="st-note lib-note">{t('library.spaces.note')}</p>
    </div>
  )
}

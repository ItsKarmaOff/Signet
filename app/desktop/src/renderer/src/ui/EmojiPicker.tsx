import { memo, useEffect, useMemo, useRef, useState } from 'react'
import { Clock } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import type { Locale } from '@signet/shared/i18n'

interface Emoji {
  char: string
  label: string
  /** Normalized search text (no accents, lowercase). */
  haystack: string
}
interface Section {
  key: string
  label: string
  icon: string
  items: Emoji[]
}

const RECENTS_KEY = 'signet.emoji-recents'
const RECENTS_MAX = 16

const fold = (s: string) => s.normalize('NFD').replace(/\p{M}/gu, '').toLowerCase()

type Raw = typeof import('virtual:emoji/en')
/** Data for one language, preprocessed at build time (scripts/emoji-plugin.ts): a separate chunk per language, loaded on first open only. */
const IMPORTERS: Record<Locale, () => Promise<Raw>> = {
  en: () => import('virtual:emoji/en'),
  fr: () => import('virtual:emoji/fr')
}
const cache = new Map<Locale, Promise<Section[]>>()
function loadData(locale: Locale): Promise<Section[]> {
  const hit = cache.get(locale)
  if (hit) return hit
  const dataPromise = IMPORTERS[locale]().then((m) =>
    m.default.map(([key, label, icon, items]) => ({
      key,
      label,
      icon,
      items: items.map(([char, name, hay]) => ({ char, label: name, haystack: hay ?? name }))
    }))
  )
  cache.set(locale, dataPromise)
  dataPromise.catch(() => cache.delete(locale))
  return dataPromise
}

function readRecents(): string[] {
  try {
    const v = JSON.parse(localStorage.getItem(RECENTS_KEY) ?? '[]')
    return Array.isArray(v) ? v.filter((x) => typeof x === 'string').slice(0, RECENTS_MAX) : []
  } catch {
    return []
  }
}
function writeRecent(char: string): void {
  try {
    const next = [char, ...readRecents().filter((c) => c !== char)].slice(0, RECENTS_MAX)
    localStorage.setItem(RECENTS_KEY, JSON.stringify(next))
  } catch {
    /* storage unavailable: no recents */
  }
}

const Cells = memo(function Cells({ items }: { items: Emoji[] }) {
  return (
    <div className="ep-cells">
      {items.map((e) => (
        <button type="button" tabIndex={-1} className="ep-cell" key={e.char} data-char={e.char} aria-label={e.label}>
          {e.char}
        </button>
      ))}
    </div>
  )
})

interface Props {
  onPick: (emoji: string) => void
}

/** Emoji picker unfolded in the form (sidebar width, no overflow). */
export function EmojiPicker({ onPick }: Props) {
  const { t, i18n } = useTranslation()
  const locale = i18n.language as Locale
  const [sections, setSections] = useState<Section[] | null>(null)
  const [failed, setFailed] = useState(false)
  const [query, setQuery] = useState('')
  const [active, setActive] = useState('recent')
  const [hover, setHover] = useState('')
  const [recents] = useState(readRecents)
  const gridRef = useRef<HTMLDivElement>(null)
  const searchRef = useRef<HTMLInputElement>(null)

  useEffect(() => {
    let alive = true
    setFailed(false)
    loadData(locale).then(
      (s) => alive && setSections(s),
      () => alive && setFailed(true)
    )
    return () => {
      alive = false
    }
  }, [locale])
  useEffect(() => searchRef.current?.focus(), [])

  const labels = useMemo(() => {
    const m = new Map<string, string>()
    sections?.forEach((s) => s.items.forEach((e) => m.set(e.char, e.label)))
    return m
  }, [sections])

  const recentItems = useMemo<Emoji[]>(
    () => recents.filter((c) => labels.has(c)).map((c) => ({ char: c, label: labels.get(c) ?? '', haystack: '' })),
    [recents, labels]
  )

  const needle = fold(query.trim())
  const results = useMemo<Emoji[] | null>(() => {
    if (!sections || !needle) return null
    const words = needle.split(/\s+/)
    return sections.flatMap((s) => s.items.filter((e) => words.every((w) => e.haystack.includes(w))))
  }, [sections, needle])

  const tabs = useMemo(() => {
    const list = sections ? sections.map((s) => ({ key: s.key, label: s.label, icon: s.icon })) : []
    return recentItems.length ? [{ key: 'recent', label: t('emoji.recent'), icon: '' }, ...list] : list
  }, [sections, recentItems, t])

  const body = useMemo(() => {
    if (!sections) return null
    if (results) return null
    return (
      <>
        {recentItems.length > 0 && (
          <section data-key="recent">
            <h4>{t('emoji.recent')}</h4>
            <Cells items={recentItems} />
          </section>
        )}
        {sections.map((s) => (
          <section key={s.key} data-key={s.key}>
            <h4>{s.label}</h4>
            <Cells items={s.items} />
          </section>
        ))}
      </>
    )
  }, [sections, results, recentItems, t])

  const pick = (char: string) => {
    writeRecent(char)
    onPick(char)
  }

  const cellOf = (t: EventTarget | null) => (t as HTMLElement | null)?.closest<HTMLElement>('.ep-cell') ?? null

  const syncActive = () => {
    const grid = gridRef.current
    if (!grid || results) return
    let cur = ''
    for (const sec of grid.querySelectorAll<HTMLElement>('section')) {
      if (sec.offsetTop - grid.offsetTop <= grid.scrollTop + 8) cur = sec.dataset.key ?? cur
    }
    if (cur) setActive(cur)
  }

  const goTo = (key: string) => {
    const grid = gridRef.current
    const sec = grid?.querySelector<HTMLElement>(`section[data-key="${key}"]`)
    if (!grid || !sec) return
    grid.scrollTo({ top: sec.offsetTop - grid.offsetTop, behavior: 'smooth' })
    setActive(key)
  }

  const moveFocus = (from: HTMLElement, key: string) => {
    const grid = gridRef.current
    if (!grid) return
    const cells = [...grid.querySelectorAll<HTMLElement>('.ep-cell')]
    const i = cells.indexOf(from)
    let next: HTMLElement | undefined
    if (key === 'ArrowLeft') next = cells[i - 1]
    else if (key === 'ArrowRight') next = cells[i + 1]
    else {
      // Up/down: computed on indices (columns read once), without measuring each cell.
      const down = key === 'ArrowDown'
      const groups = [...grid.querySelectorAll<HTMLElement>('.ep-cells')]
      const g = groups.indexOf(from.parentElement as HTMLElement)
      const own = [...groups[g].children] as HTMLElement[]
      const cols = Math.max(1, getComputedStyle(groups[g]).gridTemplateColumns.split(' ').length)
      const at = own.indexOf(from)
      const col = at % cols
      const row = Math.floor(at / cols)
      const target = row + (down ? 1 : -1)
      if (target >= 0 && target * cols < own.length) next = own[Math.min(target * cols + col, own.length - 1)]
      else {
        const other = groups[g + (down ? 1 : -1)]
        if (other?.children.length) {
          const cells = other.children
          const r = down ? 0 : Math.floor((cells.length - 1) / cols)
          next = cells[Math.min(r * cols + col, cells.length - 1)] as HTMLElement
        }
      }
      if (!next && !down) {
        searchRef.current?.focus()
        return
      }
    }
    next?.focus()
    next?.scrollIntoView({ block: 'nearest' })
  }

  return (
    <div className="emoji-picker" onClick={(e) => e.stopPropagation()}>
      <input
        ref={searchRef}
        type="search"
        value={query}
        placeholder={t('emoji.search')}
        aria-label={t('emoji.search')}
        onChange={(e) => {
          setQuery(e.target.value)
          if (gridRef.current) gridRef.current.scrollTop = 0
        }}
        onKeyDown={(e) => {
          if (e.nativeEvent.isComposing) return
          if (e.key === 'Enter') {
            e.preventDefault()
            e.stopPropagation()
            if (results?.[0]) pick(results[0].char)
          } else if (e.key === 'ArrowDown') {
            e.preventDefault()
            gridRef.current?.querySelector<HTMLElement>('.ep-cell')?.focus()
          }
        }}
      />
      <div className="ep-tabs" role="tablist">
        {tabs.map((tb) => (
          <button
            type="button"
            role="tab"
            key={tb.key}
            title={tb.label}
            aria-label={tb.label}
            aria-selected={!results && active === tb.key}
            className={!results && active === tb.key ? 'on' : ''}
            disabled={!!results}
            onClick={() => goTo(tb.key)}
          >
            {tb.key === 'recent' ? <Clock size={14} /> : tb.icon}
          </button>
        ))}
      </div>
      <div
        className="ep-grid"
        ref={gridRef}
        onScroll={syncActive}
        onClick={(e) => {
          const c = cellOf(e.target)
          if (c?.dataset.char) pick(c.dataset.char)
        }}
        onMouseOver={(e) => {
          const c = cellOf(e.target)
          if (c) setHover(c.getAttribute('aria-label') ?? '')
        }}
        onMouseLeave={() => setHover('')}
        onFocus={(e) => {
          const c = cellOf(e.target)
          if (c) setHover(c.getAttribute('aria-label') ?? '')
        }}
        onKeyDown={(e) => {
          const c = cellOf(e.target)
          if (c && e.key.startsWith('Arrow')) {
            e.preventDefault()
            moveFocus(c, e.key)
          }
        }}
      >
        {failed ? (
          <div className="ep-empty">{t('emoji.loadFailed')}</div>
        ) : !sections ? (
          <div className="ep-empty">{t('emoji.loading')}</div>
        ) : results ? (
          results.length ? (
            <Cells items={results} />
          ) : (
            <div className="ep-empty">{t('emoji.noResults')}</div>
          )
        ) : (
          body
        )}
      </div>
      <div className="ep-foot">{hover || ' '}</div>
    </div>
  )
}

import { useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'
import type { ChangeEvent, KeyboardEvent } from 'react'
import { useTranslation } from 'react-i18next'
import {
  AppWindow,
  Archive,
  Code,
  Columns2,
  Command,
  Compass,
  Copy,
  Eye,
  FlaskConical,
  FolderPlus,
  Globe,
  Layers,
  PanelBottom,
  PanelLeft,
  PanelRight,
  Pin,
  RotateCw,
  Search,
  SquareTerminal,
  Star,
  Undo2,
  X
} from 'lucide-react'
import type { LucideIcon } from 'lucide-react'
import type { OverlayPayload, PaletteResult, Rect } from '@shared/types'
import { Favicon } from '../ui/Favicon'
import { platformKeys } from '@signet/shared/i18n'
import { highlightRange } from '@shared/highlight'
import { canComplete, inlineCompletion } from '@shared/inline'

type PalettePayload = Extract<OverlayPayload, { mode: 'palette' }>

const ACTION_ICONS: Record<string, LucideIcon> = {
  'new-space': FolderPlus,
  split: Columns2,
  unsplit: Columns2,
  'toggle-sidebar': PanelLeft,
  pin: Pin,
  favorite: Star,
  'copy-url': Copy,
  little: AppWindow,
  'clear-today': Archive,
  'close-tab': X,
  reopen: Undo2,
  reload: RotateCw,
  devtools: Code,
  'devtools-right': PanelRight,
  'devtools-bottom': PanelBottom,
  'devtools-window': AppWindow,
  'js-console': SquareTerminal,
  'toggle-peek-links': Eye,
  'default-browser': Compass
}

/** Title with the typed part in bold (not for rows whose title is not the page's). */
function Title({ r, query }: { r: PaletteResult; query: string }) {
  const range = r.kind === 'tab' || r.kind === 'history' || r.kind === 'archived' || r.kind === 'suggestion' ? highlightRange(r.title, query) : null
  if (!range) return <>{r.title}</>
  return (
    <>
      {r.title.slice(0, range[0])}
      <b>{r.title.slice(range[0], range[1])}</b>
      {r.title.slice(range[1])}
    </>
  )
}

function ResultIcon({ r }: { r: PaletteResult }) {
  switch (r.kind) {
    case 'tab':
    case 'history':
    case 'archived':
      return <Favicon src={r.favicon} size={18} />
    case 'search':
    case 'suggestion':
      return <Search size={17} strokeWidth={1.8} />
    case 'url':
      return <Globe size={17} strokeWidth={1.8} />
    case 'action': {
      if (r.image) return <img className="result-image" src={r.image} alt="" />
      const Icon = r.action.startsWith('space:') ? Layers : r.action.startsWith('develop:') ? FlaskConical : (ACTION_ICONS[r.action] ?? Command)
      return <Icon size={17} strokeWidth={1.8} />
    }
  }
}

const PALETTE_W = 640
/** Minimum delay (ms) between two search requests while typing. */
const QUERY_GAP = 70

/** Palette that grows from the address field (`edit-url` mode): placed over it, scale origin on its center. */
function fromField(anchor: Rect) {
  const width = Math.min(PALETTE_W, window.innerWidth - 32)
  const center = anchor.x + anchor.width / 2
  // `left` is the center of the palette (CSS translateX(-50%)): it stays within the window.
  const left = Math.max(16 + width / 2, Math.min(center, window.innerWidth - 16 - width / 2))
  const top = Math.max(8, Math.min(anchor.y - 6, window.innerHeight - 220))
  return { left, top, transformOrigin: `${center - (left - width / 2)}px ${anchor.y + anchor.height / 2 - top}px` }
}

const PLACEHOLDERS = {
  'new-tab': 'toolbar.addressPlaceholder',
  'edit-url': 'palette.editUrl',
  split: 'palette.splitPlaceholder'
} as const

export function Palette({ payload }: { payload: PalettePayload }) {
  const { t } = useTranslation()
  const signet = window.signet
  const { paletteMode: mode, initial, hue, anchor } = payload

  const [query, setQuery] = useState(initial)
  const [base, setBase] = useState<PaletteResult[]>([])
  const [baseFor, setBaseFor] = useState<string | null>(null)
  const [suggestions, setSuggestions] = useState<PaletteResult[]>([])
  const [suggestFor, setSuggestFor] = useState('')
  const [selId, setSelId] = useState<string | null>(null)
  // Inline completion: what is appended (and selected) after the typed text. `query` stays the typed text only.
  const [completion, setCompletion] = useState('')
  const allowCompletion = useRef(false)
  const composing = useRef(false)
  const seq = useRef(0)
  const lastSent = useRef(-Infinity)
  const inputRef = useRef<HTMLInputElement>(null)
  const listRef = useRef<HTMLUListElement>(null)

  // Request while typing (spaced by 70 ms), suggestions after 150 ms of calm.
  // `seq` discards any response that arrived after a new input.
  useEffect(() => {
    const mine = ++seq.current
    // Previous suggestions stay displayed until the new ones arrive (no flicker).
    if (!query.trim()) setSuggestions([])
    setSelId(null)
    // At most one request every 70 ms (the first goes out right away); Enter does not wait, see `onKeyDown`.
    const send = () => {
      lastSent.current = performance.now()
      signet
        .paletteQuery(query, mode)
        .then((r) => {
          if (mine !== seq.current) return
          setBase(r)
          setBaseFor(query)
        })
        .catch(() => {})
    }
    const wait = Math.max(0, QUERY_GAP - (performance.now() - lastSent.current))
    const queryTimer = wait ? setTimeout(send, wait) : void send()
    if (!query.trim()) return () => clearTimeout(queryTimer)
    const timer = setTimeout(() => {
      signet
        .paletteSuggest(query)
        .then((r) => {
          if (mine !== seq.current) return
          setSuggestions(r)
          setSuggestFor(query)
        })
        .catch(() => {})
    }, 150)
    return () => {
      clearTimeout(queryTimer)
      clearTimeout(timer)
    }
  }, [query, mode, signet])

  // Complete only from results for the text currently typed, and only after typing (never after a deletion).
  useEffect(() => {
    if (baseFor !== query) return
    setCompletion(allowCompletion.current ? inlineCompletion(query, base[0]) : '')
  }, [base, baseFor, query])

  // Focus on open (text preselected to edit the address) and when focus returns.
  useEffect(() => {
    const el = inputRef.current
    if (!el) return
    el.focus()
    if (mode === 'edit-url') el.select()
    const refocus = () => el.focus()
    window.addEventListener('focus', refocus)
    return () => window.removeEventListener('focus', refocus)
  }, [mode])

  // Engine suggestions go right after the search row (or the first result if it is far down): they never take the top spot.
  const results = useMemo(() => {
    // Suggestions from a previous input: keep only those that still match.
    const q = query.trim().toLowerCase()
    const current = suggestFor === query ? suggestions : suggestions.filter((r) => r.title.toLowerCase().startsWith(q))
    if (!current.length) return base
    const seen = new Set(base.map((r) => r.title.toLowerCase()))
    const extra = current.filter((r) => !seen.has(r.title.toLowerCase()))
    if (!base.length) return extra
    const search = base.findIndex((r) => r.kind === 'search')
    const at = search >= 0 && search < 3 ? search + 1 : 1
    return [...base.slice(0, at), ...extra, ...base.slice(at)]
  }, [base, suggestions, suggestFor, query])

  const selIndex = Math.max(
    0,
    results.findIndex((r) => r.id === selId)
  )

  useEffect(() => {
    listRef.current?.children[selIndex]?.scrollIntoView({ block: 'nearest' })
  }, [selIndex, results])

  // The completion belongs to the top row: moving down the list drops it.
  const shown = selIndex === 0 ? completion : ''
  const field = query + shown
  useLayoutEffect(() => {
    if (shown) inputRef.current?.setSelectionRange(query.length, field.length)
  }, [query, shown, field.length])

  const onChange = (e: ChangeEvent<HTMLInputElement>) => {
    const value = e.target.value
    const ok = canComplete(query, value, (e.nativeEvent as InputEvent).inputType, composing.current)
    // Typing what the completion already shows keeps the rest of it, so it does not flicker.
    const typed = value.slice(query.length)
    setCompletion(ok && completion.toLowerCase().startsWith(typed.toLowerCase()) ? completion.slice(typed.length) : '')
    allowCompletion.current = ok
    setQuery(value)
  }

  const run = (r: PaletteResult) => signet.paletteExecute(r, mode)

  const onKeyDown = async (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.nativeEvent.isComposing) return
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault()
      if (!results.length) return
      const step = e.key === 'ArrowDown' ? 1 : -1
      setSelId(results[(selIndex + step + results.length) % results.length].id)
    } else if (shown && e.key === 'Escape') {
      // First Escape only removes the completion.
      e.preventDefault()
      e.stopPropagation()
      setCompletion('')
      allowCompletion.current = false
    } else if (shown && (e.key === 'ArrowRight' || e.key === 'End') && e.currentTarget.selectionStart === query.length) {
      e.preventDefault()
      e.currentTarget.setSelectionRange(field.length, field.length)
      allowCompletion.current = false
      setCompletion('')
      setQuery(field)
    } else if (e.key === 'Enter') {
      e.preventDefault()
      if (baseFor === query) {
        if (results[selIndex]) void run(results[selIndex])
      } else {
        // Results not up to date yet: query directly for the current input.
        const fresh = await signet.paletteQuery(query, mode)
        if (fresh[0]) void run(fresh[0])
      }
    }
  }

  return (
    <div className="palette-root">
      <div className="veil" onMouseDown={() => signet.closeOverlay()} />
      <div className="palette" style={{ ['--h' as string]: hue, ...(mode === 'edit-url' && anchor && fromField(anchor)) }}>
        <div className="palette-input">
          {mode === 'split' && (
            <span className="mode-tag">
              <Columns2 size={12} /> {t('palette.splitTag')}
            </span>
          )}
          <span className="input-ico">
            {shown ? <Favicon src={'favicon' in results[0] ? results[0].favicon : undefined} size={18} /> : <Search size={17} strokeWidth={1.8} />}
          </span>
          <input
            ref={inputRef}
            value={field}
            placeholder={t(PLACEHOLDERS[mode])}
            spellCheck={false}
            autoComplete="off"
            role="combobox"
            aria-expanded={results.length > 0}
            aria-controls="palette-results"
            aria-autocomplete="list"
            aria-activedescendant={results.length > 0 ? `palette-opt-${selIndex}` : undefined}
            aria-label={t(PLACEHOLDERS[mode])}
            onChange={onChange}
            onCompositionStart={() => (composing.current = true)}
            onCompositionEnd={() => (composing.current = false)}
            onKeyDown={onKeyDown}
          />
        </div>
        {results.length > 0 && (
          <ul className="results" id="palette-results" role="listbox" ref={listRef}>
            {results.map((r, i) => (
              <li
                key={r.id}
                id={`palette-opt-${i}`}
                role="option"
                aria-selected={i === selIndex}
                className={i === selIndex ? 'sel' : ''}
                onMouseMove={() => r.id !== selId && setSelId(r.id)}
                onClick={() => run(r)}
              >
                <span className="ico">
                  <ResultIcon r={r} />
                </span>
                <span className="text">
                  <span className="t">
                    <Title r={r} query={query} />
                  </span>
                  {r.subtitle && <span className="sub">{r.subtitle}</span>}
                </span>
                {r.kind === 'action' && r.shortcut && <kbd>{platformKeys(r.shortcut)}</kbd>}
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  )
}

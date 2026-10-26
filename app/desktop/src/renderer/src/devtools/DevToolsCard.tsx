import { AppWindow, Code, PanelBottom, PanelRight, X } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'
import type { PointerEvent } from 'react'
import { useTranslation } from 'react-i18next'
import type { DevToolsDock, DevToolsWindowState, Pane } from '@shared/types'
import { DEVTOOLS_BAR } from '../lib/util'

type Docked = NonNullable<Pane['devtools']>

/** Handle between the page and the tools (in the 8 px gap): drag to resize, double-click for the original size. */
function Handle({ tabId, info }: { tabId: string; info: Docked }) {
  const { t } = useTranslation()
  const vertical = info.dock === 'right'
  const drag = useRef<{ pointerId: number; start: number; size: number } | null>(null)
  const frame = useRef(0)
  const latest = useRef<number | null>(null)
  const [active, setActive] = useState(false)

  const escOff = useRef<(() => void) | null>(null)

  useEffect(
    () => () => {
      cancelAnimationFrame(frame.current)
      escOff.current?.()
    },
    []
  )

  const send = (done: boolean) => {
    frame.current = 0
    if (latest.current !== null) void window.signet.setDevToolsSize(tabId, latest.current, done).catch(() => {})
    if (done) latest.current = null
  }

  const down = (e: PointerEvent<HTMLDivElement>) => {
    if (e.button !== 0) return
    e.preventDefault()
    e.currentTarget.setPointerCapture(e.pointerId)
    const g = { pointerId: e.pointerId, start: vertical ? e.clientX : e.clientY, size: vertical ? info.card.width : info.card.height }
    drag.current = g
    setActive(true)
    // Esc: the starting size is restored.
    const onKey = (k: KeyboardEvent) => {
      if (k.key !== 'Escape' || drag.current !== g) return
      k.preventDefault()
      k.stopPropagation()
      drag.current = null
      escOff.current?.()
      cancelAnimationFrame(frame.current)
      latest.current = g.size
      send(true)
      setActive(false)
    }
    escOff.current?.()
    window.addEventListener('keydown', onKey, true)
    escOff.current = () => window.removeEventListener('keydown', onKey, true)
  }
  const move = (e: PointerEvent<HTMLDivElement>) => {
    const g = drag.current
    if (!g || g.pointerId !== e.pointerId) return
    // The tools are after the page: dragging toward the page enlarges them.
    latest.current = Math.round(g.size - ((vertical ? e.clientX : e.clientY) - g.start))
    if (!frame.current) frame.current = requestAnimationFrame(() => send(false))
  }
  const up = (e: PointerEvent<HTMLDivElement>) => {
    if (!drag.current || drag.current.pointerId !== e.pointerId) return
    drag.current = null
    escOff.current?.()
    cancelAnimationFrame(frame.current)
    send(true)
    setActive(false)
  }

  const r = info.divider
  return (
    <div
      className={`split-divider ${vertical ? 'vertical' : 'horizontal'}${active ? ' active' : ''}`}
      style={{ left: r.x, top: r.y, width: r.width, height: r.height }}
      title={t('devtools.resize')}
      onPointerDown={down}
      onPointerMove={move}
      onPointerUp={up}
      onPointerCancel={up}
      onLostPointerCapture={(e) => up(e)}
      onDoubleClick={() => void window.signet.setDevToolsSize(tabId, 0, true).catch(() => {})}
    >
      <span className="divider-handle" />
    </div>
  )
}

/** Tools header (embedded card or separate window): placement and closing. */
function Header({ tabId, dock, title, dockable = true }: { tabId: string; dock: DevToolsDock; title: string; dockable?: boolean }) {
  const { t } = useTranslation()
  const place = (to: DevToolsDock, label: string, Icon: typeof PanelRight) => (
    <button
      className={`icon sm${dock === to ? ' on' : ''}`}
      title={label}
      aria-pressed={dock === to}
      disabled={to !== 'window' && !dockable}
      onClick={() => void window.signet.setDevToolsDock(tabId, to)}
    >
      <Icon size={15} />
    </button>
  )
  return (
    <div className="devtools-bar" style={{ height: DEVTOOLS_BAR }}>
      <Code size={14} />
      <span className="devtools-title">{title}</span>
      {place('right', t('devtools.dockRight'), PanelRight)}
      {place('bottom', t('devtools.dockBottom'), PanelBottom)}
      {place('window', t('devtools.dockWindow'), AppWindow)}
      <button className="icon sm" title={t('devtools.close')} onClick={() => void window.signet.closeDevTools(tabId)}>
        <X size={15} />
      </button>
    </div>
  )
}

/** Embedded developer tools card: header above the native tools view. */
export function DevToolsCard({ tabId, info, dim }: { tabId: string; info: Docked; dim: boolean }) {
  const { t } = useTranslation()
  const c = info.card
  return (
    <>
      <div className={`pane-shadow devtools-card ${info.dock}${dim ? ' dim' : ''}`} style={{ left: c.x, top: c.y, width: c.width, height: c.height }}>
        <Header tabId={tabId} dock={info.dock} title={t('devtools.title')} />
      </div>
      <Handle tabId={tabId} info={info} />
    </>
  )
}

/** Route #devtools: header of the detached tools window (native tools view below), to reattach them to the page. */
export function DevToolsWindow() {
  const { t } = useTranslation()
  const [st, setSt] = useState<DevToolsWindowState | null>(null)
  useEffect(() => {
    const off = window.signet.onDevToolsWindowState(setSt)
    void window.signet.devToolsWindowReady().then(setSt)
    return off
  }, [])
  if (!st) return null
  return (
    <div className="devtools-window">
      <Header tabId={st.tabId} dock="window" title={st.site ? t('devtools.windowTitle', { site: st.site }) : t('devtools.title')} dockable={st.dockable} />
    </div>
  )
}

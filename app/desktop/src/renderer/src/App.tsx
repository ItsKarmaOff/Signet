import { Fragment, memo, useEffect, useLayoutEffect, useRef, useState } from 'react'
import { Globe, Plus } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import type { Bookmark, Pane, Rect, Snapshot } from '@shared/types'
import { slideAt, slideRect, type SidebarSlide } from '@shared/slide'
import { Dividers } from './tabs/Dividers'
import { DevToolsCard } from './devtools/DevToolsCard'
import { ResponsiveFrame } from './devtools/Responsive'
import { Sidebar } from './sidebar/Sidebar'
import { TabStrip } from './tabs/TabStrip'
import { Toolbar } from './toolbar/Toolbar'
import { barStyle, BOOKMARK_BAR, PANE_BAR, useHue, useSnapshot, useSpaceTheme } from './lib/util'
import { BookmarksBar } from './toolbar/BookmarksBar'
import { IS_MAC } from '@signet/shared/i18n'
import { shortcutLabel } from '@shared/shortcuts'

/** Route #sidebar: the floating sidebar (web view placed by the main process over the window). */
export function FloatingSidebar() {
  const s = useSnapshot()
  const space = s?.spaces.find((x) => x.id === s.activeSpaceId) ?? s?.spaces[0]
  useHue(space?.hue)
  const shown = !!s?.sidebarFloating
  const [on, setOn] = useState(false)
  // Enter: slide from the left edge (starts after two frames, the detached view does not run requestAnimationFrame).
  // Exit: immediate return to the left, the main process keeps the view in place during the slide; a pointer return restarts from the current position.
  useEffect(() => {
    if (!shown) return setOn(false)
    let id2 = 0
    const id1 = requestAnimationFrame(() => (id2 = requestAnimationFrame(() => setOn(true))))
    return () => (cancelAnimationFrame(id1), cancelAnimationFrame(id2))
  }, [shown])
  // The sidebar's effects (subscriptions) run before this one: the main process may replay its pending sends.
  const loaded = !!s
  useEffect(() => {
    if (loaded) void window.signet.overlayReady()
  }, [loaded])
  if (!s) return null
  return (
    <div className={`float-sidebar${on ? ' on' : ''}${s.settings.sidebarSide === 'right' ? ' right' : ''}`}>
      <Sidebar s={s} floating />
    </div>
  )
}

/** Route #toolbar: the compact mode's floating toolbar (appearance: fade + 6 px translation). */
export function FloatingToolbar() {
  const s = useSnapshot()
  const space = s?.spaces.find((x) => x.id === s.activeSpaceId) ?? s?.spaces[0]
  useHue(space?.hue)
  const shown = !!s?.toolbarFloating
  const [on, setOn] = useState(false)
  // requestAnimationFrame does not run while the view is detached: the animation starts on display.
  useEffect(() => {
    if (!shown) return setOn(false)
    let id2 = 0
    const id1 = requestAnimationFrame(() => (id2 = requestAnimationFrame(() => setOn(true))))
    return () => (cancelAnimationFrame(id1), cancelAnimationFrame(id2))
  }, [shown])
  useEffect(() => {
    const el = document.documentElement
    const enter = () => void window.signet.setToolbarHover(true)
    const leave = () => void window.signet.setToolbarHover(false)
    el.addEventListener('mouseenter', enter)
    el.addEventListener('mouseleave', leave)
    return () => (el.removeEventListener('mouseenter', enter), el.removeEventListener('mouseleave', leave))
  }, [])
  if (!s) return null
  return (
    <div className={`float-toolbar${on ? ' on' : ''}`}>
      {s.settings.tabLayout === 'horizontal' ? <TabStrip s={s} floating /> : <Toolbar s={s} leading={IS_MAC} sidebarToggle floating />}
    </div>
  )
}

/** Focus ring: glides from one pane to another, but follows the geometry without lag (divider, window, sidebar). */
function FocusRing({ id, card }: { id: string; card: { x: number; y: number; width: number; height: number } }) {
  const prev = useRef(id)
  const glide = prev.current !== id
  useEffect(() => {
    prev.current = id
  })
  return (
    <div
      className={`focus-ring${glide ? ' glide' : ''}`}
      style={{ transform: `translate(${card.x - 3}px, ${card.y - 3}px)`, width: card.width + 6, height: card.height + 6 }}
    />
  )
}

/** The fixed sidebar's shift goes through a CSS variable (styles/layout.css): no sidebar render per frame. */
function setSidebarShift(slide: SidebarSlide | null): void {
  const root = document.documentElement
  root.style.setProperty('--sidebar-slide', `${slide?.sidebar ?? 0}px`)
  root.classList.toggle('sliding', slide !== null)
}

/** Sidebar slide (toggle): the geometry comes with the state, each step's position from main (shared/slide). */
function useSidebarSlide(s: Snapshot | null): SidebarSlide | null {
  const plan = s?.sidebarSlide ?? null
  const [progress, setProgress] = useState<number | null>(null)
  useEffect(() => window.signet.onSidebarSlide(setProgress), [])
  const slide = plan ? slideAt(plan, progress ?? plan.motion.from) : null
  useLayoutEffect(() => setSidebarShift(slide), [slide])
  return slide
}

/** Two states equal for the sidebar: same values one level down (the state is rebuilt on each send). */
function sameForSidebar(a: { s: Snapshot }, b: { s: Snapshot }): boolean {
  if (a.s === b.s) return true
  // Only the main UI's page chrome reads these; the sidebar's slide goes through a CSS variable.
  const skip = new Set<string>(['sidebarSlide', 'panes', 'dividers', 'overlayOpen'])
  const x = a.s as unknown as Record<string, unknown>
  const y = b.s as unknown as Record<string, unknown>
  const keys = new Set([...Object.keys(x), ...Object.keys(y)])
  for (const k of keys) {
    if (skip.has(k) || x[k] === y[k]) continue
    const u = x[k]
    const v = y[k]
    if (Array.isArray(u) && Array.isArray(v)) {
      if (u.length !== v.length || u.some((e, i) => e !== v[i])) return false
    } else if (u && v && typeof u === 'object' && typeof v === 'object') {
      const uk = Object.keys(u)
      const vo = v as Record<string, unknown>
      if (uk.length !== Object.keys(vo).length || uk.some((key) => (u as Record<string, unknown>)[key] !== vo[key])) return false
    } else return false
  }
  return true
}

/**
 * The page chrome (cards, dividers) stretched into the space the bar leaves on this step of the slide.
 * The native pages under it already have their final size and only move: the card, in the page's
 * color, fills the rest.
 */
function stretched(s: Snapshot, slide: SidebarSlide | null): Snapshot {
  if (!slide) return s
  const r = <R extends Rect>(x: R): R => slideRect(x, slide)
  const pane = (p: Pane): Pane => ({
    ...p,
    rect: r(p.rect),
    card: r(p.card),
    ...(p.devtools && { devtools: { ...p.devtools, card: r(p.devtools.card), rect: r(p.devtools.rect), divider: r(p.devtools.divider) } }),
    ...(p.responsive && { responsive: { ...p.responsive, bar: r(p.responsive.bar), area: r(p.responsive.area) } })
  })
  return { ...s, panes: s.panes.map(pane), dividers: s.dividers.map((d) => ({ ...d, rect: r(d.rect) })) }
}

/** The slide steps do not redraw the sidebar, nor a new state that changes nothing it shows. */
const FixedSidebar = memo(Sidebar, sameForSidebar)

/** Main window: sidebar, toolbar, focus ring, empty state. */
const EMPTY_BOOKMARKS: Bookmark[] = []

export function App() {
  const { t } = useTranslation()
  const snap = useSnapshot()
  const space = snap?.spaces.find((x) => x.id === snap.activeSpaceId) ?? snap?.spaces[0]
  useSpaceTheme(space)
  const slide = useSidebarSlide(snap)
  // ⌘L: the main process asks where the visible address field is (active pane's bar); without it, the command bar stays centered.
  useEffect(
    () =>
      window.signet.onEditAddressRequest(() => {
        const r = document.querySelector('.toolbar:not(.dim):not(.floating) .address')?.getBoundingClientRect()
        void window.signet.openPalette('edit-url', r && r.width > 0 ? { x: r.left, y: r.top, width: r.width, height: r.height } : undefined)
      }),
    []
  )
  if (!snap) return null
  const s = stretched(snap, slide)

  const left = s.sidebarVisible ? s.sidebarWidth : 0
  const ring = s.panes.length > 1 ? s.panes.find((p) => p.focused) : undefined
  const compact = s.settings.compactMode
  const horizontal = s.settings.tabLayout === 'horizontal'
  const bookmarks = s.settings.showBookmarksBar !== false
  // Current shortcuts (editable in Settings) for the on-screen hints.
  const newTabKeys = shortcutLabel('newTab', s.settings.shortcuts, IS_MAC)
  const bookmarkKeys = shortcutLabel('bookmark', s.settings.shortcuts, IS_MAC)
  const right = !horizontal && s.settings.sidebarSide === 'right'
  // Sidebar on the right: the top-left card carries the traffic lights (Mac only; a hidden left sidebar hides them).
  const corner = IS_MAC && right && !compact ? [...s.panes].sort((a, b) => a.card.y - b.card.y || a.card.x - b.card.x)[0] : undefined

  // The sidebar toggle lives in the top-left pane's bar, sidebar shown or not (compact mode: the floating bar has it).
  const first = !horizontal && !compact ? [...s.panes].sort((a, b) => a.card.y - b.card.y || a.card.x - b.card.x)[0] : undefined

  return (
    <div className={`app${s.overlayOpen ? ' covered' : ''}`}>
      {!compact && !horizontal && <div className={`drag-strip${s.sidebarVisible ? '' : ' short'}`} />}
      {horizontal && !compact && <TabStrip s={s} />}
      {s.settings.compactMode && (
        <div className="top-hover" onMouseEnter={() => window.signet.setToolbarHover(true)} onMouseLeave={() => window.signet.setToolbarHover(false)} />
      )}
      {!s.sidebarVisible && !horizontal && <div className={`rim-hover${right ? ' right' : ''}`} onMouseEnter={() => window.signet.setSidebarHover(true)} />}
      {s.sidebarVisible && <FixedSidebar s={snap} />}
      {s.panes.map((p) => (
        <Fragment key={p.tabId}>
          <div
            className="pane-shadow"
            style={{ left: p.card.x, top: p.card.y, width: p.card.width, height: p.card.height, ...barStyle(s.tabs[p.tabId]?.themeColor) }}
          >
            {!compact && (
              <>
                {/* Responsive mode: no overflow, it would show through the translucent background of the device area. */}
                <div className="pane-fill" style={{ height: PANE_BAR + (bookmarks ? BOOKMARK_BAR : 0) + (p.responsive ? 0 : 12) }} />
                <div className="pane-bar" style={{ height: PANE_BAR }}>
                  <Toolbar
                    s={s}
                    tabId={p.tabId}
                    leading={p === corner}
                    sidebarToggle={p === first}
                    narrow={p.card.width < 420}
                    dim={s.panes.length > 1 && !p.focused}
                  />
                </div>
                {bookmarks && (
                  <div className={`pane-bookmarks${s.panes.length > 1 && !p.focused ? ' dim' : ''}`} style={{ top: PANE_BAR, height: BOOKMARK_BAR }}>
                    <BookmarksBar items={space?.bookmarks ?? EMPTY_BOOKMARKS} tabId={p.tabId} focused={p.focused || s.panes.length === 1} keys={bookmarkKeys} />
                  </div>
                )}
              </>
            )}
          </div>
          {p.responsive && <ResponsiveFrame tabId={p.tabId} pane={p} />}
          {p.devtools && <DevToolsCard tabId={p.tabId} info={p.devtools} dim={s.panes.length > 1 && !p.focused} />}
        </Fragment>
      ))}
      {s.panes.length === 0 && (
        <div
          className={`empty${s.settings.compactMode ? ' compact' : ''}`}
          style={{
            left: s.sidebarVisible && !right ? left : compact ? 6 : 8,
            right: s.sidebarVisible && right ? left : compact ? 6 : 8,
            ...(horizontal && !compact ? { top: 44 } : undefined)
          }}
        >
          <Globe size={28} strokeWidth={1.5} />
          <p>{t('app.noTabs')}</p>
          <button className="primary" onClick={() => window.signet.openPalette('new-tab')}>
            <Plus size={15} /> {t('sidebar.newTab')} {newTabKeys && <kbd>{newTabKeys}</kbd>}
          </button>
        </div>
      )}
      <Dividers s={s} />
      {ring && <FocusRing id={ring.tabId} card={ring.card} />}
    </div>
  )
}

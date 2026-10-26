import { Fragment, memo, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'
import type { DragEvent, PointerEvent, ReactNode, RefObject } from 'react'
import { useTranslation } from 'react-i18next'
import { IS_MAC } from '@signet/shared/i18n'
import { shortcutLabel } from '@shared/shortcuts'
import { Archive, Download, FolderPlus, Library, Plus, Settings, UserRound } from 'lucide-react'
import type { DragSource, DropTarget, IconTarget, Item, Snapshot, TabInfo, Zone } from '@shared/types'
import { FavoriteTile, FolderRow, GroupRow, ItemRow } from '../tabs/TabRow'
import type { Indicator } from '../tabs/TabRow'
import { SpaceForm } from '../settings/SpaceForm'
import { IconForm, TabEditContext } from '../tabs/TabCustom'
import { MediaCard } from '../ui/MediaCard'
import { SpacePreview } from './SpacePreview'
import { ArchiveToast } from '../overlay/Toast'
import { switchSpace, useOptimistic } from '../lib/optimistic'
import { useSpaceSwipe } from '../lib/useSpaceSwipe'
import { SpaceStrip } from './SpaceStrip'
import { aroundRun, isBlock, runsOf, todayMark, type Run } from '../tabs/groups'
import { reducedMotion, rubberband, spring, unrubberband } from '../lib/spring'
import { layoutPos, useFlip } from '../lib/flip'
import { DownloadFlight, DownloadsPanel, progressOf, useNewDownload, useUnseenDownloads } from '../windows/Downloads'
import { useExit } from '../lib/useExit'
import { DRAG_FAVORITE_MIME, DRAG_MIME } from '../lib/util'

// Same bounds as main (store.ts): 180-420 px, the rubber band covers the rest during the gesture.
const SIDEBAR_MIN = 180
const SIDEBAR_MAX = 420

const itemKey = (i: Item) => (i.kind === 'tab' ? i.tabId : i.id)
/** Maximum folder indentation depth. */
const MAX_DEPTH = 6

/** Drop target, with `into` = id of the folder receiving the item (to highlight its row). */
type Drop = DropTarget & { into?: string }

export const itemTabIds = (i: Item): string[] => (i.kind === 'tab' ? [i.tabId] : i.kind === 'split' ? i.tabIds : i.items.flatMap(itemTabIds))

/** Item (tab or shared view) that contains the tab, at any depth. */
export function leafOf(items: Item[], tabId: string): Item | null {
  for (const i of items) {
    if (i.kind === 'folder') {
      const f = leafOf(i.items, tabId)
      if (f) return f
    } else if (itemTabIds(i).includes(tabId)) return i
  }
  return null
}

/** Ids of the folder and its subfolders: forbidden targets while it is being dragged. */
export function folderIds(i: Item): string[] {
  return i.kind === 'folder' ? [i.id, ...i.items.flatMap(folderIds)] : []
}

export function findFolder(items: Item[], id: string): Item | null {
  for (const i of items) {
    if (i.kind !== 'folder') continue
    if (i.id === id) return i
    const f = findFolder(i.items, id)
    if (f) return f
  }
  return null
}

/** Keeps the current drop target while the pointer is over the gap (otherwise the list would see "at the end"). */
const keepOver = (e: DragEvent) => {
  e.preventDefault()
  e.stopPropagation()
  e.dataTransfer.dropEffect = 'move'
}

/** Insertion gap during a drag: neighbors move aside (FLIP) and the held item will land in it. */
function GapSlot({ h, tile }: { h?: number; tile: boolean }) {
  return (
    <div
      className={`gap-slot${tile ? ' tile-slot' : ''}`}
      data-flip-id="gap"
      data-flip-fade
      style={h ? { height: h } : undefined}
      onDragOver={keepOver}
      aria-hidden
    />
  )
}

/** Folder body: its height unfolds (0fr → 1fr); the content stays mounted while it collapses. */
function FolderBody({
  open,
  peek,
  flat,
  onDragOver,
  children
}: {
  open: boolean
  peek: ReactNode
  flat: boolean
  onDragOver?: (e: DragEvent) => void
  children: ReactNode
}) {
  const [mounted, setMounted] = useState(open)
  useEffect(() => {
    if (open) return setMounted(true)
    if (peek) return setMounted(false)
    const t = setTimeout(() => setMounted(false), 220)
    return () => clearTimeout(t)
  }, [open, peek])
  return (
    <div className={`folder-body${open || peek ? ' open' : ''}`} onDragOver={onDragOver}>
      <div className="folder-body-inner">
        <div className={`folder-children${flat ? ' flat' : ''}`}>{open || mounted ? children : peek}</div>
      </div>
    </div>
  )
}

/** Active tab pill: a single element that slides to the active row of the same list, otherwise fades. */
function ActivePill({
  pageRef,
  activeId,
  spaceId,
  pinned,
  today,
  width,
  layoutKey
}: {
  pageRef: RefObject<HTMLDivElement | null>
  activeId: string | null
  spaceId: string
  pinned: Item[]
  today: Item[]
  width: number
  /** Changes when rows are repositioned without the active tab changing (drag gap, FLIP return). */
  layoutKey: string
}) {
  const pill = useRef<HTMLDivElement>(null)
  const last = useRef<{ id: string | null; list: Element | null; key: string }>({ id: null, list: null, key: layoutKey })
  const place = useRef<() => void>(() => {})
  // Displayed geometry, velocities (px/s) and running springs: a new target restarts from the velocity reached.
  const cur = useRef({ x: 0, y: 0, w: 0, h: 0 })
  const vel = useRef({ x: 0, y: 0, w: 0, h: 0 })
  const stops = useRef<Partial<Record<'x' | 'y' | 'w' | 'h', () => void>>>({})
  // Last values written: style (and above all width/height, which trigger layout) is only touched when they change.
  const written = useRef({ t: '', w: '', h: '' })
  // Last requested target: an identical target within 0.5 px does not restart the springs.
  const target = useRef<{ x: number; y: number; w: number; h: number } | null>(null)
  const paint = () => {
    const el = pill.current
    if (!el) return
    const c = cur.current
    const wr = written.current
    const t = `translate(${c.x}px, ${c.y}px)`
    const w = `${c.w}px`
    const h = `${c.h}px`
    if (t !== wr.t) el.style.transform = wr.t = t
    if (w !== wr.w) el.style.width = wr.w = w
    if (h !== wr.h) el.style.height = wr.h = h
  }
  const glide = (to: { x: number; y: number; w: number; h: number }, animate: boolean) => {
    for (const k of ['x', 'y', 'w', 'h'] as const) {
      stops.current[k]?.()
      delete stops.current[k]
      if (!animate || Math.abs(to[k] - cur.current[k]) < 0.5) {
        cur.current[k] = to[k]
        vel.current[k] = 0
        continue
      }
      stops.current[k] = spring({
        from: cur.current[k],
        to: to[k],
        velocity: vel.current[k],
        response: 0.32,
        onUpdate: (v, vv) => {
          cur.current[k] = v
          vel.current[k] = vv
          paint()
        },
        onDone: () => {
          delete stops.current[k]
          vel.current[k] = 0
        }
      })
    }
    paint()
  }
  useEffect(() => () => Object.values(stops.current).forEach((stop) => stop?.()), [])
  place.current = () => {
    const page = pageRef.current
    const el = pill.current
    if (!page || !el) return
    const row = page.querySelector<HTMLElement>('.row.active:not(.leaving):not(.leaving .row):not(.ghost):not(.drag-src):not(.drag-src .row)')
    if (!row) {
      el.style.opacity = '0'
      last.current = { id: null, list: null, key: layoutKey }
      return
    }
    const list = row.closest('.list')
    const prev = last.current
    const moved = prev.id !== null && prev.id !== activeId
    // Slides only within the same list; otherwise the pill settles and fades in.
    // A move already in progress (row moving during the drag) is retargeted without restarting from zero.
    const running = Object.keys(stops.current).length > 0
    const relaid = prev.key !== layoutKey
    const slide =
      ((moved && prev.list === list) || (running && prev.list === list) || (relaid && prev.id === activeId && prev.list === list)) && !reducedMotion.matches
    // Final position of the row: without the current FLIP offset, which is only decoration.
    const r = layoutPos(row, page)
    const to = { x: r.x, y: r.y, w: row.offsetWidth, h: row.offsetHeight }
    const tg = target.current
    const same = !!tg && (['x', 'y', 'w', 'h'] as const).every((k) => Math.abs(to[k] - tg[k]) < 0.5)
    // Same target as the previous call and drag intended: the running springs already lead there, so we do not restart them.
    if (!(same && slide)) glide(to, slide)
    target.current = to
    el.style.borderRadius = getComputedStyle(row).borderTopLeftRadius
    el.style.opacity = '1'
    if (moved && !slide && !reducedMotion.matches) el.animate([{ opacity: 0 }, { opacity: 1 }], { duration: 120 })
    last.current = { id: activeId, list, key: layoutKey }
  }
  // Re-reads the geometry only when the target may have moved; the rest (folder unfolding, height) goes through the observer.
  useLayoutEffect(() => place.current(), [activeId, spaceId, pinned, today, width, layoutKey])
  // Rows also move without the active tab changing (folder unfolding, row added).
  useEffect(() => {
    const page = pageRef.current
    if (!page) return
    const ro = new ResizeObserver(() => place.current())
    ro.observe(page)
    return () => ro.disconnect()
  }, [pageRef])
  return <div ref={pill} className="active-pill" aria-hidden />
}
type FolderItem = Extract<Item, { kind: 'folder' }>

/** Stable handlers passed to the lists (they always read the current state, see `latest` in `Sidebar`). */
interface ListApi {
  onDragStart: (e: DragEvent, src: DragSource) => void
  itemOver: (zone: Zone, i: number, horizontal: boolean, folderId?: string, run?: Run) => (e: DragEvent) => void
  groupOver: (run: Run) => (e: DragEvent) => void
  groupHeadOver: (run: Run) => (e: DragEvent) => void
  folderOver: (id: string, i: number, parentId?: string) => (e: DragEvent) => void
  folderBody: (folder: FolderItem) => (e: DragEvent) => void
  listProps: (zone: Zone, length: number) => { onDragOver: (e: DragEvent) => void; onDrop: (e: DragEvent) => void; onDragLeave: (e: DragEvent) => void }
  onRename: (id: string) => void
  onDone: () => void
}

/** What the lists depend on: if these references do not change, list rendering is skipped. */
interface ListState {
  tabs: Record<string, TabInfo>
  activeId: string | null
  /** Drop target shown, only if it targets this list. */
  drop: Drop | null
  srcId: string | null
  renaming: string | null
  gapped: boolean
  fresh: Set<string>
  gapH: RefObject<number>
  api: ListApi
}

/** Insertion marker of row `i` in a list of `len` items. */
function indicatorOf(d: Drop | null, zone: Zone, i: number, len: number, folderId?: string): Indicator {
  if (!d || d.zone !== zone || d.folderId !== folderId || d.into) return null
  if (d.index === i) return 'before'
  return i === len - 1 && d.index === len ? 'after' : null
}

/** Insertion gap around an item (without reduced motion: the row's insertion marker). */
function withGap(p: ListState, key: string, ind: Indicator, node: ReactNode, tile = false) {
  const gap = <GapSlot h={tile ? undefined : p.gapH.current} tile={tile} />
  return (
    <Fragment key={key}>
      {p.gapped && ind === 'before' && gap}
      {node}
      {p.gapped && ind === 'after' && gap}
    </Fragment>
  )
}
const markOf = (p: ListState, ind: Indicator): Indicator => (p.gapped ? null : ind)

function renderItems(p: ListState, empty: string, zone: Zone, items: Item[], folderId?: string, depth = 0, parentHue?: number): ReactNode {
  const { activeId, srcId, drop, api } = p
  return items.map((item, i) => {
    if (item.kind === 'folder') {
      const inside = activeId ? itemTabIds(item).includes(activeId) : false
      // The active tab of a closed folder stays visible under the row of the topmost closed folder.
      const shown = !item.open && inside && activeId ? leafOf(item.items, activeId) : null
      const ind = indicatorOf(drop, zone, i, items.length, folderId)
      return withGap(
        p,
        item.id,
        ind,
        <div
          className={`folder${item.hue !== undefined ? ' tinted' : ''}${item.hue !== undefined && item.hue === parentHue ? ' same' : ''}${srcId === item.id ? ' drag-src' : ''}`}
          style={item.hue !== undefined ? { ['--fh' as string]: item.hue } : undefined}
          data-flip-id={item.id}
          onDragOver={item.open ? api.folderBody(item) : undefined}
        >
          <FolderRow
            folder={item}
            indicator={markOf(p, ind)}
            into={drop?.into === item.id}
            editing={p.renaming === item.id}
            onRename={api.onRename}
            onDone={api.onDone}
            onDragStart={api.onDragStart}
            onDragOver={api.folderOver(item.id, i, folderId)}
          />
          <FolderBody
            open={item.open}
            flat={depth + 1 >= MAX_DEPTH}
            onDragOver={item.open ? api.folderBody(item) : undefined}
            peek={
              shown ? (
                <ItemRow
                  item={shown}
                  tabs={p.tabs}
                  activeTabId={activeId}
                  zone={zone}
                  indicator={null}
                  fresh={shown.kind === 'tab' && p.fresh.has(shown.tabId)}
                  dragSrc={srcId === itemKey(shown)}
                  onDragStart={api.onDragStart}
                  onDragOver={api.folderOver(item.id, i, folderId)}
                />
              ) : null
            }
          >
            {renderItems(p, empty, zone, item.items, item.id, depth + 1, item.hue)}
            {item.items.length === 0 && <span className="folder-empty">{empty}</span>}
          </FolderBody>
        </div>
      )
    }
    const ind = indicatorOf(drop, zone, i, items.length, folderId)
    return withGap(
      p,
      itemKey(item),
      ind,
      <ItemRow
        item={item}
        tabs={p.tabs}
        activeTabId={activeId}
        zone={zone}
        indicator={markOf(p, ind)}
        fresh={item.kind === 'tab' && p.fresh.has(item.tabId)}
        dragSrc={srcId === itemKey(item)}
        onDragStart={api.onDragStart}
        onDragOver={api.itemOver(zone, i, false, folderId)}
      />
    )
  })
}

/** Favorites (tiles). */
const FavoritesList = memo(function FavoritesList(p: ListState & { ids: string[]; listRef: RefObject<HTMLDivElement | null> }) {
  const { t } = useTranslation()
  return (
    <div ref={p.listRef} className={`favorites${p.ids.length === 0 ? ' empty-zone' : ''}`} {...p.api.listProps('favorites', p.ids.length)}>
      {p.ids.map((id, i) => {
        const tab = p.tabs[id]
        const ind = indicatorOf(p.drop, 'favorites', i, p.ids.length)
        return tab
          ? withGap(
              p,
              id,
              ind,
              <FavoriteTile
                tab={tab}
                active={id === p.activeId}
                dragSrc={p.srcId === id}
                indicator={markOf(p, ind)}
                onDragStart={p.api.onDragStart}
                onDragOver={p.api.itemOver('favorites', i, true)}
              />,
              true
            )
          : null
      })}
      {p.ids.length === 0 && <span className="hint">{t('sidebar.dropFavorites')}</span>}
    </div>
  )
})

/** Pinned tabs and folders. */
const PinnedList = memo(function PinnedList(p: ListState & { items: Item[]; over: boolean; hint: boolean }) {
  const { t } = useTranslation()
  return (
    <div className={`list pinned${p.over ? ' over' : ''}`} {...p.api.listProps('pinned', p.items.length)}>
      {renderItems(p, t('sidebar.emptyFolder'), 'pinned', p.items)}
      {p.hint && <span className="hint">{t('sidebar.dropPin')}</span>}
    </div>
  )
})

/** Today's tabs: each group is a card with its header; ranks stay those of the flat list. */
const TodayList = memo(function TodayList(p: ListState & { items: Item[]; groups: Snapshot['spaces'][number]['groups'] }) {
  const { items, groups, activeId, srcId, drop, api } = p
  const tm = todayMark(items, drop)
  const row = (i: number, run?: Run, ind?: Indicator) =>
    withGap(
      p,
      itemKey(items[i]),
      ind ?? null,
      <ItemRow
        item={items[i]}
        tabs={p.tabs}
        activeTabId={activeId}
        zone="today"
        indicator={markOf(p, ind ?? null)}
        fresh={items[i].kind === 'tab' && p.fresh.has(itemKey(items[i]))}
        dragSrc={srcId === itemKey(items[i])}
        onDragStart={api.onDragStart}
        onDragOver={api.itemOver('today', i, false, undefined, run)}
      />
    )
  return (
    <div className="list today" {...api.listProps('today', items.length)}>
      {runsOf(items).map((run) => {
        const markAt = (i: number) => (tm?.index === i && !tm.card ? tm.side : null)
        if (!run.group) return row(run.start, undefined, markAt(run.start))
        const header = groups?.[run.group] ?? { name: '', hue: 215, open: true }
        // Insertion right before the group: the marker goes on the header, not on the first tab.
        const beforeCard = tm?.side === 'before' && tm.index === run.start && drop?.groupId !== run.group
        // Insertion right after the card (end of list): the marker goes after it, not inside the group.
        const afterCard = !!tm?.card && tm.index === run.end - 1
        const members = items.slice(run.start, run.end)
        const shown = !header.open && activeId ? members.findIndex((m) => itemTabIds(m).includes(activeId)) : -1
        const rows: ReactNode[] = []
        for (let i = run.start; i < run.end; i++) rows.push(row(i, run, i === run.start && beforeCard ? null : markAt(i)))
        return withGap(
          p,
          `g:${run.group}`,
          beforeCard ? 'before' : afterCard ? 'after' : null,
          <>
            <div
              className={`folder tinted tgroup${srcId === `g:${run.group}` ? ' drag-src' : ''}`}
              data-flip-id={`g:${run.group}`}
              style={{ ['--fh' as string]: header.hue }}
              onDragOver={api.groupOver(run)}
            >
              <GroupRow
                groupId={run.group}
                group={header}
                first={p.tabs[itemTabIds(members[0])[0]]}
                indicator={markOf(p, beforeCard ? 'before' : null)}
                into={drop?.into === run.group}
                editing={p.renaming === run.group}
                onRename={api.onRename}
                onDone={api.onDone}
                onDragStart={api.onDragStart}
                onDragOver={api.groupHeadOver(run)}
              />
              <FolderBody
                open={header.open}
                flat={false}
                onDragOver={header.open ? api.groupOver(run) : undefined}
                peek={shown >= 0 ? row(run.start + shown, run) : null}
              >
                {rows}
              </FolderBody>
            </div>
            {afterCard && !p.gapped && (
              <div aria-hidden style={{ position: 'relative', height: 0 }}>
                <span
                  style={{ position: 'absolute', left: 'var(--s2)', right: 'var(--s2)', top: 0, height: 2, borderRadius: 1, background: 'var(--accent)' }}
                />
              </div>
            )}
          </>
        )
      })}
    </div>
  )
})

export function Sidebar({ s, floating = false }: { s: Snapshot; floating?: boolean }) {
  const { t } = useTranslation()
  const signet = window.signet
  // Optimistic selection: the chosen space or tab is displayed before the main state arrives.
  const opt = useOptimistic()
  const space = s.spaces.find((x) => x.id === (opt.spaceId ?? s.activeSpaceId)) ?? s.spaces.find((x) => x.id === s.activeSpaceId) ?? s.spaces[0]
  const activeId = opt.tabId ?? space.activeTabId
  const right = s.settings.sidebarSide === 'right'
  const scrollRef = useRef<HTMLDivElement>(null)
  // The active tab stays visible (e.g. a new tab added at the bottom of a long list).
  useEffect(() => {
    const el = scrollRef.current?.querySelector<HTMLElement>('.row.active')
    el?.scrollIntoView({ block: 'nearest', behavior: reducedMotion.matches ? 'auto' : 'smooth' })
  }, [activeId, space.today.length])

  const [form, setForm] = useState<{ spaceId: string | null } | null>(null)
  const [drop, setDrop] = useState<Drop | null>(null)
  const [renaming, setRenaming] = useState<string | null>(null)
  const [editingTab, setEditingTab] = useState<string | null>(null)
  const [iconFor, setIconFor] = useState<IconTarget | null>(null)
  const [dragging, setDragging] = useState(false)
  // Id (FLIP) of the held item: its place collapses. `settling`: signature of the data at drop time, until the main state arrives.
  const [src, setSrc] = useState<string | null>(null)
  const [settling, setSettling] = useState<string | null>(null)
  const [overSpace, setOverSpace] = useState<string | null>(null)
  const [width, setWidth] = useState<number | null>(null)
  const [dlOpen, setDlOpen] = useState(false)
  const editCtx = useMemo(() => ({ editing: editingTab, setEditing: setEditingTab }), [editingTab])
  const [flying, setFlying] = useState<number | null>(null)
  const [bouncing, setBouncing] = useState(false)
  const pageRef = useRef<HTMLDivElement>(null)
  const favRef = useRef<HTMLDivElement>(null)
  const gapH = useRef(0) // height of the held item: that of the gap being opened
  const dataSigRef = useRef('')
  const settleRef = useRef(false)
  // The drop happened in the sidebar (list or space dot): the only case where the place collapses while waiting for main.
  const droppedHere = useRef(false)
  const landRef = useRef<string | null>(null)
  const seenTabs = useRef<{ space: string; ids: Set<string> } | null>(null)
  const dragRef = useRef<DragSource | null>(null)
  const insideRef = useRef(true)
  const blockedRef = useRef(false)
  const dropRef = useRef<Drop | null>(null)
  const forbiddenRef = useRef<Set<string>>(new Set())
  // The hover targets the inside of the dragged folder: the drop must do nothing.
  const forbiddenHitRef = useRef(false)
  const resizing = useRef(false)
  const widthFrame = useRef(0)
  const pendingWidth = useRef<number | null>(null)
  const curWidth = useRef<number | null>(null) // last width written during the gesture
  const springStop = useRef<(() => void) | null>(null)
  const lastMove = useRef({ w: 0, t: 0, v: 0 }) // edge velocity (px/s) for the release spring
  const startWidth = useRef(0) // width at grab time (Escape restores it)
  const rawBias = useRef(0) // gap between the raw width and the displayed width at grab time (rubber band return in progress)
  const escOff = useRef<(() => void) | null>(null)
  const cancelled = useRef(false) // Escape pressed: the rest of the gesture is ignored until release
  const grab = useRef(0) // gap between the pointer and the right edge at grab time
  const rootRef = useRef<HTMLElement>(null)
  const clipRef = useRef<HTMLDivElement>(null)
  const spaceIndex = s.spaces.indexOf(space)
  useSpaceSwipe({
    rootRef,
    clipRef,
    track: '.space-track',
    pos: '.space-strip',
    spaceIds: s.spaces.map((x) => x.id),
    activeId: space.id,
    onSwitch: switchSpace
  })

  useEffect(
    () =>
      signet.onEditSpace((req) => {
        setDlOpen(false)
        setIconFor(null)
        setForm({ spaceId: req.spaceId })
      }),
    [signet]
  )

  useEffect(() => signet.onRenameFolder((req) => setRenaming(req.folderId)), [signet])
  useEffect(
    () =>
      signet.onRenameTab((req) => {
        setDlOpen(false)
        // A favorite (tile) has no in-place field: renamed through the popover.
        const row = Array.from(document.querySelectorAll<HTMLElement>('[data-flip-id]')).find((n) => n.dataset.flipId === req.tabId)
        if (!row || !row.querySelector('.title')) {
          setEditingTab(null)
          const r = row?.getBoundingClientRect()
          void signet.openPopover(
            { kind: 'rename', target: { kind: 'tab', tabId: req.tabId } },
            r ? { x: r.left, y: r.top, width: r.width, height: r.height } : { x: 0, y: 0, width: 0, height: 0 }
          )
          return
        }
        setEditingTab(req.tabId)
      }),
    [signet]
  )
  useEffect(
    () =>
      signet.onPickIcon((target) => {
        setDlOpen(false)
        setForm(null)
        setIconFor(target)
      }),
    [signet]
  )

  // Floating bar: moving the mouse out closes it, unless a drag is in progress or a panel is open.
  blockedRef.current = !!form || !!iconFor || !!editingTab || dlOpen || dragging
  useEffect(() => {
    if (!floating) return
    const el = document.documentElement
    const blocked = () => blockedRef.current || !!dragRef.current
    const enter = () => {
      insideRef.current = true
      void signet.setSidebarHover(true)
    }
    const leave = () => {
      insideRef.current = false
      if (!blocked()) void signet.setSidebarHover(false)
    }
    // During a drag, mouse events are suspended: we follow the drag events instead.
    const dragIn = () => {
      insideRef.current = true
    }
    const dragOut = (e: globalThis.DragEvent) => {
      if (!e.relatedTarget) insideRef.current = false
    }
    el.addEventListener('mouseenter', enter)
    el.addEventListener('mouseleave', leave)
    window.addEventListener('dragenter', dragIn)
    window.addEventListener('dragover', dragIn)
    window.addEventListener('dragleave', dragOut)
    return () => {
      el.removeEventListener('mouseenter', enter)
      el.removeEventListener('mouseleave', leave)
      window.removeEventListener('dragenter', dragIn)
      window.removeEventListener('dragover', dragIn)
      window.removeEventListener('dragleave', dragOut)
    }
  }, [signet, floating])

  // The lock just dropped while the mouse is already outside: close.
  useEffect(() => {
    if (floating && !form && !iconFor && !editingTab && !dlOpen && !dragging && !insideRef.current) void signet.setSidebarHover(false)
  }, [signet, floating, form, iconFor, editingTab, dlOpen, dragging])

  // Panels: stay mounted while their exit animation runs.
  const dlEx = useExit(dlOpen)
  const iconEx = useExit(iconFor)
  const formEx = useExit(form)

  const newDownload = useNewDownload(s.downloads)
  useEffect(() => {
    if (!newDownload || floating || reducedMotion.matches) return
    setFlying(newDownload.key)
  }, [newDownload, floating])

  const unseen = useUnseenDownloads(s.downloads, dlOpen)
  const progress = progressOf(s.downloads)
  const closeDownloads = useCallback(() => setDlOpen(false), [])

  // End of drag: wherever it ends, reset everything.
  useEffect(() => {
    const end = (e: Event) => {
      if (dragRef.current?.kind === 'tab') window.signet.endTabDrag().catch(() => {})
      // Drop: the gap and the collapsed place stay until the main state arrives (or a short delay),
      // so the item lands in the gap instead of reappearing at its old place.
      const dropped = e.type === 'drop' && dragRef.current !== null && droppedHere.current
      droppedHere.current = false
      const afterDrop = e.type === 'dragend' && settleRef.current
      settleRef.current = dropped
      dragRef.current = null
      forbiddenRef.current = new Set()
      dropRef.current = null
      forbiddenHitRef.current = false
      setDragging(false)
      setOverSpace(null)
      if (dropped) return setSettling(dataSigRef.current)
      if (afterDrop) return
      setDrop(null)
      setSrc(null)
      setSettling(null)
    }
    window.addEventListener('dragend', end)
    window.addEventListener('drop', end)
    return () => {
      window.removeEventListener('dragend', end)
      window.removeEventListener('drop', end)
    }
  }, [])

  useEffect(
    () => () => {
      cancelAnimationFrame(widthFrame.current)
      springStop.current?.()
      escOff.current?.()
    },
    []
  )

  // Release the local width once the main state has caught up.
  useEffect(() => {
    if (!resizing.current) setWidth(null)
  }, [s.sidebarWidth])

  // Tabs that arrived since the last render (excluding space changes): their row unfolds.
  const tabIds = useMemo(() => [...space.pinned, ...space.today].flatMap(itemTabIds), [space.pinned, space.today])
  const freshRef = useRef<Map<string, number>>(new Map())
  const lastFresh = useRef<Set<string>>(new Set())
  const fresh = useMemo(() => {
    const seen = seenTabs.current
    const now = Date.now()
    if (!seen || seen.space !== space.id) freshRef.current.clear()
    else for (const id of tabIds) if (!seen.ids.has(id)) freshRef.current.set(id, now)
    // A row stays "fresh" for the duration of its animation, even if another render happens.
    for (const [id, at] of freshRef.current) if (now - at > 400) freshRef.current.delete(id)
    // Same content: keep the same Set, so the lists do not re-render.
    const ids = new Set(freshRef.current.keys())
    const prev = lastFresh.current
    if (prev.size === ids.size && [...ids].every((id) => prev.has(id))) return prev
    lastFresh.current = ids
    return ids
  }, [tabIds, space.id])
  useEffect(() => {
    seenTabs.current = { space: space.id, ids: new Set(tabIds) }
  }, [tabIds, space.id])

  /* ---------- list motion (FLIP, drag gap) ---------- */

  // List structure: what, when it changes, moves rows (order, open folders and groups, favorites).
  const dataSig = useMemo(() => {
    const sig = (items: Item[]): string =>
      items.map((i) => (i.kind === 'folder' ? `${i.id}${i.open ? '+' : '-'}(${sig(i.items)})` : `${itemKey(i)}${i.group ? `@${i.group}` : ''}`)).join(',')
    const groups = Object.entries(space.groups ?? {})
      .map(([id, g]) => `${id}${g.open ? '+' : '-'}`)
      .join(',')
    return `${sig(space.pinned)}|${sig(space.today)}|${groups}|${s.favorites.join(',')}`
  }, [space.pinned, space.today, space.groups, s.favorites])
  dataSigRef.current = dataSig

  // Pending drop: as soon as the data has changed (or after a short delay), the gap and the collapsed place disappear.
  const settled = settling !== null && settling !== dataSig
  const clearSettle = useCallback(() => {
    settleRef.current = false
    setSettling(null)
    setSrc(null)
    setDrop(null)
  }, [])
  useEffect(() => {
    if (settling === null) return
    if (settled) return clearSettle()
    const id = setTimeout(clearSettle, 450)
    return () => clearTimeout(id)
  }, [settling, settled, clearSettle])
  const live = settling === null || !settled
  const srcId = live ? src : null
  const dropShown = live ? drop : null
  if (srcId) landRef.current = srcId
  const reduceMotion = reducedMotion.matches
  // Reduced motion: no gap, only the insertion marker.
  const gapped = !reduceMotion

  // The signature includes the gap: a single mechanism animates both the rows moving aside and those being reordered.
  const dropSig = dropShown && !dropShown.into ? `${dropShown.zone}/${dropShown.folderId ?? ''}/${dropShown.groupId ?? ''}/${dropShown.index}` : ''
  useFlip(pageRef, `${dataSig}#${srcId ?? ''}#${gapped ? dropSig : ''}`, space.id, landRef.current)
  useFlip(favRef, `${s.favorites.join(',')}#${srcId ?? ''}#${gapped && dropShown?.zone === 'favorites' ? dropShown.index : ''}`, space.id, landRef.current)

  /* ---------- drag and drop ---------- */

  const putDrop = (t: Drop | null) => {
    forbiddenHitRef.current = !!t?.folderId && forbiddenRef.current.has(t.folderId)
    if (forbiddenHitRef.current) t = null
    const p = dropRef.current
    if (
      p?.zone === t?.zone &&
      p?.index === t?.index &&
      p?.spaceId === t?.spaceId &&
      p?.folderId === t?.folderId &&
      p?.into === t?.into &&
      p?.groupId === t?.groupId
    )
      return
    dropRef.current = t
    setDrop(t)
  }
  /**
   * Final rectangle of an item (without the current FLIP offsets): rows slide under the pointer when the gap changes
   * place, and it is their arrival place that must decide the side, otherwise a back-and-forth sets in.
   */
  const finalRect = (el: HTMLElement) => {
    const root = [pageRef.current, favRef.current].find((n) => n?.contains(el))
    const r = el.getBoundingClientRect()
    if (!root) return r
    const b = root.getBoundingClientRect()
    const p = layoutPos(el, root)
    return { left: b.left + p.x, top: b.top + p.y, width: r.width, height: r.height }
  }
  /** Dead band (px) around the middle of a row: as long as we stay in it, the current target is kept. */
  const DEAD_BAND = 4
  /** Is the pointer past the middle? In the dead band, we keep the side of the current target if it touches this row. */
  const sideOf = (pos: number, mid: number, current: number | null, index: number): boolean => {
    if (current !== null && (current === index || current === index + 1) && Math.abs(pos - mid) <= DEAD_BAND) return current === index + 1
    return pos > mid
  }
  const accepts = (zone: Zone) => {
    const kind = dragRef.current?.kind
    if (!kind) return false
    // A group dropped into the pinned tabs becomes a folder; a folder dropped into today's tabs, a group.
    return kind === 'tab' || zone !== 'favorites'
  }

  const startDrag = (e: DragEvent, src: DragSource) => {
    e.dataTransfer.setData(DRAG_MIME, JSON.stringify(src))
    e.dataTransfer.effectAllowed = 'move'
    dragRef.current = src
    const dragged = src.kind === 'folder' ? findFolder(space.pinned, src.id) : null
    forbiddenRef.current = new Set(dragged ? folderIds(dragged) : [])
    // Gap height: that of the held item (the whole card for a folder or a group).
    const el = e.currentTarget as HTMLElement
    gapH.current = (src.kind === 'folder' || src.kind === 'group' ? (el.closest('.folder') ?? el) : el).getBoundingClientRect().height
    // Modifying the DOM (or native views) during dragstart would cancel the drag; the drag image is already taken afterwards.
    setTimeout(() => {
      clearSettle()
      setDragging(true)
      if (dragRef.current === src) setSrc(src.kind === 'tab' ? src.tabId : src.kind === 'group' ? `g:${src.id}` : src.id)
    }, 0)
    if (src.kind === 'tab') {
      // A favorite can only be displayed (central zone): the MIME type signals it to the overlay, which cannot read the data before the drop.
      if (s.favorites.includes(src.tabId)) e.dataTransfer.setData(DRAG_FAVORITE_MIME, '1')
      setTimeout(() => dragRef.current === src && window.signet.beginTabDrag(src.tabId).catch(() => {}), 0)
    }
  }

  /**
   * Hover over a row or tile: insertion before or after depending on the hovered half.
   * In a group card (`run`), the dropped item joins the group; a dragged group, however, lands before or after the card.
   */
  const overItem =
    (zone: Zone, index: number, horizontal = false, folderId?: string, run?: Run) =>
    (e: DragEvent) => {
      if (!accepts(zone)) return
      e.preventDefault()
      e.stopPropagation()
      e.dataTransfer.dropEffect = 'move'
      const card = run?.group ? e.currentTarget.closest('.tgroup') : null
      if (run && card && isBlock(dragRef.current)) {
        putDrop({ zone, spaceId: space.id, index: aroundRun(card, run, e.clientY, false, dropRef.current?.index ?? null) })
        return
      }
      const r = finalRect(e.currentTarget as HTMLElement)
      const p = dropRef.current
      const cur = p && p.zone === zone && p.folderId === folderId && p.groupId === run?.group && !p.into && p.spaceId === space.id ? p.index : null
      const after = horizontal ? sideOf(e.clientX, r.left + r.width / 2, cur, index) : sideOf(e.clientY, r.top + r.height / 2, cur, index)
      putDrop({ zone, spaceId: space.id, folderId, index: index + (after ? 1 : 0), groupId: run?.group })
    }

  /** Hover over a group header: upper half = before the group, lower half = inside, at the top (including collapsed). */
  const overGroupHead = (run: Run) => (e: DragEvent) => {
    if (!accepts('today')) return
    e.preventDefault()
    e.stopPropagation()
    e.dataTransfer.dropEffect = 'move'
    const card = e.currentTarget.closest('.tgroup')
    if (isBlock(dragRef.current)) {
      if (card) putDrop({ zone: 'today', spaceId: space.id, index: aroundRun(card, run, e.clientY, false, dropRef.current?.index ?? null) })
      return
    }
    const r = finalRect(e.currentTarget as HTMLElement)
    const p = dropRef.current
    const wasInto = p?.zone === 'today' && p.into === run.group
    const mid = r.top + r.height / 2
    // Dead band: we keep "before" or "inside" as long as the pointer stays near the middle.
    const before = Math.abs(e.clientY - mid) <= DEAD_BAND && p?.zone === 'today' && p.index === run.start ? !wasInto : e.clientY < mid
    if (before) putDrop({ zone: 'today', spaceId: space.id, index: run.start })
    else putDrop({ zone: 'today', spaceId: space.id, index: run.start, groupId: run.group, into: run.group })
  }

  /** Hover over the margins of a group card: at the end of the group (or before/after the card for a dragged group). */
  const overGroup = (run: Run) => (e: DragEvent) => {
    if (!accepts('today')) return
    e.preventDefault()
    e.stopPropagation()
    e.dataTransfer.dropEffect = 'move'
    if (isBlock(dragRef.current))
      putDrop({ zone: 'today', spaceId: space.id, index: aroundRun(e.currentTarget, run, e.clientY, false, dropRef.current?.index ?? null) })
    else putDrop({ zone: 'today', spaceId: space.id, index: run.end, groupId: run.group })
  }

  /** Hover over a folder row: top quarter = before, bottom quarter = after, middle = inside (at the end). */
  const overFolder = (folderId: string, index: number, parentId?: string) => (e: DragEvent) => {
    const folder = findFolder(space.pinned, folderId)
    if (folder?.kind !== 'folder' || !accepts('pinned')) return
    e.preventDefault()
    e.stopPropagation()
    e.dataTransfer.dropEffect = 'move'
    const r = finalRect(e.currentTarget as HTMLElement)
    const y = (e.clientY - r.top) / r.height
    if (y > 0.25 && y < 0.75 && !forbiddenRef.current.has(folder.id)) {
      putDrop({ zone: 'pinned', spaceId: space.id, folderId: folder.id, index: folder.items.length, into: folder.id })
    } else {
      putDrop({ zone: 'pinned', spaceId: space.id, folderId: parentId, index: index + (y >= 0.5 ? 1 : 0) })
    }
  }

  /** Hover over the empty area of an open folder: append at the end of the folder. */
  const overFolderBody = (folder: Extract<Item, { kind: 'folder' }>) => (e: DragEvent) => {
    if (!accepts('pinned')) return
    e.preventDefault()
    e.stopPropagation()
    e.dataTransfer.dropEffect = 'move'
    putDrop({ zone: 'pinned', spaceId: space.id, folderId: folder.id, index: folder.items.length })
  }

  /** Hover over the empty area of a list: append at the end. */
  const overList = (zone: Zone, length: number) => (e: DragEvent) => {
    if (!accepts(zone)) return
    e.preventDefault()
    e.dataTransfer.dropEffect = 'move'
    putDrop({ zone, spaceId: space.id, index: zone === 'today' ? length : indexAt(e, zone === 'favorites', length) })
  }

  /** Insertion rank from the pointer, in the gaps of a list: first row whose middle is after it, otherwise the end. */
  const indexAt = (e: DragEvent, grid: boolean, length: number): number => {
    const rows = Array.from((e.currentTarget as HTMLElement).children).filter((c): c is HTMLElement => {
      const id = c.getAttribute('data-flip-id')
      return id !== null && id !== 'gap' && id !== 'divider' && id !== 'new-tab'
    })
    if (rows.length !== length) return length
    for (let i = 0; i < rows.length; i++) {
      const r = finalRect(rows[i])
      if (grid ? e.clientY < r.top || (e.clientY <= r.top + r.height && e.clientX < r.left + r.width / 2) : e.clientY < r.top + r.height / 2) return i
    }
    return length
  }

  const dropOnList = (zone: Zone, length: number) => (e: DragEvent) => {
    e.preventDefault()
    droppedHere.current = true
    if (forbiddenHitRef.current) return
    const raw = e.dataTransfer.getData(DRAG_MIME)
    if (!raw) return
    const src = JSON.parse(raw) as DragSource
    const t = dropRef.current
    void signet.moveItem(
      src,
      t && t.zone === zone
        ? { zone: t.zone, spaceId: t.spaceId, folderId: t.folderId, groupId: t.groupId, index: t.index }
        : { zone, spaceId: space.id, index: length }
    )
  }

  const leaveList = (e: DragEvent) => {
    if (!e.currentTarget.contains(e.relatedTarget as Node | null)) putDrop(null)
  }

  // The lists are memoized: their handlers must keep the same identity from one render to the next.
  // They go through `latest` to always read the current space.
  const latest = useRef({ startDrag, overItem, overFolder, overGroup, overGroupHead, overFolderBody, overList, dropOnList, leaveList })
  latest.current = { startDrag, overItem, overFolder, overGroup, overGroupHead, overFolderBody, overList, dropOnList, leaveList }
  const overCache = useRef(new Map<string, (e: DragEvent) => void>())
  // Keys depend on ranks: we start from scratch whenever the structure changes, so the cache does not grow.
  const overCacheSig = useRef(dataSig)
  if (overCacheSig.current !== dataSig) {
    overCacheSig.current = dataSig
    overCache.current.clear()
  }
  const api = useMemo<ListApi>(() => {
    const cached = (key: string, make: () => (e: DragEvent) => void) => {
      let f = overCache.current.get(key)
      if (!f) overCache.current.set(key, (f = make()))
      return f
    }
    return {
      onDragStart: (e, src) => latest.current.startDrag(e, src),
      itemOver: (zone, i, horizontal, folderId, run) =>
        cached(
          `i|${zone}|${i}|${horizontal}|${folderId}|${run ? `${run.group}:${run.start}:${run.end}` : ''}`,
          () => (e) => latest.current.overItem(zone, i, horizontal, folderId, run)(e)
        ),
      groupOver: (run) => cached(`g|${run.group}|${run.start}|${run.end}`, () => (e) => latest.current.overGroup(run)(e)),
      groupHeadOver: (run) => cached(`h|${run.group}|${run.start}|${run.end}`, () => (e) => latest.current.overGroupHead(run)(e)),
      folderOver: (id, i, parentId) => cached(`f|${id}|${i}|${parentId}`, () => (e) => latest.current.overFolder(id, i, parentId)(e)),
      folderBody: (folder) => (e) => latest.current.overFolderBody(folder)(e),
      listProps: (zone, length) => ({
        onDragOver: (e) => latest.current.overList(zone, length)(e),
        onDrop: (e) => latest.current.dropOnList(zone, length)(e),
        onDragLeave: (e) => latest.current.leaveList(e)
      }),
      onRename: setRenaming,
      onDone: () => setRenaming(null)
    }
  }, [])
  const st = { tabs: s.tabs, activeId, srcId, renaming, gapped, fresh, gapH, api }

  /* ---------- resizing ---------- */

  const onHandleDown = (e: PointerEvent<HTMLDivElement>) => {
    e.currentTarget.setPointerCapture(e.pointerId)
    // A new grab interrupts the rubber band return: we start again from the displayed width.
    springStop.current?.()
    springStop.current = null
    resizing.current = true
    cancelled.current = false
    // Edge pulled: the right one of the sidebar on the left, the left one of the sidebar on the right.
    const r = rootRef.current?.getBoundingClientRect()
    const edge = (right ? r?.left : r?.right) ?? e.clientX
    grab.current = e.clientX - edge
    // Displayed width: if it is out of bounds (rubber band return), we go back up to the raw width before reapplying resistance.
    const shownW = r?.width ?? width ?? s.sidebarWidth
    // The render width is frozen at the displayed width: during the gesture it is written directly to the style (see `applyWidth`).
    curWidth.current = shownW
    setWidth(shownW)
    const rawW =
      shownW > SIDEBAR_MAX
        ? SIDEBAR_MAX + unrubberband(shownW - SIDEBAR_MAX, 160)
        : shownW < SIDEBAR_MIN
          ? SIDEBAR_MIN + unrubberband(shownW - SIDEBAR_MIN, 160)
          : shownW
    rawBias.current = rawW - shownW
    startWidth.current = Math.min(SIDEBAR_MAX, Math.max(SIDEBAR_MIN, shownW))
    lastMove.current = { w: shownW, t: performance.now(), v: 0 }
    // Escape cancels the resize: back to the starting width.
    const onKey = (k: KeyboardEvent) => {
      if (k.key !== 'Escape' || !resizing.current) return
      k.preventDefault()
      k.stopPropagation()
      cancelled.current = true
      cancelAnimationFrame(widthFrame.current)
      widthFrame.current = 0
      const from = pendingWidth.current ?? curWidth.current ?? width ?? s.sidebarWidth
      pendingWidth.current = null
      lastMove.current.v = 0
      releaseTo(from, startWidth.current)
    }
    escOff.current?.()
    window.addEventListener('keydown', onKey, true)
    escOff.current = () => window.removeEventListener('keydown', onKey, true)
  }
  /** During the gesture, the width goes straight into the sidebar style: no React render per frame. */
  const applyWidth = (w: number) => {
    curWidth.current = w
    if (rootRef.current) rootRef.current.style.width = `${w}px`
  }
  // At most one update per frame; the final value always goes out on release.
  const flushWidth = () => {
    widthFrame.current = 0
    const w = pendingWidth.current
    pendingWidth.current = null
    if (w === null) return
    applyWidth(w)
    void signet.setSidebarWidth(w, true)
  }
  const onHandleMove = (e: PointerEvent<HTMLDivElement>) => {
    if (!resizing.current || cancelled.current) return
    const raw = (right ? window.innerWidth - (e.clientX - grab.current) : e.clientX - grab.current) + rawBias.current
    // Out of bounds, the displayed width resists (rubber band); main tolerates this overshoot during the gesture.
    const reduce = reducedMotion.matches
    const w = reduce
      ? Math.min(SIDEBAR_MAX, Math.max(SIDEBAR_MIN, raw))
      : raw < SIDEBAR_MIN
        ? SIDEBAR_MIN + rubberband(raw - SIDEBAR_MIN, 160)
        : raw > SIDEBAR_MAX
          ? SIDEBAR_MAX + rubberband(raw - SIDEBAR_MAX, 160)
          : raw
    pendingWidth.current = Math.round(w)
    const now = performance.now()
    const dt = now - lastMove.current.t
    if (dt > 0) lastMove.current = { w, t: now, v: lastMove.current.v * 0.5 + ((w - lastMove.current.w) / dt) * 1000 * 0.5 }
    if (!widthFrame.current) widthFrame.current = requestAnimationFrame(flushWidth)
  }
  /** Brings the width from `from` to `to` (clamped) with a spring starting at the edge velocity, then persists it. */
  const releaseTo = (from: number, to: number) => {
    escOff.current?.()
    escOff.current = null
    const settle = () => {
      springStop.current = null
      resizing.current = false
      applyWidth(to)
      curWidth.current = null
      setWidth(to)
      void signet.setSidebarWidth(to) // final value: clamped and persisted
    }
    if (from === to || reducedMotion.matches) {
      settle()
      return
    }
    const idle = performance.now() - lastMove.current.t > 100
    let pending = 0
    springStop.current = spring({
      from,
      to,
      velocity: idle ? 0 : lastMove.current.v,
      response: 0.3,
      onUpdate: (v) => {
        const r = Math.round(v)
        if (r === pending) return
        pending = r
        applyWidth(r)
        void signet.setSidebarWidth(r, true)
      },
      onDone: settle
    })
  }
  const onHandleUp = () => {
    if (!resizing.current || cancelled.current) return
    cancelAnimationFrame(widthFrame.current)
    widthFrame.current = 0
    const from = pendingWidth.current ?? curWidth.current ?? width ?? s.sidebarWidth
    pendingWidth.current = null
    releaseTo(from, Math.min(SIDEBAR_MAX, Math.max(SIDEBAR_MIN, from)))
  }

  /* ---------- render ---------- */

  const showFavorites = s.favorites.length > 0 || dragging
  const pinnedDrop = dropShown?.zone === 'pinned'

  return (
    <TabEditContext.Provider value={editCtx}>
      <aside ref={rootRef} className={`sidebar${floating ? ' floating' : ''}${right ? ' right' : ''}`} style={{ width: width ?? s.sidebarWidth }}>
        <div className="sb-head" onContextMenu={(e) => (e.preventDefault(), signet.showSpaceMenu(space.id))}>
          <span className="name">{space.name}</span>
          {space.profile === 'isolated' && (
            <span className="profile-badge" title={t('sidebar.isolatedProfile')}>
              <UserRound size={13} />
            </span>
          )}
          <span className="spacer" />
          {s.downloads.length > 0 && (
            <button
              className={`icon dl-btn${dlOpen ? ' on' : ''}${bouncing ? ' bounce' : ''}`}
              onAnimationEnd={(e) => e.target === e.currentTarget && setBouncing(false)}
              title={t('sidebar.downloads')}
              onClick={() => {
                setForm(null)
                setDlOpen((o) => !o)
              }}
            >
              <Download size={16} />
              {progress.active && (
                <svg className={`ring${progress.fraction === null ? ' spin' : ''}`} viewBox="0 0 28 28">
                  <circle cx="14" cy="14" r="12" pathLength="100" strokeDasharray={`${progress.fraction === null ? 28 : progress.fraction * 100} 100`} />
                </svg>
              )}
              {unseen && <i className="dot" />}
            </button>
          )}
          <button
            className="icon add-folder"
            title={t('sidebar.newFolder')}
            onClick={() => signet.createFolder()}
            onContextMenu={(e) => (e.preventDefault(), e.stopPropagation(), signet.showAddFolderMenu())}
          >
            <FolderPlus size={15} />
          </button>
        </div>

        <div ref={scrollRef} className="scroll">
          {showFavorites && <FavoritesList {...st} drop={dropShown?.zone === 'favorites' ? dropShown : null} ids={s.favorites} listRef={favRef} />}

          <div className="space-clip" ref={clipRef}>
            <div className="space-track">
              <SpacePreview space={s.spaces[spaceIndex - 1] ?? null} tabs={s.tabs} />
              <div className="space-page cur" key={space.id} ref={pageRef}>
                <PinnedList {...st} drop={pinnedDrop ? dropShown : null} items={space.pinned} over={pinnedDrop} hint={space.pinned.length === 0 && dragging} />

                <div className="divider" data-flip-id="divider">
                  <span className="line" />
                  {space.today.length > 0 && (
                    <button className="clean" onClick={() => signet.clearToday()}>
                      <Archive size={12} /> {t('sidebar.archive')}
                    </button>
                  )}
                </div>

                <button
                  className="row new-tab"
                  data-flip-id="new-tab"
                  onClick={() => signet.openPalette('new-tab')}
                  onContextMenu={(e) => {
                    e.preventDefault()
                    void signet.showNewMenu()
                  }}
                >
                  <Plus size={16} />
                  <span className="title">{t('sidebar.newTab')}</span>
                </button>

                <TodayList {...st} drop={dropShown?.zone === 'today' ? dropShown : null} items={space.today} groups={space.groups} />
                <ActivePill
                  pageRef={pageRef}
                  activeId={activeId}
                  spaceId={space.id}
                  pinned={space.pinned}
                  today={space.today}
                  width={width ?? s.sidebarWidth}
                  layoutKey={`${dataSig}#${srcId ?? ''}#${gapped ? dropSig : ''}`}
                />
              </div>
              <SpacePreview space={s.spaces[spaceIndex + 1] ?? null} tabs={s.tabs} />
            </div>
          </div>
        </div>

        <MediaCard media={s.media} tabs={s.tabs} />

        <div className="bottombar">
          <button className="icon" title={t('sidebar.settings')} onClick={() => signet.openSettings()}>
            <Settings size={16} />
          </button>
          <button
            className="icon"
            title={(() => {
              const k = shortcutLabel('library', s.settings.shortcuts, IS_MAC)
              return k ? t('sidebar.library', { keys: k }) : t('sidebar.libraryNoKeys')
            })()}
            onClick={() => signet.openLibrary()}
          >
            <Library size={16} />
          </button>
          <SpaceStrip activeIndex={spaceIndex}>
            {s.spaces.map((sp, i) => (
              <button
                key={sp.id}
                style={{ ['--i' as string]: i }}
                className={`space-dot${sp.id === space.id ? ' on' : ''}${overSpace === sp.id ? ' over' : ''}`}
                title={sp.name}
                aria-label={sp.name}
                onClick={() => switchSpace(sp.id)}
                onContextMenu={(e) => (e.preventDefault(), signet.showSpaceMenu(sp.id))}
                onDragOver={(e) => {
                  if (!dragRef.current) return
                  e.preventDefault()
                  setOverSpace(sp.id)
                }}
                onDragLeave={() => setOverSpace((o) => (o === sp.id ? null : o))}
                onDrop={(e) => {
                  e.preventDefault()
                  droppedHere.current = true
                  const raw = e.dataTransfer.getData(DRAG_MIME)
                  if (!raw) return
                  const src = JSON.parse(raw) as DragSource
                  // A folder only goes into the pinned tabs; the rest lands in today's tabs.
                  void signet.moveItem(
                    src,
                    src.kind === 'folder' ? { zone: 'pinned', spaceId: sp.id, index: sp.pinned.length } : { zone: 'today', spaceId: sp.id, index: 0 }
                  )
                }}
              >
                <span className="em">{sp.icon}</span>
              </button>
            ))}
          </SpaceStrip>
          <button
            className="icon"
            title={t('sidebar.newSpace')}
            onClick={() => {
              setDlOpen(false)
              setForm({ spaceId: null })
            }}
          >
            <Plus size={16} />
          </button>
        </div>

        <ArchiveToast />

        {flying !== null && newDownload && flying === newDownload.key && (
          <DownloadFlight
            d={newDownload.d}
            left={right ? 8 : s.sidebarWidth}
            right={right ? s.sidebarWidth : 8}
            onLand={() => {
              setFlying(null)
              setBouncing(true)
            }}
          />
        )}
        {dlEx.value && (
          <div className={`ex${dlEx.closing ? ' closing' : ''}`}>
            <DownloadsPanel downloads={s.downloads} onClose={closeDownloads} />
          </div>
        )}
        {iconEx.value && (
          <div className={`ex${iconEx.closing ? ' closing' : ''}`}>
            <IconForm key={JSON.stringify(iconEx.value)} s={s} target={iconEx.value} onClose={() => setIconFor(null)} />
          </div>
        )}
        {formEx.value && (
          <div className={`ex${formEx.closing ? ' closing' : ''}`}>
            <SpaceForm
              key={formEx.value.spaceId ?? 'new'}
              editId={formEx.value.spaceId}
              space={formEx.value.spaceId ? (s.spaces.find((x) => x.id === formEx.value?.spaceId) ?? null) : null}
              onClose={() => setForm(null)}
            />
          </div>
        )}

        <div className="resize" onPointerDown={onHandleDown} onPointerMove={onHandleMove} onPointerUp={onHandleUp} onPointerCancel={onHandleUp} />
      </aside>
    </TabEditContext.Provider>
  )
}

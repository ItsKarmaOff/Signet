import { Fragment, memo, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'
import type { DragEvent, WheelEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { IS_MAC } from '@signet/shared/i18n'
import { shortcutLabel } from '@shared/shortcuts'
import { ChevronDown, Columns2, Download, Grid2x2, Library, Plus, Rows2, Settings, UserRound, Volume2, VolumeX } from 'lucide-react'
import type { DragSource, DropTarget, Item, Rect, Snapshot, TabGroup, TabInfo, Zone } from '@shared/types'
import { EmojiIcon, FolderIcon, TabEditContext, TabIcon, displayTitle } from './TabCustom'
import { TabLine, tabHandlers } from './TabRow'
import { activateTab, useOptimistic } from '../lib/optimistic'
import type { Indicator } from './TabRow'
import { findFolder, folderIds, itemTabIds } from '../sidebar/Sidebar'
import { MediaChip } from '../ui/MediaCard'
import { progressOf, useUnseenDownloads } from '../windows/Downloads'
import { DRAG_FAVORITE_MIME, DRAG_MIME } from '../lib/util'
import { reducedMotion } from '../lib/spring'
import { useFlip } from '../lib/flip'
import { aroundRun, isBlock, runsOf, todayMark, type Run } from './groups'

type FolderItem = Extract<Item, { kind: 'folder' }>
/** Drop target, with `into` = id of the folder receiving the item (to highlight its pill). */
type Drop = DropTarget & { into?: string }

const FADE = 24
/** A dragged tab hovering for a while over the space switcher: the menu opens. */
const DWELL = 450
const rectOf = (el: Element): Rect => {
  const r = el.getBoundingClientRect()
  return { x: r.left, y: r.top, width: r.width, height: r.height }
}

const ind = (i: Indicator) => (i ? ` drop-${i}` : '')
type StartDrag = (e: DragEvent, src: DragSource) => void
type Over = (e: DragEvent) => void

/** Square pill for a favorite or pinned item: favicon (or emoji) only, title in a tooltip. */
const IconChip = memo(function IconChip({
  tab,
  active,
  fresh,
  mark,
  draggable,
  onDragStart,
  onDragOver
}: {
  tab: TabInfo
  active: boolean
  fresh: boolean
  mark: Indicator
  draggable: boolean
  onDragStart: StartDrag
  onDragOver: Over
}) {
  const { t } = useTranslation()
  const handlers = useMemo(() => tabHandlers(tab, () => activateTab(tab.id)), [tab])
  return (
    <div
      data-tab={tab.id}
      data-flip-id={tab.id}
      className={`tchip sq${active ? ' active' : ''}${tab.loaded ? '' : ' unloaded'}${fresh ? ' fresh' : ''}${ind(mark)}`}
      title={displayTitle(tab, t('sidebar.newTab'))}
      draggable={draggable}
      onDragStart={(e) => onDragStart(e, { kind: 'tab', tabId: tab.id })}
      onDragOver={onDragOver}
      {...handlers}
    >
      <TabIcon tab={tab} size={16} />
      {(tab.audible || tab.muted) && <span className="tchip-sound">{tab.muted ? <VolumeX size={9} /> : <Volume2 size={9} />}</span>}
    </div>
  )
})

/** Split view segment: one member tab. */
const SplitSeg = memo(function SplitSeg({ tab, active }: { tab: TabInfo; active: boolean }) {
  const { t } = useTranslation()
  const handlers = useMemo(() => tabHandlers(tab, () => void window.signet.focusPane(tab.id)), [tab])
  return (
    <div
      data-tab={tab.id}
      className={`tseg${active ? ' active' : ''}${tab.loaded ? '' : ' unloaded'}`}
      title={displayTitle(tab, t('sidebar.newTab'))}
      {...handlers}
    >
      <TabIcon tab={tab} size={16} />
    </div>
  )
})

type SplitProps = {
  item: Extract<Item, { kind: 'split' }>
  tabs: Record<string, TabInfo>
  activeId: string | null
  fresh: boolean
  mark: Indicator
  onDragStart: StartDrag
  onDragOver: Over
}
/** Split view memo: only its member tabs matter (and which one is active), not the rest of the table. */
function sameSplit(a: SplitProps, b: SplitProps): boolean {
  if (a.item !== b.item || a.fresh !== b.fresh || a.mark !== b.mark || a.onDragStart !== b.onDragStart || a.onDragOver !== b.onDragOver) return false
  if (a.tabs === b.tabs && a.activeId === b.activeId) return true
  return b.item.tabIds.every((id) => a.tabs[id] === b.tabs[id] && (a.activeId === id) === (b.activeId === id))
}

/** Split view: a pill that groups its tabs' favicons and the layout icon. */
const SplitChip = memo(function SplitChip({ item, tabs, activeId, fresh, mark, onDragStart, onDragOver }: SplitProps) {
  const { t } = useTranslation()
  const members = item.tabIds.map((id) => tabs[id]).filter((m): m is TabInfo => !!m)
  return (
    <div
      data-flip-id={item.id}
      className={`tsplit${ind(mark)}${fresh ? ' fresh' : ''}`}
      draggable
      onDragStart={(e) => onDragStart(e, { kind: 'split', id: item.id })}
      onDragOver={onDragOver}
    >
      {members.map((tab) => (
        <SplitSeg key={tab.id} tab={tab} active={tab.id === activeId} />
      ))}
      <span className="tsplit-layout" title={t('tabs.splitLayout')}>
        {item.layout === 'rows' ? <Rows2 size={12} /> : item.layout === 'grid' ? <Grid2x2 size={12} /> : <Columns2 size={12} />}
      </span>
    </div>
  )
}, sameSplit)

/** Pinned folder: tinted icon; a click opens the list of its tabs above the pages. */
const FolderChip = memo(function FolderChip({
  folder,
  inside,
  into,
  mark,
  onDragStart,
  onDragOver
}: {
  folder: FolderItem
  inside: boolean
  into: boolean
  mark: Indicator
  onDragStart: StartDrag
  onDragOver: Over
}) {
  const signet = window.signet
  return (
    <div
      data-folder={folder.id}
      data-flip-id={folder.id}
      className={`tchip sq folder${inside ? ' active' : ''}${folder.hue !== undefined ? ' tinted' : ''}${into ? ' drop-into' : ''}${ind(mark)}`}
      style={folder.hue !== undefined ? { ['--fh' as string]: folder.hue } : undefined}
      title={folder.name}
      draggable
      onDragStart={(e) => onDragStart(e, { kind: 'folder', id: folder.id })}
      onDragOver={onDragOver}
      onClick={(e) => signet.openPopover({ kind: 'folder', folderId: folder.id }, rectOf(e.currentTarget))}
      onContextMenu={(e) => (e.preventDefault(), signet.showFolderMenu(folder.id))}
    >
      <FolderIcon folder={folder} />
    </div>
  )
})

/** Today tab: favicon + title, close button on hover. */
const DayChip = memo(function DayChip({
  tab,
  active,
  fresh,
  mark,
  draggable,
  onDragStart,
  onDragOver
}: {
  tab: TabInfo
  active: boolean
  fresh: boolean
  mark: Indicator
  draggable: boolean
  onDragStart: StartDrag
  onDragOver: Over
}) {
  const { t } = useTranslation()
  const handlers = useMemo(() => tabHandlers(tab, () => activateTab(tab.id)), [tab])
  return (
    <div
      data-tab={tab.id}
      data-flip-id={tab.id}
      className={`tchip day${active ? ' active' : ''}${tab.loaded ? '' : ' unloaded'}${fresh ? ' fresh' : ''}${ind(mark)}`}
      title={displayTitle(tab, t('sidebar.newTab'))}
      draggable={draggable}
      onDragStart={(e) => onDragStart(e, { kind: 'tab', tabId: tab.id })}
      onDragOver={onDragOver}
      {...handlers}
    >
      <TabLine tab={tab} zone="today" />
    </div>
  )
})

/**
 * Row of tabs at the top of the window: another presentation of the same Snapshot as the sidebar,
 * with the same calls to main (activation, move, menus, rename). Dropdown menus and the
 * space form go through `openPopover`: they live in the overlay, above the web views.
 */
export function TabStrip({ s, floating = false }: { s: Snapshot; floating?: boolean }) {
  const { t } = useTranslation()
  const signet = window.signet
  const opt = useOptimistic()
  const space = s.spaces.find((x) => x.id === (opt.spaceId ?? s.activeSpaceId)) ?? s.spaces.find((x) => x.id === s.activeSpaceId) ?? s.spaces[0]
  const activeId = opt.tabId ?? space.activeTabId

  const [drop, setDrop] = useState<Drop | null>(null)
  const [dragging, setDragging] = useState(false)
  const [overSpace, setOverSpace] = useState(false)
  const [editingTab, setEditingTab] = useState<string | null>(null)
  const [fade, setFade] = useState({ l: 0, r: 0 })
  const editCtx = useMemo(() => ({ editing: editingTab, setEditing: setEditingTab }), [editingTab])
  const stripRef = useRef<HTMLDivElement>(null)
  const daysRef = useRef<HTMLDivElement>(null)
  const favRef = useRef<HTMLDivElement>(null)
  const pinnedRef = useRef<HTMLDivElement>(null)
  // Pills slide to their new place when the order changes (FLIP, see `flip.ts`).
  const flipKey = (it: Item) => (it.kind === 'tab' ? it.tabId : it.id)
  useFlip(favRef, s.favorites.join(','), space.id)
  useFlip(pinnedRef, space.pinned.map(flipKey).join(','), space.id)
  useFlip(daysRef, space.today.map((it) => `${flipKey(it)}${it.kind === 'tab' ? (it.group ?? '') : ''}`).join(','), space.id)
  const spaceRef = useRef<HTMLButtonElement>(null)
  const dragRef = useRef<DragSource | null>(null)
  const dropRef = useRef<Drop | null>(null)
  const forbiddenRef = useRef<Set<string>>(new Set())
  const dwellRef = useRef<number | undefined>(undefined)
  const seenTabs = useRef<{ space: string; ids: Set<string> } | null>(null)
  const freshRef = useRef<Map<string, number>>(new Map())

  /* ---------- requests from main (rename, choose an icon) ---------- */

  const spaceAnchor = (): Rect => (spaceRef.current ? rectOf(spaceRef.current) : { x: 78, y: 6, width: 140, height: 28 })
  /** Rectangle of a tab's or folder's pill, once the row is updated. */
  const anchorOf = (selector: string): Rect => {
    const el = stripRef.current?.querySelector(selector)
    return el ? rectOf(el) : spaceAnchor()
  }
  useEffect(
    () =>
      signet.onRenameFolder((req) => {
        // Same channel for tab groups: they are recognized by their label in the row.
        setTimeout(() => {
          const group = stripRef.current?.querySelector(`[data-group="${req.folderId}"]`)
          if (group) void signet.openPopover({ kind: 'rename', target: { kind: 'group', groupId: req.folderId } }, rectOf(group))
          else void signet.openPopover({ kind: 'rename', target: { kind: 'folder', folderId: req.folderId } }, anchorOf(`[data-folder="${req.folderId}"]`))
        }, 60)
      }),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [signet]
  )
  useEffect(
    () =>
      signet.onRenameTab((req) => {
        // A today tab is renamed in place; pills without a title go through a field below them.
        if (daysRef.current?.querySelector(`[data-tab="${req.tabId}"]`)) setEditingTab(req.tabId)
        else void signet.openPopover({ kind: 'rename', target: { kind: 'tab', tabId: req.tabId } }, anchorOf(`[data-tab="${req.tabId}"]`))
      }),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [signet]
  )
  useEffect(
    () =>
      signet.onPickIcon((target) => {
        const selector =
          target.kind === 'tab'
            ? `[data-tab="${target.tabId}"]`
            : target.kind === 'folder'
              ? `[data-folder="${target.folderId}"]`
              : `[data-group="${target.groupId}"]`
        void signet.openPopover({ kind: 'icon', target }, anchorOf(selector))
      }),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [signet]
  )

  /* ---------- downloads ---------- */

  const progress = progressOf(s.downloads)
  const unseen = useUnseenDownloads(s.downloads, false)

  /* ---------- scrolling of today tabs ---------- */

  const updateFade = () => {
    const el = daysRef.current
    if (!el) return
    const l = el.scrollLeft > 1 ? FADE : 0
    const r = el.scrollLeft + el.clientWidth < el.scrollWidth - 1 ? FADE : 0
    setFade((f) => (f.l === l && f.r === r ? f : { l, r }))
  }
  // Content width: ids, titles and sound pill of today tabs (the container itself is observed further down).
  const daysKey = useMemo(
    () =>
      space.today
        .flatMap(itemTabIds)
        .map((id) => {
          const tb = s.tabs[id]
          return tb ? `${id}:${displayTitle(tb, '')}:${tb.audible ? 1 : 0}${tb.muted ? 1 : 0}` : id
        })
        .join('|'),
    [space.today, s.tabs]
  )
  useLayoutEffect(updateFade, [daysKey])
  useEffect(() => {
    const el = daysRef.current
    if (!el) return
    const ro = new ResizeObserver(updateFade)
    ro.observe(el)
    return () => ro.disconnect()
  }, [])
  // The active tab stays visible.
  useEffect(() => {
    if (!activeId) return
    const el = daysRef.current?.querySelector<HTMLElement>(`[data-tab="${activeId}"]`)
    el?.scrollIntoView({ inline: 'nearest', block: 'nearest', behavior: reducedMotion.matches ? 'auto' : 'smooth' })
  }, [activeId, space.today.length])

  /** Two-finger swipe on the row: horizontal scroll of today tabs. */
  const onWheel = (e: WheelEvent) => {
    const el = daysRef.current
    if (!el) return
    const inside = el.contains(e.target as Node)
    const dominant = Math.abs(e.deltaX) >= Math.abs(e.deltaY) ? e.deltaX : e.deltaY
    // The horizontal gesture in the scrolling area is already handled by the browser.
    if (inside && Math.abs(e.deltaX) >= Math.abs(e.deltaY)) return
    el.scrollLeft += dominant
  }

  /* ---------- arrived tabs (their pill opens in width) ---------- */

  const tabIds = useMemo(() => [...space.pinned, ...space.today].flatMap(itemTabIds), [space.pinned, space.today])
  const fresh = useMemo(() => {
    const seen = seenTabs.current
    const now = Date.now()
    if (!seen || seen.space !== space.id) freshRef.current.clear()
    else for (const id of tabIds) if (!seen.ids.has(id)) freshRef.current.set(id, now)
    for (const [id, at] of freshRef.current) if (now - at > 400) freshRef.current.delete(id)
    return new Set(freshRef.current.keys())
  }, [tabIds, space.id])
  useEffect(() => {
    seenTabs.current = { space: space.id, ids: new Set(tabIds) }
  }, [tabIds, space.id])

  /* ---------- drag and drop (same calls to main as the sidebar) ---------- */

  useEffect(() => {
    const end = () => {
      if (dragRef.current?.kind === 'tab') window.signet.endTabDrag().catch(() => {})
      dragRef.current = null
      forbiddenRef.current = new Set()
      dropRef.current = null
      clearTimeout(dwellRef.current)
      dwellRef.current = undefined
      setDrop(null)
      setDragging(false)
      setOverSpace(false)
    }
    window.addEventListener('dragend', end)
    window.addEventListener('drop', end)
    return () => {
      window.removeEventListener('dragend', end)
      window.removeEventListener('drop', end)
    }
  }, [])

  const putDrop = (target: Drop | null) => {
    if (target?.folderId && forbiddenRef.current.has(target.folderId)) target = null
    const p = dropRef.current
    if (
      p?.zone === target?.zone &&
      p?.index === target?.index &&
      p?.spaceId === target?.spaceId &&
      p?.folderId === target?.folderId &&
      p?.into === target?.into &&
      p?.groupId === target?.groupId
    )
      return
    dropRef.current = target
    setDrop(target)
  }
  const accepts = (zone: Zone) => {
    const kind = dragRef.current?.kind
    if (!kind) return false
    // A group dropped into pinned becomes a folder; a folder dropped into today tabs, a group.
    return kind === 'tab' || zone !== 'favorites'
  }

  const startDrag = (e: DragEvent, src: DragSource) => {
    e.stopPropagation()
    e.dataTransfer.setData(DRAG_MIME, JSON.stringify(src))
    e.dataTransfer.effectAllowed = 'move'
    dragRef.current = src
    const dragged = src.kind === 'folder' ? findFolder(space.pinned, src.id) : null
    forbiddenRef.current = new Set(dragged ? folderIds(dragged) : [])
    // Modifying the DOM (or native views) during dragstart would cancel the drag.
    setTimeout(() => setDragging(true), 0)
    if (src.kind === 'tab') {
      if (s.favorites.includes(src.tabId)) e.dataTransfer.setData(DRAG_FAVORITE_MIME, '1')
      setTimeout(() => dragRef.current === src && window.signet.beginTabDrag(src.tabId).catch(() => {}), 0)
    }
  }

  /**
   * Hover over a pill: insert before or after depending on the left or right half.
   * In a group (`run`), the dropped item joins it; a dragged group lands before or after it.
   */
  const overItem = (zone: Zone, index: number, run?: Run) => (e: DragEvent) => {
    if (!accepts(zone)) return
    e.preventDefault()
    e.stopPropagation()
    e.dataTransfer.dropEffect = 'move'
    const card = run?.group ? e.currentTarget.closest('.tsgroup') : null
    if (run && card && isBlock(dragRef.current)) {
      putDrop({ zone, spaceId: space.id, index: aroundRun(card, run, e.clientX, true, dropRef.current?.index ?? null) })
      return
    }
    const r = e.currentTarget.getBoundingClientRect()
    putDrop({ zone, spaceId: space.id, index: index + (e.clientX > r.left + r.width / 2 ? 1 : 0), groupId: run?.group })
  }
  /** Hover over a group's label: left half = before the group, right half = inside, at the head. */
  const overGroupHead = (run: Run) => (e: DragEvent) => {
    if (!accepts('today')) return
    e.preventDefault()
    e.stopPropagation()
    e.dataTransfer.dropEffect = 'move'
    const card = e.currentTarget.closest('.tsgroup')
    if (isBlock(dragRef.current)) {
      if (card) putDrop({ zone: 'today', spaceId: space.id, index: aroundRun(card, run, e.clientX, true, dropRef.current?.index ?? null) })
      return
    }
    const r = e.currentTarget.getBoundingClientRect()
    if (e.clientX < r.left + r.width / 2) putDrop({ zone: 'today', spaceId: space.id, index: run.start })
    else putDrop({ zone: 'today', spaceId: space.id, index: run.start, groupId: run.group, into: run.group })
  }
  /** Hover over a group's margins: at its end (or before/after it for a dragged group). */
  const overGroup = (run: Run) => (e: DragEvent) => {
    if (!accepts('today')) return
    e.preventDefault()
    e.stopPropagation()
    e.dataTransfer.dropEffect = 'move'
    if (isBlock(dragRef.current))
      putDrop({ zone: 'today', spaceId: space.id, index: aroundRun(e.currentTarget, run, e.clientX, true, dropRef.current?.index ?? null) })
    else putDrop({ zone: 'today', spaceId: space.id, index: run.end, groupId: run.group })
  }

  /** Hover over a folder: left quarter = before, right quarter = after, middle = inside (at the end). */
  const overFolder = (folder: FolderItem, index: number) => (e: DragEvent) => {
    if (!accepts('pinned')) return
    e.preventDefault()
    e.stopPropagation()
    e.dataTransfer.dropEffect = 'move'
    const r = e.currentTarget.getBoundingClientRect()
    const x = (e.clientX - r.left) / r.width
    if (x > 0.25 && x < 0.75 && !forbiddenRef.current.has(folder.id)) {
      putDrop({ zone: 'pinned', spaceId: space.id, folderId: folder.id, index: folder.items.length, into: folder.id })
    } else putDrop({ zone: 'pinned', spaceId: space.id, index: index + (x >= 0.5 ? 1 : 0) })
  }

  /** Hover over a group's empty area: append at the end. */
  const overList = (zone: Zone, length: number) => (e: DragEvent) => {
    if (!accepts(zone)) return
    e.preventDefault()
    e.dataTransfer.dropEffect = 'move'
    putDrop({ zone, spaceId: space.id, index: zone === 'today' ? length : indexAt(e, length) })
  }
  /** Insertion rank from the pointer in a list's gaps: first pill whose middle is after it, otherwise the end. */
  const indexAt = (e: DragEvent, length: number): number => {
    const chips = Array.from((e.currentTarget as HTMLElement).children).filter((c) => c.hasAttribute('data-flip-id'))
    if (chips.length !== length) return length
    for (let i = 0; i < chips.length; i++) {
      const r = chips[i].getBoundingClientRect()
      if (e.clientX < r.left + r.width / 2) return i
    }
    return length
  }
  const dropOnList = (zone: Zone, length: number) => (e: DragEvent) => {
    e.preventDefault()
    const raw = e.dataTransfer.getData(DRAG_MIME)
    if (!raw) return
    const src = JSON.parse(raw) as DragSource
    const d = dropRef.current
    void signet.moveItem(
      src,
      d && d.zone === zone
        ? { zone: d.zone, spaceId: d.spaceId, folderId: d.folderId, groupId: d.groupId, index: d.index }
        : { zone, spaceId: space.id, index: length }
    )
  }
  const listProps = (zone: Zone, length: number) => ({
    onDragOver: overList(zone, length),
    onDrop: dropOnList(zone, length),
    onDragLeave: (e: DragEvent) => {
      if (!e.currentTarget.contains(e.relatedTarget as Node | null)) putDrop(null)
    }
  })

  const indicator = (zone: Zone, i: number, len: number): Indicator => {
    if (!drop || drop.zone !== zone || drop.folderId || drop.into) return null
    if (drop.index === i) return 'before'
    return i === len - 1 && drop.index === len ? 'after' : null
  }

  /** A dragged tab lingering on the space switcher opens the spaces menu (the tab is dropped there). */
  const spaceDragOver = (e: DragEvent) => {
    if (!dragRef.current) return
    e.preventDefault()
    setOverSpace(true)
    if (dwellRef.current === undefined) {
      dwellRef.current = window.setTimeout(() => {
        dwellRef.current = undefined
        void signet.openPopover({ kind: 'spaces' }, spaceAnchor())
      }, DWELL)
    }
  }
  const spaceDragLeave = () => {
    clearTimeout(dwellRef.current)
    dwellRef.current = undefined
    setOverSpace(false)
  }

  /* ---------- pills ---------- */

  // Pills are memoized: their handlers keep the same identity from one render to the next
  // and go through `latest` to read the current space (same pattern as the sidebar).
  const latest = useRef({ startDrag, overItem, overFolder })
  latest.current = { startDrag, overItem, overFolder }
  const onDragStart = useCallback<StartDrag>((e, src) => latest.current.startDrag(e, src), [])
  const overCache = useRef(new Map<string, Over>())
  // Keys depend on ranks: start over when the lists change.
  const overCacheDeps = useRef<unknown[]>([])
  const deps = [space.pinned, space.today, s.favorites]
  if (deps.some((d, i) => d !== overCacheDeps.current[i])) {
    overCacheDeps.current = deps
    overCache.current.clear()
  }
  const cached = (key: string, make: () => Over) => {
    let f = overCache.current.get(key)
    if (!f) overCache.current.set(key, (f = make()))
    return f
  }
  const itemOver = (zone: Zone, i: number, run?: Run) =>
    cached(`i|${zone}|${i}|${run ? `${run.group}:${run.start}:${run.end}` : ''}`, () => (e) => latest.current.overItem(zone, i, run)(e))
  const folderOver = (folder: FolderItem, i: number) =>
    cached(`f|${folder.id}|${i}|${folder.items.length}`, () => (e) => latest.current.overFolder(folder, i)(e))

  const iconChip = (tab: TabInfo, zone: Zone, i: number, len: number) => (
    <IconChip
      key={tab.id}
      tab={tab}
      active={tab.id === activeId}
      fresh={fresh.has(tab.id)}
      mark={indicator(zone, i, len)}
      draggable={editingTab !== tab.id}
      onDragStart={onDragStart}
      onDragOver={itemOver(zone, i)}
    />
  )

  const splitChip = (item: Extract<Item, { kind: 'split' }>, zone: Zone, i: number, mark: Indicator, run?: Run) => (
    <SplitChip
      key={item.id}
      item={item}
      tabs={s.tabs}
      activeId={activeId}
      fresh={fresh.has(item.tabIds[0])}
      mark={mark}
      onDragStart={onDragStart}
      onDragOver={itemOver(zone, i, run)}
    />
  )

  const folderChip = (folder: FolderItem, i: number, len: number) => (
    <FolderChip
      key={folder.id}
      folder={folder}
      inside={activeId ? itemTabIds(folder).includes(activeId) : false}
      into={drop?.into === folder.id}
      mark={indicator('pinned', i, len)}
      onDragStart={onDragStart}
      onDragOver={folderOver(folder, i)}
    />
  )

  const dayChip = (tab: TabInfo, i: number, mark: Indicator, run?: Run) => (
    <DayChip
      key={tab.id}
      tab={tab}
      active={tab.id === activeId}
      fresh={fresh.has(tab.id)}
      mark={mark}
      draggable={editingTab !== tab.id}
      onDragStart={onDragStart}
      onDragOver={itemOver('today', i, run)}
    />
  )

  /** Group label: its title in color (click: collapse / expand, right click: group menu). */
  const groupLabel = (run: Run, header: TabGroup, before = false) => {
    const id = run.group!
    const first = s.tabs[itemTabIds(space.today[run.start])[0]]
    const name = header.name || (first ? displayTitle(first, t('sidebar.newTab')) : '')
    return (
      <div
        data-group={id}
        className={`tglabel${drop?.into === id ? ' drop-into' : ''}`}
        title={name}
        draggable
        onDragStart={(e) => startDrag(e, { kind: 'group', id })}
        onDragOver={overGroupHead(run)}
        onClick={() => signet.toggleGroup(id)}
        onContextMenu={(e) => (e.preventDefault(), signet.showGroupMenu(id))}
      >
        {header.icon && <EmojiIcon emoji={header.icon} size={14} />}
        <span className="tglabel-name">{name}</span>
        {before && (
          <span
            aria-hidden
            style={{ position: 'absolute', top: 3, bottom: 3, left: -4, width: 2, borderRadius: 1, background: 'var(--accent)', pointerEvents: 'none' }}
          />
        )}
      </div>
    )
  }

  const favorites = s.favorites.map((id) => s.tabs[id]).filter((x): x is TabInfo => !!x)
  const showFavorites = favorites.length > 0 || dragging
  const showPinned = space.pinned.length > 0 || dragging

  return (
    <TabEditContext.Provider value={editCtx}>
      <div ref={stripRef} className={`tabstrip${floating ? ' floating' : ''}`} onWheel={onWheel}>
        <button
          ref={spaceRef}
          className={`ts-space${overSpace ? ' over' : ''}`}
          title={t('tabstrip.spaces')}
          aria-haspopup="menu"
          onClick={(e) => signet.openPopover({ kind: 'spaces' }, rectOf(e.currentTarget))}
          onContextMenu={(e) => (e.preventDefault(), signet.showSpaceMenu(space.id))}
          onDragEnter={spaceDragOver}
          onDragOver={spaceDragOver}
          onDragLeave={spaceDragLeave}
          onDrop={(e) => {
            // Dropping on the switcher itself sends nowhere: the send happens on a menu row.
            e.preventDefault()
            spaceDragLeave()
          }}
        >
          <span className="emoji">{space.icon}</span>
          <span className="name">{space.name}</span>
          {space.profile === 'isolated' && <UserRound size={12} className="ts-iso" aria-label={t('sidebar.isolatedProfile')} />}
          <ChevronDown size={13} className="ts-caret" />
        </button>

        {showFavorites && (
          <div ref={favRef} className={`ts-group ts-fav${favorites.length === 0 ? ' empty-zone' : ''}`} {...listProps('favorites', favorites.length)}>
            {favorites.map((tab, i) => iconChip(tab, 'favorites', i, favorites.length))}
            {favorites.length === 0 && <span className="ts-hint">{t('sidebar.dropFavorites')}</span>}
          </div>
        )}
        {showPinned && (
          <div ref={pinnedRef} className={`ts-group ts-pinned${space.pinned.length === 0 ? ' empty-zone' : ''}`} {...listProps('pinned', space.pinned.length)}>
            {space.pinned.map((item, i) => {
              if (item.kind === 'folder') return folderChip(item, i, space.pinned.length)
              if (item.kind === 'split') return splitChip(item, 'pinned', i, indicator('pinned', i, space.pinned.length))
              const tab = s.tabs[item.tabId]
              return tab ? iconChip(tab, 'pinned', i, space.pinned.length) : null
            })}
            {space.pinned.length === 0 && <span className="ts-hint">{t('sidebar.dropPin')}</span>}
          </div>
        )}
        {(showFavorites || showPinned) && <span className="ts-sep" />}

        <div
          ref={daysRef}
          className="ts-group ts-days"
          data-fade={fade.l || fade.r ? '' : undefined}
          style={{ ['--fl' as string]: `${fade.l}px`, ['--fr' as string]: `${fade.r}px` }}
          onScroll={updateFade}
          {...listProps('today', space.today.length)}
        >
          {(() => {
            const mark = todayMark(space.today, drop)
            const chip = (i: number, run?: Run, beforeCard = false) => {
              const item = space.today[i]
              const m = mark?.index === i && !beforeCard && !mark.card ? mark.side : null
              if (item.kind === 'split') return splitChip(item, 'today', i, m, run)
              if (item.kind !== 'tab') return null
              const tab = s.tabs[item.tabId]
              return tab ? dayChip(tab, i, m, run) : null
            }
            return runsOf(space.today).map((run) => {
              if (!run.group) return chip(run.start)
              const header = space.groups?.[run.group] ?? { name: '', hue: 215, open: true }
              const chips: React.ReactNode[] = []
              // Insertion right before the group: the marker goes on the label, not on the first tab.
              const beforeCard = mark?.side === 'before' && mark.index === run.start && drop?.groupId !== run.group
              for (let i = run.start; i < run.end; i++) {
                // Collapsed: only the active tab's pill remains, if it is in the group.
                if (header.open || (activeId && itemTabIds(space.today[i]).includes(activeId))) chips.push(chip(i, run, beforeCard))
              }
              return (
                <Fragment key={`g:${run.group}`}>
                  <div className={`tsgroup${header.open ? '' : ' closed'}`} style={{ ['--fh' as string]: header.hue }} onDragOver={overGroup(run)}>
                    {groupLabel(run, header, beforeCard)}
                    {chips}
                  </div>
                  {mark?.card && mark.index === run.end - 1 && (
                    <span aria-hidden style={{ position: 'relative', flex: 'none', width: 0, alignSelf: 'stretch' }}>
                      <span style={{ position: 'absolute', top: 3, bottom: 3, left: 1, width: 2, borderRadius: 1, background: 'var(--accent)' }} />
                    </span>
                  )}
                </Fragment>
              )
            })
          })()}
        </div>
        <button className="icon ts-plus" title={t('sidebar.newTab')} onClick={() => signet.openPalette('new-tab')}>
          <Plus size={16} />
        </button>

        <span className="ts-spacer" />

        <MediaChip media={s.media} tabs={s.tabs} />
        {s.downloads.length > 0 && (
          <button className="icon dl-btn" title={t('sidebar.downloads')} onClick={(e) => signet.openPopover({ kind: 'downloads' }, rectOf(e.currentTarget))}>
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
          className="icon"
          title={(() => {
            const k = shortcutLabel('library', s.settings.shortcuts, IS_MAC)
            return k ? t('sidebar.library', { keys: k }) : t('sidebar.libraryNoKeys')
          })()}
          onClick={() => signet.openLibrary()}
        >
          <Library size={16} />
        </button>
        <button className="icon" title={t('sidebar.settings')} onClick={() => signet.openSettings()}>
          <Settings size={16} />
        </button>
      </div>
    </TabEditContext.Provider>
  )
}

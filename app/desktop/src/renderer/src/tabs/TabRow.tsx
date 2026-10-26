import type { DragEvent, KeyboardEvent, MouseEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { memo, useContext, useEffect, useRef, useState } from 'react'
import { ChevronRight, Columns2, Grid2x2, Rows2, Volume2, VolumeX, X } from 'lucide-react'
import type { DragSource, Item, TabGroup, TabInfo, Zone } from '@shared/types'
import { activateTab } from '../lib/optimistic'
import { reducedMotion } from '../lib/spring'
import { previewHandlers } from '../lib/previewHover'
import { FolderIcon, GroupIcon, TabEditContext, TabIcon, TabTitle, displayTitle } from './TabCustom'

export type Indicator = 'before' | 'after' | null

const norm = (u: string) => u.replace(/\/$/, '')

/**
 * Closes the tab right away (the page and its sound stop, the next tab activates without waiting) and fades
 * a ghost of its row out in its place (fade + slight zoom). Neighbors then slide over (FLIP, see `flip.ts`).
 * A pinned tab stays in the list (it is only unloaded): no ghost. Reduced motion: no ghost.
 */
export function closeAnimated(tab: TabInfo, from: Element) {
  const row = from.closest<HTMLElement>('.row, .tchip')
  const host = row?.parentElement
  const anchor = row?.offsetParent
  if (!row || !host || !anchor || tab.pinnedUrl || reducedMotion.matches) return window.signet.closeTab(tab.id)
  const r = row.getBoundingClientRect()
  const a = anchor.getBoundingClientRect()
  const ghost = row.cloneNode(true) as HTMLElement
  ghost.classList.add('ghost')
  ghost.removeAttribute('data-flip-id')
  for (const n of ghost.querySelectorAll('[data-flip-id], [id]')) {
    n.removeAttribute('data-flip-id')
    n.removeAttribute('id')
  }
  Object.assign(ghost.style, {
    position: 'absolute',
    left: `${r.left - a.left + anchor.scrollLeft - anchor.clientLeft}px`,
    top: `${r.top - a.top + anchor.scrollTop - anchor.clientTop}px`,
    width: `${r.width}px`,
    height: `${r.height}px`,
    margin: '0',
    pointerEvents: 'none',
    zIndex: '1'
  })
  ghost.setAttribute('aria-hidden', 'true')
  ghost.tabIndex = -1
  host.appendChild(ghost)
  // The real row disappears under the ghost; the render removes it once the tab is closed.
  row.style.visibility = 'hidden'
  const done = () => {
    ghost.remove()
    if (row.isConnected) row.style.visibility = ''
  }
  const dur = row.classList.contains('row') ? 130 : 190
  ghost
    .animate(
      [
        { opacity: 1, scale: 1 },
        { opacity: 0, scale: 0.96 }
      ],
      { duration: dur, easing: 'cubic-bezier(0.2, 0.8, 0.2, 1)', fill: 'forwards' }
    )
    .finished.then(done, done)
  void window.signet.closeTab(tab.id)
}

/** Content of a tab row: favicon, title, sound, close, back to the pinned URL. */
export const TabLine = memo(function TabLine({ tab, zone }: { tab: TabInfo; zone: Zone }) {
  const { t } = useTranslation()
  const signet = window.signet
  const stop = (e: MouseEvent) => e.stopPropagation()
  const drifted = zone === 'pinned' && !!tab.pinnedUrl && norm(tab.url) !== norm(tab.pinnedUrl)
  return (
    <>
      <TabIcon tab={tab} />
      {/* Sound to the left of the title: the cross that appears on the right on hover does not move it. */}
      {(tab.audible || tab.muted) && (
        <button
          className="mini sound"
          title={tab.muted ? t('media.unmute') : t('media.mute')}
          onClick={(e) => {
            stop(e)
            void signet.toggleMute(tab.id)
          }}
        >
          {tab.muted ? <VolumeX size={14} /> : <Volume2 size={14} />}
        </button>
      )}
      <TabTitle tab={tab} />
      {/* Item of a live folder not yet opened. */}
      {tab.liveUnread && <span className="live-dot" title={t('tab.liveUnread')} aria-label={t('tab.liveUnread')} />}
      {drifted && (
        <button
          className="mini reset"
          title={t('tabs.resetPinned')}
          onClick={(e) => {
            stop(e)
            void signet.resetPinned(tab.id)
          }}
        >
          /
        </button>
      )}
      <button
        className="mini close"
        title={tab.pinnedUrl ? t('tabs.unload') : t('common.close')}
        onClick={(e) => {
          stop(e)
          void closeAnimated(tab, e.currentTarget)
        }}
      >
        <X size={14} />
      </button>
    </>
  )
})

/** Moves keyboard focus to the previous / next visible [data-nav] row. */
export function moveNav(from: Element, dir: 1 | -1): void {
  const all = Array.from(document.querySelectorAll<HTMLElement>('[data-nav]')).filter((el) => el.getClientRects().length > 0)
  all[all.indexOf(from as HTMLElement) + dir]?.focus()
}

/** Gives focus back to a tab's row once the list is redrawn (React may have remounted the row). */
function refocusRow(tabId: string): void {
  let tries = 0
  const tick = () => {
    const el = document.querySelector<HTMLElement>(`[data-nav][data-flip-id="${CSS.escape(tabId)}"]`)
    if (el && el !== document.activeElement) el.focus({ preventScroll: false })
    if (++tries < 4) requestAnimationFrame(tick)
  }
  requestAnimationFrame(tick)
}

/** Keys shared by rows (focus only on the row itself): arrows, Enter / Space. Returns true if handled. */
export function navKeys(e: KeyboardEvent, onActivate: () => void): boolean {
  if (e.target !== e.currentTarget || e.altKey || e.ctrlKey || e.metaKey) return false
  if (e.key === 'ArrowDown' || e.key === 'ArrowRight') moveNav(e.currentTarget, 1)
  else if (e.key === 'ArrowUp' || e.key === 'ArrowLeft') moveNav(e.currentTarget, -1)
  else if (e.key === 'Enter' || e.key === ' ') onActivate()
  else return false
  e.preventDefault()
  return true
}

/** Gestures shared by rows: activation on press, middle click, right click, keyboard (roving tabIndex: only the active row is a Tab stop). */
export function tabHandlers(tab: TabInfo, onActivate: () => void, active = true) {
  const signet = window.signet
  return {
    role: 'button' as const,
    tabIndex: active ? 0 : -1,
    'data-nav': '',
    'aria-current': active ? ('true' as const) : undefined,
    onKeyDown: (e: KeyboardEvent) => {
      if (e.key === 'Delete' && e.target === e.currentTarget) void closeAnimated(tab, e.currentTarget as Element)
      else if (e.target === e.currentTarget && e.altKey && e.shiftKey && !e.ctrlKey && !e.metaKey && (e.key === 'ArrowUp' || e.key === 'ArrowDown')) {
        // ⌥⇧↑/↓: moves the row by one rank; focus follows it after the render.
        e.preventDefault()
        void signet.moveTabStep(tab.id, e.key === 'ArrowUp' ? -1 : 1)
        refocusRow(tab.id)
      } else navKeys(e, onActivate)
    },
    // Tab preview on hover (see previewHover.ts).
    ...previewHandlers(tab.id),
    onMouseDown: (e: MouseEvent) => {
      if (e.button === 1) return e.preventDefault()
      // No preventDefault: native drag and selection stay possible.
      if (e.button !== 0 || e.ctrlKey) return
      if ((e.target as Element).closest('button, input, .mini')) return
      onActivate()
    },
    onAuxClick: (e: MouseEvent) => {
      if (e.button === 1) void closeAnimated(tab, e.currentTarget as Element)
    },
    onContextMenu: (e: MouseEvent) => {
      e.preventDefault()
      void signet.showTabMenu(tab.id)
    }
  }
}

interface Props {
  item: Item
  tabs: Record<string, TabInfo>
  activeTabId: string | null
  zone: Zone
  indicator: Indicator
  /** Row that arrived since the last render: it expands. */
  fresh: boolean
  /** Item held during a drag: its slot collapses. */
  dragSrc: boolean
  onDragStart: (e: DragEvent, src: DragSource) => void
  onDragOver: (e: DragEvent) => void
}

/** Row memo: only its own tabs matter (and whether they are active), not the rest of the table. */
function sameRow(a: Props, b: Props): boolean {
  if (
    a.item !== b.item ||
    a.zone !== b.zone ||
    a.indicator !== b.indicator ||
    a.fresh !== b.fresh ||
    a.dragSrc !== b.dragSrc ||
    a.onDragStart !== b.onDragStart ||
    a.onDragOver !== b.onDragOver
  )
    return false
  if (a.tabs === b.tabs && a.activeTabId === b.activeTabId) return true
  return itemIds(b.item).every((id) => a.tabs[id] === b.tabs[id] && (a.activeTabId === id) === (b.activeTabId === id))
}
const itemIds = (i: Item): string[] => (i.kind === 'tab' ? [i.tabId] : i.kind === 'split' ? i.tabIds : [])

/** A sidebar row: a single tab or a split view. */
export const ItemRow = memo(function ItemRow({ item, tabs, activeTabId, zone, indicator, fresh, dragSrc, onDragStart, onDragOver }: Props) {
  const { t } = useTranslation()
  const signet = window.signet
  const { editing } = useContext(TabEditContext)
  const ind = indicator ? ` drop-${indicator}` : ''

  if (item.kind === 'folder') return null

  if (item.kind === 'tab') {
    const tab = tabs[item.tabId]
    if (!tab) return null
    return (
      <div
        className={`row${tab.id === activeTabId ? ' active' : ''}${tab.loaded ? '' : ' unloaded'}${fresh ? ' fresh' : ''}${dragSrc ? ' drag-src' : ''}${ind}`}
        data-flip-id={tab.id}
        draggable={editing !== tab.id}
        onDragStart={(e) => onDragStart(e, { kind: 'tab', tabId: tab.id })}
        onDragOver={onDragOver}
        {...tabHandlers(tab, () => activateTab(tab.id), tab.id === activeTabId)}
      >
        <TabLine tab={tab} zone={zone} />
      </div>
    )
  }

  const members = item.tabIds.map((id) => tabs[id]).filter((m): m is TabInfo => !!m)
  return (
    <div
      className={`split${dragSrc ? ' drag-src' : ''}${ind}`}
      data-flip-id={item.id}
      draggable
      onDragStart={(e) => onDragStart(e, { kind: 'split', id: item.id })}
      onDragOver={onDragOver}
    >
      {members.map((tab) => (
        <div
          key={tab.id}
          className={`row seg${tab.id === activeTabId ? ' active' : ''}${tab.loaded ? '' : ' unloaded'}`}
          {...tabHandlers(tab, () => void signet.focusPane(tab.id), tab.id === activeTabId)}
        >
          <TabLine tab={tab} zone="today" />
        </div>
      ))}
      <span className="split-layout" title={t('tabs.splitLayout')}>
        {item.layout === 'rows' ? <Rows2 size={12} /> : item.layout === 'grid' ? <Grid2x2 size={12} /> : <Columns2 size={12} />}
      </span>
    </div>
  )
}, sameRow)

type FolderItem = Extract<Item, { kind: 'folder' }>

/** Folder row: opens on click, renames in place (double-click or request from main). */
export const FolderRow = memo(function FolderRow({
  folder,
  indicator,
  into,
  editing,
  onRename,
  onDone,
  onDragStart,
  onDragOver
}: {
  folder: FolderItem
  indicator: Indicator
  into: boolean
  editing: boolean
  onRename: (id: string) => void
  onDone: () => void
  onDragStart: (e: DragEvent, src: DragSource) => void
  onDragOver: (e: DragEvent) => void
}) {
  const signet = window.signet
  const [value, setValue] = useState(folder.name)
  const cancelled = useRef(false)
  // Open/closed state before the first click of a double-click: renaming restores it.
  const openBefore = useRef(folder.open)
  // Value set when entering edit mode, before the field mounts: its selection (everything) is no longer overwritten.
  const [wasEditing, setWasEditing] = useState(editing)
  if (editing !== wasEditing) {
    setWasEditing(editing)
    if (editing) setValue(folder.name)
  }
  useEffect(() => {
    if (editing) cancelled.current = false
  }, [editing])

  const commit = () => {
    const name = value.trim()
    if (!cancelled.current && name && name !== folder.name) void signet.renameFolder(folder.id, name)
    onDone()
  }
  const ind = indicator ? ` drop-${indicator}` : ''
  return (
    <div
      className={`row folder-row${into ? ' drop-into' : ''}${ind}`}
      role="button"
      tabIndex={-1}
      data-nav=""
      aria-expanded={folder.open}
      draggable={!editing}
      onKeyDown={(e) => {
        if (editing || e.target !== e.currentTarget) return
        const toggle = () => void signet.toggleFolder(folder.id)
        if (e.key === 'ArrowRight' && !folder.open) toggle()
        else if (e.key === 'ArrowLeft' && folder.open) toggle()
        else if (e.key === 'F2') onRename(folder.id)
        else if (!navKeys(e, toggle)) return
        e.preventDefault()
      }}
      onDragStart={(e) => onDragStart(e, { kind: 'folder', id: folder.id })}
      onDragOver={onDragOver}
      onClick={(e) => {
        if (editing || e.detail >= 2) return
        openBefore.current = folder.open
        void signet.toggleFolder(folder.id)
      }}
      onContextMenu={(e) => {
        e.preventDefault()
        void signet.showFolderMenu(folder.id)
      }}
    >
      <FolderIcon folder={folder} />
      {editing ? (
        <input
          className="folder-input"
          autoFocus
          value={value}
          onFocus={(e) => e.currentTarget.select()}
          onChange={(e) => setValue(e.target.value)}
          onClick={(e) => e.stopPropagation()}
          onBlur={commit}
          onKeyDown={(e) => {
            if (e.nativeEvent.isComposing) return
            if (e.key === 'Enter') e.currentTarget.blur()
            else if (e.key === 'Escape') {
              cancelled.current = true
              e.currentTarget.blur()
            }
          }}
        />
      ) : (
        <span
          className="title"
          onDoubleClick={() => {
            // The first click toggled the folder: put it back as it was (order guaranteed on the main side).
            void signet.toggleFolder(folder.id, openBefore.current)
            onRename(folder.id)
          }}
        >
          {folder.name}
        </span>
      )}
      <ChevronRight size={14} className={`chevron${folder.open ? ' open' : ''}`} />
    </div>
  )
})

/**
 * Header of a today tab group: first tab's icon, title in the group's color, arrow.
 * Click: collapse / expand; double-click: rename; drag: the whole group; right click: menu (including "Close group").
 */
export const GroupRow = memo(function GroupRow({
  groupId,
  group,
  first,
  indicator,
  into,
  editing,
  onRename,
  onDone,
  onDragStart,
  onDragOver
}: {
  groupId: string
  group: TabGroup
  first: TabInfo | undefined
  indicator: Indicator
  into: boolean
  editing: boolean
  onRename: (id: string) => void
  onDone: () => void
  onDragStart: (e: DragEvent, src: DragSource) => void
  onDragOver: (e: DragEvent) => void
}) {
  const { t } = useTranslation()
  const signet = window.signet
  const name = group.name || (first ? displayTitle(first, t('sidebar.newTab')) : '')
  const [value, setValue] = useState(name)
  const cancelled = useRef(false)
  const openBefore = useRef(group.open)
  const [wasEditing, setWasEditing] = useState(editing)
  if (editing !== wasEditing) {
    setWasEditing(editing)
    if (editing) setValue(name)
  }
  useEffect(() => {
    if (editing) cancelled.current = false
  }, [editing])

  const commit = () => {
    const clean = value.trim()
    if (!cancelled.current && clean && clean !== name) void signet.renameGroup(groupId, clean)
    onDone()
  }
  return (
    <div
      className={`row folder-row group-row${into ? ' drop-into' : ''}${indicator ? ` drop-${indicator}` : ''}`}
      role="button"
      tabIndex={-1}
      data-nav=""
      aria-expanded={group.open}
      onKeyDown={(e) => {
        if (editing || e.target !== e.currentTarget) return
        const toggle = () => void signet.toggleGroup(groupId)
        if (e.key === 'ArrowRight' && !group.open) toggle()
        else if (e.key === 'ArrowLeft' && group.open) toggle()
        else if (e.key === 'F2') onRename(groupId)
        else if (!navKeys(e, toggle)) return
        e.preventDefault()
      }}
      draggable={!editing}
      onDragStart={(e) => onDragStart(e, { kind: 'group', id: groupId })}
      onDragOver={onDragOver}
      onClick={(e) => {
        if (editing || e.detail >= 2) return
        openBefore.current = group.open
        void signet.toggleGroup(groupId)
      }}
      onContextMenu={(e) => {
        e.preventDefault()
        void signet.showGroupMenu(groupId)
      }}
    >
      <GroupIcon group={group} first={first} />
      {editing ? (
        <input
          className="folder-input"
          autoFocus
          value={value}
          onFocus={(e) => e.currentTarget.select()}
          onChange={(e) => setValue(e.target.value)}
          onClick={(e) => e.stopPropagation()}
          onBlur={commit}
          onKeyDown={(e) => {
            if (e.nativeEvent.isComposing) return
            if (e.key === 'Enter') e.currentTarget.blur()
            else if (e.key === 'Escape') {
              cancelled.current = true
              e.currentTarget.blur()
            }
          }}
        />
      ) : (
        <span
          className="title"
          onDoubleClick={() => {
            // The first click collapsed or expanded the group: put it back as it was.
            void signet.toggleGroup(groupId, openBefore.current)
            onRename(groupId)
          }}
        >
          {name}
        </span>
      )}
      <ChevronRight size={14} className={`chevron${group.open ? ' open' : ''}`} />
    </div>
  )
})

/** Tuile de favori. */
export const FavoriteTile = memo(function FavoriteTile({
  tab,
  active,
  dragSrc,
  indicator,
  onDragStart,
  onDragOver
}: {
  tab: TabInfo
  active: boolean
  dragSrc: boolean
  indicator: Indicator
  onDragStart: (e: DragEvent, src: DragSource) => void
  onDragOver: (e: DragEvent) => void
}) {
  const { t } = useTranslation()
  return (
    <div
      className={`tile${active ? ' active' : ''}${tab.loaded ? '' : ' unloaded'}${dragSrc ? ' drag-src' : ''}${indicator ? ` drop-${indicator}` : ''}`}
      data-flip-id={tab.id}
      title={displayTitle(tab, t('sidebar.newTab'))}
      draggable
      onDragStart={(e) => onDragStart(e, { kind: 'tab', tabId: tab.id })}
      onDragOver={onDragOver}
      {...tabHandlers(tab, () => activateTab(tab.id), active)}
    >
      <TabIcon tab={tab} size={18} />
    </div>
  )
})

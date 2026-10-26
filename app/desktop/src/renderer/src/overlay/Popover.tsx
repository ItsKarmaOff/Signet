import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import type { CSSProperties, DragEvent, ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { Check, ChevronRight, Folder, FolderOpen, Pencil, Plus } from 'lucide-react'
import type { Bookmark, DragSource, Item, OverlayPayload, PopoverRequest, Rect, Snapshot } from '@shared/types'
import { DownloadsPanel, useUnseenDownloads } from '../windows/Downloads'
import { SpaceForm } from '../settings/SpaceForm'
import { LiveFolderForm } from '../settings/LiveFolderForm'
import { FolderIcon, IconForm, TabIcon, displayTitle } from '../tabs/TabCustom'
import { DRAG_MIME, useHue, useSnapshot, wantsNewTab } from '../lib/util'
import { Favicon } from '../ui/Favicon'

type Payload = Extract<OverlayPayload, { mode: 'popover' }>

const WIDTH = { spaces: 248, folder: 288, 'space-form': 304, 'live-folder': 320, downloads: 344, icon: 304, rename: 264, bookmarks: 288 } as const

/**
 * Popover of the tab row (route #overlay, `popover` mode): dropdown menus and forms placed
 * above the web views, under the chip that opens them. A click outside or Escape closes them.
 */
export function Popover({ payload }: { payload: Payload }) {
  const signet = window.signet
  const s = useSnapshot()
  useHue(payload.hue)
  const { popover, anchor } = payload
  const width = WIDTH[popover.kind]
  const left = Math.max(8, Math.min(anchor.x, window.innerWidth - width - 8))
  const below = anchor.y + anchor.height + 6
  const slotRef = useRef<HTMLDivElement>(null)
  const [top, setTop] = useState(below)
  // If the popover overflows at the bottom, place it above the chip (or pin it against the bottom edge).
  // No dependencies: the height changes with the content; setState is guarded by equality.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useLayoutEffect(() => {
    const h = slotRef.current?.offsetHeight ?? 0
    let next = below
    if (below + h > window.innerHeight - 8) next = Math.max(8, anchor.y - 6 - h >= 8 ? anchor.y - 6 - h : window.innerHeight - 8 - h)
    setTop((t) => (t === next ? t : next))
  })
  const close = () => signet.closeOverlay()

  let body: ReactNode = null
  if (s) {
    if (popover.kind === 'spaces') body = <SpaceMenu s={s} anchor={anchor} />
    else if (popover.kind === 'folder') body = <FolderMenu s={s} folderId={popover.folderId} />
    else if (popover.kind === 'space-form') {
      body = <SpaceForm space={popover.spaceId ? (s.spaces.find((x) => x.id === popover.spaceId) ?? null) : null} onClose={close} />
    } else if (popover.kind === 'live-folder') body = <LiveFolderForm s={s} folderId={popover.folderId} onClose={close} />
    else if (popover.kind === 'downloads') body = <Downloads s={s} />
    else if (popover.kind === 'icon') body = <IconForm s={s} target={popover.target} onClose={close} />
    else if (popover.kind === 'bookmarks') body = <BookmarksMenu s={s} folderId={popover.folderId} from={popover.from} tabId={popover.tabId} />
    else body = <RenameForm s={s} target={popover.target} />
  }

  return (
    <div className="pop-layer" onMouseDown={() => signet.closeOverlay()}>
      <div
        ref={slotRef}
        className={`pop-slot pop-${popover.kind}`}
        style={{ left, top, width, '--origin-x': `${anchor.x + anchor.width / 2 - left}px` } as CSSProperties}
        onMouseDown={(e) => e.stopPropagation()}
      >
        {body}
      </div>
    </div>
  )
}

/** Delay before closing a submenu when the pointer leaves its row: leaves time to reach it diagonally. */
const SUB_CLOSE_MS = 250

/** Open submenu folder: opening is immediate, closing is deferred and cancelled by entering the submenu. */
function useSubmenu() {
  const [openId, setOpen] = useState<string | null>(null)
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null)
  const cancel = useCallback(() => {
    if (timer.current) clearTimeout(timer.current)
    timer.current = null
  }, [])
  useEffect(() => cancel, [cancel])
  const set = (id: string | null) => {
    cancel()
    setOpen(id)
  }
  const closeSoon = () => {
    cancel()
    timer.current = setTimeout(() => {
      timer.current = null
      setOpen(null)
    }, SUB_CLOSE_MS)
  }
  return { openId, set, closeSoon, cancel }
}

/**
 * Folder submenu: rendered in the popover layer (outside the scrolling menu, which would clip it) and placed to the right
 * of the `anchor` row, or to the left if it would leave the window.
 */
function Submenu({ anchor, onEnter, children }: { anchor: HTMLElement | null; onEnter: () => void; children: ReactNode }) {
  const ref = useRef<HTMLDivElement>(null)
  const [pos, setPos] = useState<{ left: number; top: number } | null>(null)
  useLayoutEffect(() => {
    const el = ref.current
    if (!el || !anchor) return
    const r = anchor.getBoundingClientRect()
    const w = el.offsetWidth
    const right = r.right + 2
    const left = right + w + 8 > window.innerWidth ? Math.max(8, r.left - 2 - w) : right
    const top = Math.max(8, Math.min(r.top - 6, window.innerHeight - el.offsetHeight - 8))
    setPos((p) => (p && p.left === left && p.top === top ? p : { left, top }))
  }, [anchor, children])
  const layer = anchor?.closest('.pop-layer')
  if (!layer) return null
  return createPortal(
    <div
      ref={ref}
      className="pop-menu pop-submenu"
      role="menu"
      onMouseEnter={onEnter}
      style={{ left: pos?.left ?? 0, top: pos?.top ?? 0, visibility: pos ? 'visible' : 'hidden' }}
    >
      {children}
    </div>,
    layer
  )
}

/** Spaces menu: choose, create, edit; each row also receives a dropped tab. */
function SpaceMenu({ s, anchor }: { s: Snapshot; anchor: Rect }) {
  const { t } = useTranslation()
  const signet = window.signet
  const [over, setOver] = useState<string | null>(null)

  const accepts = (e: DragEvent) => Array.from(e.dataTransfer.types).includes(DRAG_MIME)
  const dropOn = (spaceId: string) => (e: DragEvent) => {
    if (!accepts(e)) return
    e.preventDefault()
    setOver(null)
    try {
      const src = JSON.parse(e.dataTransfer.getData(DRAG_MIME)) as DragSource
      const space = s.spaces.find((x) => x.id === spaceId)
      // Same rule as the sidebar dots: a folder goes into pinned, the rest into today's tabs.
      void signet.moveItem(src, src.kind === 'folder' ? { zone: 'pinned', spaceId, index: space?.pinned.length ?? 0 } : { zone: 'today', spaceId, index: 0 })
    } catch {
      /* foreign drag: ignored */
    }
    signet.endTabDrag().catch(() => {})
    void signet.closeOverlay()
  }

  return (
    <div className="pop-menu" role="menu" aria-label={t('tabstrip.spaces')}>
      {s.spaces.map((sp) => (
        <button
          key={sp.id}
          role="menuitemradio"
          aria-checked={sp.id === s.activeSpaceId}
          className={`pop-item${over === sp.id ? ' over' : ''}`}
          style={{ ['--h' as string]: sp.hue }}
          onClick={() => (signet.switchSpace(sp.id), signet.closeOverlay())}
          onContextMenu={(e) => (e.preventDefault(), signet.showSpaceMenu(sp.id))}
          onDragEnter={(e) => accepts(e) && (e.preventDefault(), setOver(sp.id))}
          onDragOver={(e) => accepts(e) && (e.preventDefault(), (e.dataTransfer.dropEffect = 'move'), setOver(sp.id))}
          onDragLeave={() => setOver((o) => (o === sp.id ? null : o))}
          onDrop={dropOn(sp.id)}
        >
          <span className="emoji">{sp.icon}</span>
          <span className="label">{sp.name}</span>
          {sp.id === s.activeSpaceId && <Check size={14} className="check" />}
        </button>
      ))}
      <div className="pop-sep" />
      <button className="pop-item" role="menuitem" onClick={() => signet.openPopover({ kind: 'space-form', spaceId: null }, anchor)}>
        <Plus size={15} />
        <span className="label">{t('tabstrip.newSpace')}</span>
      </button>
      <button className="pop-item" role="menuitem" onClick={() => signet.openPopover({ kind: 'space-form', spaceId: s.activeSpaceId }, anchor)}>
        <Pencil size={14} />
        <span className="label">{t('tabstrip.editSpace')}</span>
      </button>
    </div>
  )
}

function findFolder(items: Item[], id: string): Extract<Item, { kind: 'folder' }> | null {
  for (const i of items) {
    if (i.kind !== 'folder') continue
    if (i.id === id) return i
    const f = findFolder(i.items, id)
    if (f) return f
  }
  return null
}

/** Folder contents: its tabs, and its subfolders as submenus (opened on hover). */
function findBookmark(items: Bookmark[], id: string): Bookmark | null {
  for (const i of items) {
    if (i.id === id) return i
    if (i.kind === 'folder') {
      const f = findBookmark(i.items, id)
      if (f) return f
    }
  }
  return null
}

function FolderItems({ items, s, depth }: { items: Item[]; s: Snapshot; depth: number }) {
  const { t } = useTranslation()
  const signet = window.signet
  const sub = useSubmenu()
  const openId = sub.openId
  const activeId = s.spaces.find((x) => x.id === s.activeSpaceId)?.activeTabId ?? null
  const [subEl, setSubEl] = useState<HTMLElement | null>(null)
  if (items.length === 0) return <span className="pop-empty">{t('sidebar.emptyFolder')}</span>

  const pick = (tabId: string) => {
    void signet.activateTab(tabId)
    void signet.closeOverlay()
  }
  return (
    <>
      {items.map((item) => {
        if (item.kind === 'folder') {
          const open = openId === item.id
          return (
            <div key={item.id} className="pop-sub" ref={open ? setSubEl : undefined} onMouseEnter={() => sub.set(item.id)}>
              <button
                className={`pop-item${open ? ' open' : ''}`}
                role="menuitem"
                aria-haspopup="menu"
                aria-expanded={open}
                style={item.hue !== undefined ? { ['--fh' as string]: item.hue } : undefined}
                onClick={() => sub.set(open ? null : item.id)}
                onContextMenu={(e) => (e.preventDefault(), signet.showFolderMenu(item.id))}
              >
                <FolderIcon folder={{ ...item, open }} className={item.hue !== undefined ? 'tint' : ''} />
                <span className="label">{item.name}</span>
                <ChevronRight size={14} className="chev" />
              </button>
              {open && (
                <Submenu anchor={subEl} onEnter={sub.cancel}>
                  <FolderItems items={item.items} s={s} depth={depth + 1} />
                </Submenu>
              )}
            </div>
          )
        }
        const ids = item.kind === 'tab' ? [item.tabId] : item.tabIds
        return ids.map((id) => {
          const tab = s.tabs[id]
          if (!tab) return null
          return (
            <button
              key={id}
              role="menuitem"
              className={`pop-item${id === activeId ? ' active' : ''}${tab.loaded ? '' : ' unloaded'}`}
              title={displayTitle(tab, t('sidebar.newTab'))}
              onMouseEnter={sub.closeSoon}
              onClick={() => pick(id)}
              onContextMenu={(e) => (e.preventDefault(), signet.showTabMenu(id))}
              onAuxClick={(e) => e.button === 1 && signet.closeTab(id)}
            >
              <TabIcon tab={tab} size={16} />
              <span className="label">{displayTitle(tab, t('sidebar.newTab'))}</span>
            </button>
          )
        })
      })}
    </>
  )
}

function findBookmarkFolder(items: Bookmark[], id: string): Extract<Bookmark, { kind: 'folder' }> | null {
  for (const i of items) {
    if (i.kind !== 'folder') continue
    if (i.id === id) return i
    const f = findBookmarkFolder(i.items, id)
    if (f) return f
  }
  return null
}

/** Bookmarks of a folder (or what overflows the bar): links, and subfolders as submenus opened on hover. */
function BookmarkItems({ items, tabId, depth }: { items: Bookmark[]; tabId: string | null; depth: number }) {
  const { t } = useTranslation()
  const signet = window.signet
  const sub = useSubmenu()
  const openId = sub.openId
  const [subEl, setSubEl] = useState<HTMLElement | null>(null)
  if (items.length === 0) return <span className="pop-empty">{t('sidebar.emptyFolder')}</span>
  return (
    <>
      {items.map((item) => {
        if (item.kind === 'folder') {
          const open = openId === item.id
          return (
            <div key={item.id} className="pop-sub" ref={open ? setSubEl : undefined} onMouseEnter={() => sub.set(item.id)}>
              <button
                className={`pop-item${open ? ' open' : ''}`}
                role="menuitem"
                aria-haspopup="menu"
                aria-expanded={open}
                onClick={() => sub.set(open ? null : item.id)}
                onContextMenu={(e) => (e.preventDefault(), signet.showBookmarkMenu(item.id, tabId))}
              >
                {open ? <FolderOpen size={16} /> : <Folder size={16} />}
                <span className="label">{item.name}</span>
                <ChevronRight size={14} className="chev" />
              </button>
              {open && (
                <Submenu anchor={subEl} onEnter={sub.cancel}>
                  <BookmarkItems items={item.items} tabId={tabId} depth={depth + 1} />
                </Submenu>
              )}
            </div>
          )
        }
        return (
          <button
            key={item.id}
            role="menuitem"
            className="pop-item"
            title={item.url}
            onMouseEnter={sub.closeSoon}
            onClick={(e) => {
              void signet.openBookmark(item.id, tabId, wantsNewTab(e))
              void signet.closeOverlay()
            }}
            onMouseDown={(e) => e.button === 1 && e.preventDefault()}
            onAuxClick={(e) => e.button === 1 && void signet.openBookmark(item.id, tabId, true)}
            onContextMenu={(e) => (e.preventDefault(), signet.showBookmarkMenu(item.id, tabId))}
          >
            <Favicon src={item.favicon} size={16} />
            <span className="label">{item.title}</span>
          </button>
        )
      })}
    </>
  )
}

function BookmarksMenu({ s, folderId, from, tabId }: { s: Snapshot; folderId: string | null; from?: number; tabId: string | null }) {
  const space = s.spaces.find((x) => x.id === s.activeSpaceId) ?? s.spaces[0]
  const all = space.bookmarks ?? []
  const items = folderId ? findBookmarkFolder(all, folderId)?.items : all.slice(from ?? 0)
  if (!items) return null
  return (
    <div className="pop-menu" role="menu">
      <BookmarkItems items={items} tabId={tabId} depth={0} />
    </div>
  )
}

function FolderMenu({ s, folderId }: { s: Snapshot; folderId: string }) {
  const space = s.spaces.find((x) => x.id === s.activeSpaceId) ?? s.spaces[0]
  const folder = findFolder(space.pinned, folderId)
  if (!folder) return null
  return (
    <div className="pop-menu" role="menu" aria-label={folder.name}>
      <FolderItems items={folder.items} s={s} depth={0} />
    </div>
  )
}

function Downloads({ s }: { s: Snapshot }) {
  // The panel is open: whatever finishes in the meantime is seen.
  useUnseenDownloads(s.downloads, true)
  return <DownloadsPanel downloads={s.downloads} onClose={() => window.signet.closeOverlay()} />
}

/** Rename field for a tab, folder or tab group that has no in-place field on screen. */
function RenameForm({ s, target }: { s: Snapshot; target: Extract<PopoverRequest, { kind: 'rename' }>['target'] }) {
  const { t } = useTranslation()
  const signet = window.signet
  const space = s.spaces.find((x) => x.id === s.activeSpaceId) ?? s.spaces[0]
  const tab = target.kind === 'tab' ? s.tabs[target.tabId] : undefined
  const folder = target.kind === 'folder' ? findFolder(space.pinned, target.folderId) : null
  const group = target.kind === 'group' ? space.groups?.[target.groupId] : undefined
  const bookmark = target.kind === 'bookmark' ? findBookmark(space.bookmarks ?? [], target.id) : null
  const initial = tab
    ? (tab.customTitle ?? tab.title)
    : (folder?.name ?? group?.name ?? (bookmark ? (bookmark.kind === 'link' ? bookmark.title : bookmark.name) : ''))
  const [value, setValue] = useState(initial)
  const ref = useRef<HTMLInputElement>(null)
  useEffect(() => {
    ref.current?.focus()
    ref.current?.select()
  }, [])

  const commit = () => {
    const name = value.trim()
    if (target.kind === 'tab') {
      // Empty or identical to the page title: back to the original title.
      void signet.renameTab(target.tabId, name && name !== tab?.title ? name : null)
    } else if (target.kind === 'bookmark') {
      if (name && name !== initial) void signet.renameBookmark(target.id, name)
    } else if (target.kind === 'group') {
      if (name && name !== group?.name) void signet.renameGroup(target.groupId, name)
    } else if (name && name !== folder?.name) void signet.renameFolder(target.folderId, name)
    void signet.closeOverlay()
  }

  return (
    <form
      className="space-form rename-form"
      onSubmit={(e) => (e.preventDefault(), commit())}
      onKeyDown={(e) => e.key === 'Escape' && !e.nativeEvent.isComposing && signet.closeOverlay()}
    >
      <input
        ref={ref}
        value={value}
        maxLength={200}
        placeholder={tab?.title}
        aria-label={
          target.kind === 'tab'
            ? t('tabstrip.renameTab')
            : target.kind === 'group'
              ? t('tabstrip.renameGroup')
              : target.kind === 'bookmark'
                ? t('tabstrip.renameBookmark')
                : t('tabstrip.renameFolder')
        }
        onChange={(e) => setValue(e.target.value)}
      />
      <div className="actions">
        <button type="button" onClick={() => signet.closeOverlay()}>
          {t('common.cancel')}
        </button>
        <button type="submit" className="primary">
          {t('tabstrip.rename')}
        </button>
      </div>
    </form>
  )
}

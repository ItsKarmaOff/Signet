import { memo, useEffect, useLayoutEffect, useRef, useState } from 'react'
import type { DragEvent, MouseEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { ChevronDown, ChevronsRight, Folder } from 'lucide-react'
import type { Bookmark, BookmarkTarget, DragSource, Rect } from '@shared/types'
import { Favicon } from '../ui/Favicon'
import { DRAG_MIME, wantsNewTab } from '../lib/util'
import { useFlip } from '../lib/flip'

/** MIME type of a bookmark being dragged (reorder in the bar or file into a folder). */
export const BOOKMARK_MIME = 'application/x-signet-bookmark'
/** Room reserved for the "»" button when bookmarks overflow. */
const MORE = 30

const rectOf = (el: Element): Rect => {
  const r = el.getBoundingClientRect()
  return { x: r.left, y: r.top, width: r.width, height: r.height }
}

/** What can be dropped on the bar: a bookmark, or a tab from the sidebar or the top row. */
const droppable = (e: DragEvent) => e.dataTransfer.types.includes(BOOKMARK_MIME) || e.dataTransfer.types.includes(DRAG_MIME)

/** Drop on the bar: moves the dragged bookmark, or adds the dragged tab, at `target`. */
export function dropBookmark(e: DragEvent, target: BookmarkTarget): void {
  e.preventDefault()
  const id = e.dataTransfer.getData(BOOKMARK_MIME)
  if (id) return void window.signet.moveBookmark(id, target)
  const raw = e.dataTransfer.getData(DRAG_MIME)
  if (!raw) return
  const src = JSON.parse(raw) as DragSource
  if (src.kind === 'tab') void window.signet.addBookmark(src.tabId, target)
}

const hasBookmark = (items: Bookmark[], id: string): boolean => items.some((b) => b.id === id || (b.kind === 'folder' && hasBookmark(b.items, id)))

type Mark = { index: number; side: 'before' | 'after' | 'into' } | null

/**
 * The space's bookmarks bar, under a pane's address bar. Bookmarks that do not fit go into the
 * "»" menu; a folder opens as a menu above the pages. Click: in the pane's tab; ⌘click (Ctrl+click) or
 * middle click: new tab. Drag: reorder, file into a folder, or drop a tab to add it.
 */
export const BookmarksBar = memo(function BookmarksBar({
  items,
  tabId,
  focused,
  keys
}: {
  items: Bookmark[]
  tabId: string | null
  focused: boolean
  /** Current "Add to bookmarks" shortcut (absent: no shortcut). */
  keys?: string
}) {
  const { t } = useTranslation()
  const signet = window.signet
  const rowRef = useRef<HTMLDivElement>(null)
  const moreRef = useRef<HTMLButtonElement>(null)
  const [fit, setFit] = useState(items.length)
  const [mark, setMark] = useState<Mark>(null)

  // Right edge of each child (relative to the row) when everything is shown: measured once per set of bookmarks.
  const rights = useRef<number[]>([])
  // How many bookmarks fit, from the cached widths: no DOM read before the final write (no React render).
  const fitFromCache = () => {
    const row = rowRef.current
    if (!row) return
    const kids = [...row.children] as HTMLElement[]
    const rs = rights.current
    const width = row.clientWidth
    let n = kids.length
    if (n && rs[n - 1] > width) {
      // The "»" button already takes its place (flex) if it is shown; otherwise we reserve it for it.
      const limit = width - (moreRef.current ? 0 : MORE)
      n = rs.findIndex((r) => r > limit)
      if (n < 0) n = kids.length
    }
    kids.forEach((k, i) => {
      const d = i < n ? '' : 'none'
      if (k.style.display !== d) k.style.display = d
    })
    setFit((f) => (f === n ? f : n))
  }
  // Full measurement: put all the children back, read their widths, then hide the ones that overflow.
  const measure = () => {
    const row = rowRef.current
    if (!row) return
    const kids = [...row.children] as HTMLElement[]
    for (const k of kids) k.style.display = ''
    const x0 = row.getBoundingClientRect().left
    rights.current = kids.map((k) => k.getBoundingClientRect().right - x0)
    fitFromCache()
  }
  // Resize: the cache is enough as long as the children have not changed.
  const onResize = () => (rights.current.length === (rowRef.current?.children.length ?? 0) ? fitFromCache() : measure())
  useLayoutEffect(measure, [items])
  // Bookmarks slide to their new place when the order changes (after `measure`, which hides the overflowing ones).
  useFlip(rowRef, items.map((b) => b.id).join(','), 'bar')
  useEffect(() => {
    const row = rowRef.current
    if (!row) return
    const ro = new ResizeObserver(onResize)
    ro.observe(row)
    // Fonts change widths without resizing the row.
    void document.fonts?.ready.then(measure)
    return () => ro.disconnect()
    // `measure` and `onResize` only read refs: taking them at mount is enough.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  // Rename (requested by the bookmark's menu): field under the bookmark, or under "»" if it is hidden.
  useEffect(
    () =>
      signet.onRenameBookmark(({ id }) => {
        if (!focused) return
        const visible = (el: HTMLElement | null | undefined) => (el && el.style.display !== 'none' ? el : null)
        const top = (id: string) => rowRef.current?.querySelector<HTMLElement>(`[data-bm="${id}"]`)
        // Bookmark filed in a folder: anchor on the bar folder that contains it.
        const holder = items.find((b) => b.id === id || (b.kind === 'folder' && hasBookmark(b.items, id)))
        const anchor = visible(top(id)) ?? (holder && visible(top(holder.id))) ?? moreRef.current ?? rowRef.current
        if (anchor) void signet.openPopover({ kind: 'rename', target: { kind: 'bookmark', id } }, rectOf(anchor))
      }),
    [signet, focused, items]
  )

  const over = (i: number, item: Bookmark) => (e: DragEvent) => {
    if (!droppable(e)) return
    e.preventDefault()
    e.stopPropagation()
    e.dataTransfer.dropEffect = 'move'
    const r = e.currentTarget.getBoundingClientRect()
    const x = (e.clientX - r.left) / r.width
    // Folder: the middle files inside, the edges place before or after.
    const side = item.kind === 'folder' && x > 0.25 && x < 0.75 ? 'into' : x < 0.5 ? 'before' : 'after'
    setMark((m) => (m?.index === i && m.side === side ? m : { index: i, side }))
  }
  const target = (m: NonNullable<Mark>): BookmarkTarget => {
    const item = items[m.index]
    if (m.side === 'into' && item?.kind === 'folder') return { folderId: item.id, index: item.items.length }
    return { index: m.index + (m.side === 'after' ? 1 : 0) }
  }
  const drop = (e: DragEvent) => {
    const m = mark
    setMark(null)
    dropBookmark(e, m ? target(m) : { index: items.length })
  }

  const click = (item: Bookmark) => (e: MouseEvent<HTMLElement>) => {
    if (item.kind === 'folder') return signet.openPopover({ kind: 'bookmarks', folderId: item.id, tabId }, rectOf(e.currentTarget))
    void signet.openBookmark(item.id, tabId, wantsNewTab(e))
  }

  return (
    <div
      className={`bm-bar${mark ? ' dragging' : ''}`}
      onContextMenu={(e) => (e.preventDefault(), signet.showBookmarksBarMenu())}
      onDragOver={(e) => {
        if (!droppable(e)) return
        e.preventDefault()
        setMark(null)
      }}
      onDragLeave={(e) => {
        if (!e.currentTarget.contains(e.relatedTarget as Node | null)) setMark(null)
      }}
      onDrop={drop}
    >
      <div className="bm-items" ref={rowRef}>
        {items.map((item, i) => (
          <button
            key={item.id}
            data-bm={item.id}
            data-flip-id={item.id}
            className={`bm-item${item.kind === 'folder' ? ' bm-folder' : ''}${mark?.index === i ? ` drop-${mark.side}` : ''}`}
            title={item.kind === 'link' ? `${item.title}\n${item.url}` : item.name}
            draggable
            onDragStart={(e) => {
              e.dataTransfer.setData(BOOKMARK_MIME, item.id)
              e.dataTransfer.effectAllowed = 'move'
            }}
            onDragEnd={() => setMark(null)}
            onDragOver={over(i, item)}
            onClick={click(item)}
            onMouseDown={(e) => e.button === 1 && e.preventDefault()}
            onAuxClick={(e) => e.button === 1 && item.kind === 'link' && void signet.openBookmark(item.id, tabId, true)}
            onContextMenu={(e) => {
              e.preventDefault()
              e.stopPropagation()
              void signet.showBookmarkMenu(item.id, tabId)
            }}
          >
            {item.kind === 'link' ? <Favicon src={item.favicon} size={14} /> : <Folder size={14} />}
            <span className="bm-title">{item.kind === 'link' ? item.title : item.name}</span>
            {item.kind === 'folder' && <ChevronDown size={12} className="bm-caret" />}
          </button>
        ))}
        {items.length === 0 && <span className="bm-hint">{keys ? t('bookmarks.hint', { keys }) : t('bookmarks.hintNoKeys')}</span>}
      </div>
      {fit < items.length && (
        <button
          ref={moreRef}
          className="icon bm-more"
          title={t('bookmarks.more')}
          onClick={(e) => signet.openPopover({ kind: 'bookmarks', folderId: null, from: fit, tabId }, rectOf(e.currentTarget))}
        >
          <ChevronsRight size={15} />
        </button>
      )}
    </div>
  )
})

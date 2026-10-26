import { createContext, useContext, useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { CircleAlert, Folder, FolderOpen, GitPullRequest, RefreshCw, Rss } from 'lucide-react'
import type { IconTarget, Item, Snapshot, TabGroup, TabInfo } from '@shared/types'
import { EmojiPicker } from '../ui/EmojiPicker'
import { Favicon } from '../ui/Favicon'

/** Tab whose title is being edited in the sidebar (provided by `Sidebar`). */
export const TabEditContext = createContext<{ editing: string | null; setEditing: (id: string | null) => void }>({
  editing: null,
  setEditing: () => {}
})

/** Displayed title: the name chosen by the user, otherwise the page's. */
export const displayTitle = (tab: TabInfo, fallback: string): string => tab.customTitle || tab.title || tab.url || fallback

/** Emoji shown in place of an icon, in a `size` px box. */
export function EmojiIcon({ emoji, size = 16 }: { emoji: string; size?: number }) {
  return (
    <span className="tab-emoji" style={{ fontSize: size - 1, width: size, height: size }} aria-hidden>
      {emoji}
    </span>
  )
}

/** Tab icon: the chosen emoji in place of the favicon. */
export function TabIcon({ tab, size = 16 }: { tab: TabInfo; size?: number }) {
  if (tab.customIcon) return <EmojiIcon emoji={tab.customIcon} size={size} />
  return <Favicon src={tab.favicon} size={size} loading={tab.loading} />
}

type FolderItem = Extract<Item, { kind: 'folder' }>

/** Folder icon: the chosen emoji, otherwise the open or closed folder. */
export function FolderIcon({ folder, size = 16, className }: { folder: FolderItem; size?: number; className?: string }) {
  if (folder.live) return <LiveIcon folder={folder} size={size} className={className} />
  if (folder.icon) return <EmojiIcon emoji={folder.icon} size={size} />
  return folder.open ? <FolderOpen size={size} className={className} /> : <Folder size={size} className={className} />
}

/** Live folder icon: its source (feed, GitHub) or the chosen emoji; spins while updating, flags a failure. */
function LiveIcon({ folder, size, className }: { folder: FolderItem; size: number; className?: string }) {
  const { t } = useTranslation()
  const live = folder.live!
  const title = live.refreshing ? t('folder.liveRefreshing') : live.error ? t('folder.liveError', { error: live.error }) : t('folder.live')
  const Icon = live.source.kind === 'github' ? GitPullRequest : Rss
  return (
    <span className={`live-icon${live.refreshing ? ' refreshing' : ''}${live.error ? ' failed' : ''}`} title={title}>
      {folder.icon ? <EmojiIcon emoji={folder.icon} size={size} /> : <Icon size={size} className={className} />}
      {live.refreshing && <RefreshCw size={Math.round(size * 0.6)} className="live-spin" />}
      {!live.refreshing && live.error && <CircleAlert size={Math.round(size * 0.6)} className="live-alert" />}
    </span>
  )
}

/** Tab group icon: the chosen emoji, otherwise its first tab's. */
export function GroupIcon({ group, first, size = 16 }: { group: TabGroup | undefined; first: TabInfo | undefined; size?: number }) {
  if (group?.icon) return <EmojiIcon emoji={group.icon} size={size} />
  return first ? <TabIcon tab={first} size={size} /> : <Folder size={size} />
}

/** Title of a tab row; double-click = rename in place (Enter confirms, Escape cancels, empty = page title). */
export function TabTitle({ tab }: { tab: TabInfo }) {
  const { t } = useTranslation()
  const { editing, setEditing } = useContext(TabEditContext)
  const shown = displayTitle(tab, t('sidebar.newTab'))
  const [value, setValue] = useState(shown)
  const cancelled = useRef(false)
  const on = editing === tab.id
  // Value set when entering edit mode, before the field mounts: its selection (everything) is no longer overwritten.
  const [wasOn, setWasOn] = useState(on)
  if (on !== wasOn) {
    setWasOn(on)
    if (on) setValue(tab.customTitle ?? shown)
  }
  useEffect(() => {
    if (on) cancelled.current = false
  }, [on])

  if (!on) {
    return (
      <span className="title" onDoubleClick={() => setEditing(tab.id)}>
        {shown}
      </span>
    )
  }
  const commit = () => {
    const next = value.trim()
    if (!cancelled.current && next !== (tab.customTitle ?? '') && !(next === tab.title && !tab.customTitle)) {
      void window.signet.renameTab(tab.id, next || null)
    }
    setEditing(null)
  }
  return (
    <input
      className="folder-input"
      autoFocus
      value={value}
      maxLength={200}
      placeholder={tab.title}
      onFocus={(e) => e.currentTarget.select()}
      onChange={(e) => setValue(e.target.value)}
      onClick={(e) => e.stopPropagation()}
      onDoubleClick={(e) => e.stopPropagation()}
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
  )
}

function findFolderIn(items: Item[], id: string): FolderItem | null {
  for (const i of items) {
    if (i.kind !== 'folder') continue
    if (i.id === id) return i
    const inner = findFolderIn(i.items, id)
    if (inner) return inner
  }
  return null
}

const tabIdsOf = (i: Item): string[] => (i.kind === 'tab' ? [i.tabId] : i.kind === 'split' ? i.tabIds : i.items.flatMap(tabIdsOf))

/** What the panel shows and edits for a target: preview, name, current emoji. */
function describe(s: Snapshot, target: IconTarget, fallback: string): { preview: React.ReactNode; name: string; current?: string } | null {
  if (target.kind === 'tab') {
    const tab = s.tabs[target.tabId]
    return tab ? { preview: <TabIcon tab={tab} size={18} />, name: displayTitle(tab, fallback), current: tab.customIcon } : null
  }
  if (target.kind === 'folder') {
    for (const sp of s.spaces) {
      const folder = findFolderIn(sp.pinned, target.folderId)
      if (folder) return { preview: <FolderIcon folder={folder} size={18} />, name: folder.name, current: folder.icon }
    }
    return null
  }
  for (const sp of s.spaces) {
    const group = sp.groups?.[target.groupId]
    if (!group) continue
    const firstItem = sp.today.find((i) => i.kind !== 'folder' && i.group === target.groupId)
    const first = firstItem ? s.tabs[tabIdsOf(firstItem)[0]] : undefined
    return {
      preview: <GroupIcon group={group} first={first} size={18} />,
      name: group.name || (first ? displayTitle(first, fallback) : ''),
      current: group.icon
    }
  }
  return null
}

/** "Choose an icon" panel for a tab, folder or group, confined to the sidebar like the space form. */
export function IconForm({ s, target, onClose }: { s: Snapshot; target: IconTarget; onClose: () => void }) {
  const { t } = useTranslation()
  const info = describe(s, target, t('sidebar.newTab'))
  if (!info) return null
  return (
    <div className="space-form tab-icon-form" onKeyDown={(e) => e.key === 'Escape' && !e.nativeEvent.isComposing && onClose()}>
      <div className="tab-icon-head">
        {info.preview}
        <span className="tab-icon-name">{info.name}</span>
      </div>
      <EmojiPicker
        onPick={(emoji) => {
          void window.signet.setIcon(target, emoji)
          onClose()
        }}
      />
      <div className="actions">
        <button type="button" onClick={onClose}>
          {t('common.cancel')}
        </button>
        <button
          type="button"
          disabled={!info.current}
          onClick={() => {
            void window.signet.setIcon(target, null)
            onClose()
          }}
        >
          {t('tabIcon.remove')}
        </button>
      </div>
    </div>
  )
}

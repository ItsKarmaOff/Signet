import { memo } from 'react'
import { Plus } from 'lucide-react'
import type { Item, Space, TabInfo } from '@shared/types'
import { useTranslation } from 'react-i18next'
import { displayTitle, FolderIcon, GroupIcon, TabIcon } from '../tabs/TabCustom'
import { runsOf } from '../tabs/groups'

const idsOf = (items: Item[]): string[] => items.flatMap((i) => (i.kind === 'tab' ? [i.tabId] : i.kind === 'split' ? i.tabIds : idsOf(i.items)))

type Props = { space: Space | null; tabs: Record<string, TabInfo> }

/** Memo: the preview only redraws if its space or one of its tabs changed. */
function samePreview(a: Props, b: Props): boolean {
  if (a.space !== b.space) return false
  if (a.tabs === b.tabs || !b.space) return true
  return [...idsOf(b.space.pinned), ...idsOf(b.space.today)].every((id) => a.tabs[id] === b.tabs[id])
}

/** Read-only preview of a neighboring space, visible while dragging. */
export const SpacePreview = memo(function SpacePreview({ space, tabs }: Props) {
  const { t } = useTranslation()
  if (!space) return <div className="space-page preview" aria-hidden />
  const rows = (items: Item[], depth = 0): React.ReactNode =>
    items.map((item) => {
      if (item.kind === 'folder') {
        return (
          <div key={item.id} className="folder">
            <div className="row folder-row">
              <FolderIcon folder={item} />
              <span className="title">{item.name}</span>
            </div>
            {item.open && depth < 3 && <div className="folder-children">{rows(item.items, depth + 1)}</div>}
          </div>
        )
      }
      const ids = item.kind === 'tab' ? [item.tabId] : item.tabIds
      return ids.map((id) => {
        const tab = tabs[id]
        return tab ? (
          <div key={id} className={`row${tab.loaded ? '' : ' unloaded'}`}>
            <TabIcon tab={tab} />
            <span className="title">{displayTitle(tab, t('sidebar.newTab'))}</span>
          </div>
        ) : null
      })
    })

  return (
    <div className="space-page preview" aria-hidden style={{ ['--h' as string]: space.hue }}>
      <div className="list pinned">{rows(space.pinned)}</div>
      <div className="divider">
        <span className="line" />
      </div>
      <div className="row new-tab">
        <Plus size={16} />
        <span className="title">{t('sidebar.newTab')}</span>
      </div>
      <div className="list today">
        {runsOf(space.today).map((run) => {
          if (!run.group) return rows(space.today.slice(run.start, run.end))
          const header = space.groups?.[run.group]
          const members = space.today.slice(run.start, run.end)
          const first = tabs[idsOf(members)[0]]
          return (
            <div key={`g:${run.group}`} className="folder tinted tgroup" style={{ ['--fh' as string]: header?.hue ?? 215 }}>
              <div className="row folder-row group-row">
                <GroupIcon group={header} first={first} />
                <span className="title">{header?.name || (first ? displayTitle(first, t('sidebar.newTab')) : '')}</span>
              </div>
              {header?.open !== false && <div className="folder-children">{rows(members)}</div>}
            </div>
          )
        })}
      </div>
    </div>
  )
}, samePreview)

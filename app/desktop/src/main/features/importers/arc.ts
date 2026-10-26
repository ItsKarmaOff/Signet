// Tolerant reading of StorableSidebar.json (Arc): file content -> imported spaces.
import { isWebUrl, type ImportedNode, type ImportedSpace, type ImportedTab, type SpacesImport } from './types'
import { t } from '../../app/i18n'

type Obj = Record<string, unknown>
const isObj = (v: unknown): v is Obj => !!v && typeof v === 'object' && !Array.isArray(v)
const MAX_DEPTH = 8
const EMOJI = /\p{Extended_Pictographic}/u

/** Arrays alternate identifier and object (or contain only objects): we index by `id`. */
function indexById(list: unknown): Map<string, Obj> {
  const map = new Map<string, Obj>()
  if (!Array.isArray(list)) return map
  for (const v of list) if (isObj(v) && typeof v.id === 'string') map.set(v.id, v)
  return map
}

/** First text value that is an emoji (descending into objects), otherwise ✨. */
function emojiOf(v: unknown, depth = 0): string | null {
  if (typeof v === 'string') return EMOJI.test(v) ? v : null
  if (typeof v === 'number' && v > 0x1f && v <= 0x10ffff) {
    try {
      const c = String.fromCodePoint(v)
      return EMOJI.test(c) ? c : null
    } catch {
      return null
    }
  }
  if (depth > 3 || !v || typeof v !== 'object') return null
  for (const x of Object.values(v)) {
    const e = emojiOf(x, depth + 1)
    if (e) return e
  }
  return null
}

/** Id of the "pinned" or "unpinned" root in `containerIDs` (label followed by an id). */
function rootId(containerIDs: unknown, label: string): string | null {
  if (!Array.isArray(containerIDs)) return null
  const i = containerIDs.indexOf(label)
  const next = i >= 0 ? containerIDs[i + 1] : null
  return typeof next === 'string' ? next : null
}

export function parseSpacesFile(json: unknown): SpacesImport {
  const containers = isObj(json) && isObj(json.sidebar) && Array.isArray(json.sidebar.containers) ? json.sidebar.containers : []
  const useful = containers.find((c): c is Obj => isObj(c) && Array.isArray(c.spaces) && Array.isArray(c.items))
  if (!useful) throw new Error(t('import.error.format'))
  const items = indexById(useful.items)
  const spaces = indexById(useful.spaces)

  const children = (item: Obj): Obj[] =>
    (Array.isArray(item.childrenIds) ? item.childrenIds : []).map((id) => (typeof id === 'string' ? items.get(id) : undefined)).filter((x): x is Obj => !!x)

  const tabOf = (item: Obj): ImportedTab | null => {
    const tab = isObj(item.data) && isObj(item.data.tab) ? item.data.tab : null
    if (!tab || !isWebUrl(tab.savedURL)) return null
    const url = tab.savedURL.trim()
    const title = [item.title, tab.savedTitle].find((v): v is string => typeof v === 'string' && !!v.trim())
    return { url, title: title?.trim() ?? url }
  }

  /** Items of a tree; `folders` false = folders are flattened (today tabs, favorites). */
  const convert = (item: Obj, depth: number, seen: Set<string>, folders: boolean): ImportedNode[] => {
    const id = typeof item.id === 'string' ? item.id : null
    if (!id || seen.has(id) || depth > MAX_DEPTH) return []
    seen.add(id)
    const data = isObj(item.data) ? item.data : {}
    const kids = () => children(item).flatMap((c) => convert(c, depth + 1, seen, folders))
    if (data.tab) {
      const t = tabOf(item)
      return t ? [{ kind: 'tab', ...t }] : []
    }
    if (data.list) {
      const inner = kids()
      if (!folders) return inner
      const name = typeof item.title === 'string' && item.title.trim() ? item.title.trim() : t('defaults.folderFallback')
      return [{ kind: 'folder', name, items: inner }]
    }
    if (data.splitView) {
      const tabs = children(item).flatMap((c) => {
        if (seen.has(String(c.id))) return []
        seen.add(String(c.id))
        const t = tabOf(c)
        return t ? [t] : []
      })
      if (tabs.length === 0) return []
      if (tabs.length === 1) return [{ kind: 'tab', ...tabs[0] }]
      return [{ kind: 'split', tabs: tabs.slice(0, 3) }, ...tabs.slice(3).map((t): ImportedNode => ({ kind: 'tab', ...t }))]
    }
    if (data.itemContainer) return kids()
    return []
  }

  const tree = (id: string | null, folders: boolean): ImportedNode[] => {
    const root = id ? items.get(id) : undefined
    return root ? convert(root, 0, new Set(), folders) : []
  }

  const out: ImportedSpace[] = []
  let n = 0
  for (const sp of spaces.values()) {
    n++
    const name = typeof sp.title === 'string' && sp.title.trim() ? sp.title.trim() : t('defaults.spaceNumbered', { number: n })
    const info = isObj(sp.customInfo) ? sp.customInfo : {}
    out.push({
      name,
      icon: emojiOf(info.iconType) ?? '✨',
      pinned: tree(rootId(sp.containerIDs, 'pinned'), true),
      today: tree(rootId(sp.containerIDs, 'unpinned'), false)
    })
  }

  // Global favorites: `topAppsContainerIDs` alternates descriptor and container id.
  const favorites: ImportedTab[] = []
  const seenUrls = new Set<string>()
  const tops = containers.flatMap((c) => (isObj(c) && Array.isArray(c.topAppsContainerIDs) ? c.topAppsContainerIDs : []))
  for (const id of tops) {
    if (typeof id !== 'string') continue
    for (const node of tree(id, false)) {
      if (node.kind !== 'tab' || seenUrls.has(node.url)) continue
      seenUrls.add(node.url)
      favorites.push({ url: node.url, title: node.title })
    }
  }
  return { spaces: out, favorites }
}

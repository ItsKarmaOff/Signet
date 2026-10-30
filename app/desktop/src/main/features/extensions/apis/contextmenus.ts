import { readFileSync } from 'node:fs'
import { AtomicWriter } from '../../../core/atomic'
import { ApiError, type ApiCaller, type ApiHandler, type ExtensionEmitter } from './types'
import { urlMatches } from './urlmatch'

export const MENU_CLICK_EVENT = 'contextMenus.onClicked'

/** Contexts of the extension button's own menu. */
export const ACTION_CONTEXTS = ['action', 'browser_action', 'page_action'] as const

export type MenuType = 'normal' | 'checkbox' | 'radio' | 'separator'

export interface MenuItem {
  /** As the extension gave it (string or number); keys use its string form. */
  id: string | number
  type: MenuType
  title: string
  checked: boolean
  enabled: boolean
  visible: boolean
  contexts: string[]
  parentId?: string | number
  documentUrlPatterns?: string[]
  targetUrlPatterns?: string[]
}

const TYPES = new Set<string>(['normal', 'checkbox', 'radio', 'separator'])

const list = (v: unknown): string[] | undefined => (Array.isArray(v) ? v.map(String) : undefined)

/** Properties that `create` and `update` accept, applied over an item. */
export function mergeProps(item: MenuItem, props: Record<string, unknown>): MenuItem {
  const next: MenuItem = { ...item }
  if (typeof props.type === 'string') {
    if (!TYPES.has(props.type)) throw new ApiError(`Invalid value for argument: type '${props.type}'.`)
    next.type = props.type as MenuType
  }
  if (typeof props.title === 'string') next.title = props.title
  if (typeof props.checked === 'boolean') next.checked = props.checked
  if (typeof props.enabled === 'boolean') next.enabled = props.enabled
  if (typeof props.visible === 'boolean') next.visible = props.visible
  if (props.contexts !== undefined) next.contexts = list(Array.isArray(props.contexts) ? props.contexts : [props.contexts]) ?? next.contexts
  if (props.documentUrlPatterns !== undefined) next.documentUrlPatterns = list(props.documentUrlPatterns)
  if (props.targetUrlPatterns !== undefined) next.targetUrlPatterns = list(props.targetUrlPatterns)
  if (props.parentId !== undefined) next.parentId = props.parentId as string | number
  if (next.type !== 'separator' && !next.title) throw new ApiError('All menu items except for separators must have a title.')
  return next
}

/** What was right-clicked, in the terms of `ContextMenuParams`. */
export interface MenuTarget {
  pageURL?: string
  frameURL?: string
  linkURL?: string
  srcURL?: string
  selectionText?: string
  mediaType?: string
  isEditable?: boolean
}

/** Contexts a click falls in. `page` only applies when nothing more specific was clicked, like Chrome. */
export function contextsOf(t: MenuTarget): Set<string> {
  const out = new Set<string>(['all'])
  if (t.mediaType === 'image' || t.mediaType === 'video' || t.mediaType === 'audio') out.add(t.mediaType)
  if (t.linkURL) out.add('link')
  if (t.frameURL) out.add('frame')
  if (t.selectionText) out.add('selection')
  if (t.isEditable) out.add('editable')
  if (out.size === 1 || (out.size === 2 && out.has('frame'))) out.add('page')
  return out
}

/** Does the item belong in a menu for these contexts (and, for a page menu, this target)? */
export function itemMatches(item: MenuItem, contexts: ReadonlySet<string>, target?: MenuTarget): boolean {
  if (!item.visible) return false
  const wanted = item.contexts.length ? item.contexts : ['page']
  if (!wanted.some((c) => contexts.has(c))) return false
  if (target) {
    const targetUrl = target.srcURL || target.linkURL
    const documentUrl = target.frameURL || target.pageURL
    if (item.targetUrlPatterns?.length && targetUrl && !item.targetUrlPatterns.some((p) => urlMatches(p, targetUrl))) return false
    if (item.documentUrlPatterns?.length && documentUrl && !item.documentUrlPatterns.some((p) => urlMatches(p, documentUrl))) return false
  }
  return true
}

/** An entry to show: the item, its label for this click and its submenu. */
export interface MenuNode {
  item: MenuItem
  label: string
  children: MenuNode[]
}

/** `%s` stands for the selected text. */
export function labelFor(title: string, selection?: string): string {
  return selection && title.includes('%s') ? title.split('%s').join(selection) : title
}

export interface ClickInfo {
  menuItemId: string | number
  parentMenuItemId?: string | number
  checked?: boolean
  wasChecked?: boolean
}

/** Items of every extension, with the rules of `chrome.contextMenus`. */
export class ContextMenus {
  private items = new Map<string, Map<string, MenuItem>>()
  private counter = 0
  private writer: AtomicWriter | null = null
  /** Items read from disk, until their extension loads. */
  private saved = new Map<string, MenuItem[]>()
  /** `extensionId/id` of items that exist only because they were restored: a `create` with that id replaces them. */
  private restored = new Set<string>()

  constructor(private emitter: Pick<ExtensionEmitter, 'emit'>) {}

  /** Keeps the items in `file` (Chrome keeps them across restarts). Call once, when `userData` is available. */
  persist(file: string): void {
    try {
      const data = JSON.parse(readFileSync(file, 'utf8')) as Record<string, MenuItem[]>
      for (const [id, items] of Object.entries(data))
        if (Array.isArray(items) && items.length)
          this.saved.set(
            id,
            items.filter((i) => i && i.id !== undefined)
          )
    } catch {
      // no file yet
    }
    this.writer = new AtomicWriter(file, () =>
      JSON.stringify(Object.fromEntries([...this.items].filter(([, m]) => m.size).map(([id, m]) => [id, [...m.values()]])))
    )
  }

  /** The extension loaded: its saved items come back (those it already created this run win). */
  restore(extensionId: string): void {
    const saved = this.saved.get(extensionId)
    this.saved.delete(extensionId)
    if (!saved?.length) return
    const items = this.of(extensionId)
    for (const item of saved) {
      if (items.has(String(item.id))) continue
      items.set(String(item.id), item)
      this.restored.add(`${extensionId}/${item.id}`)
    }
  }

  flush(): void {
    this.writer?.flushSync()
  }

  private changed(): void {
    this.writer?.request()
  }

  private of(extensionId: string): Map<string, MenuItem> {
    let m = this.items.get(extensionId)
    if (!m) this.items.set(extensionId, (m = new Map()))
    return m
  }

  create(extensionId: string, raw: unknown): string | number {
    const p = raw !== null && typeof raw === 'object' ? (raw as Record<string, unknown>) : {}
    const id = typeof p.id === 'string' || typeof p.id === 'number' ? p.id : `signet-menu-${++this.counter}`
    const items = this.of(extensionId)
    const restoredKey = `${extensionId}/${id}`
    if (items.has(String(id))) {
      // An id that only came back from disk is replaced (the extension recreates its items at startup); one created this run is a real duplicate.
      if (!this.restored.has(restoredKey)) throw new ApiError(`Cannot create item with duplicate id ${id}`)
      this.restored.delete(restoredKey)
      items.delete(String(id))
    }
    if (p.parentId !== undefined && !items.has(String(p.parentId))) throw new ApiError(`Cannot find menu item with id ${String(p.parentId)}`)
    const base: MenuItem = { id, type: 'normal', title: '', checked: false, enabled: true, visible: true, contexts: ['page'] }
    items.set(String(id), mergeProps(base, p))
    this.changed()
    return id
  }

  update(extensionId: string, id: unknown, props: unknown): void {
    const items = this.of(extensionId)
    const item = items.get(String(id))
    if (!item) throw new ApiError(`Cannot find menu item with id ${String(id)}`)
    const p = props !== null && typeof props === 'object' ? (props as Record<string, unknown>) : {}
    if (
      p.parentId !== undefined &&
      (String(p.parentId) === String(id) || !items.has(String(p.parentId)) || this.descendants(extensionId, String(id)).has(String(p.parentId)))
    )
      throw new ApiError(`Cannot find menu item with id ${String(p.parentId)}`)
    items.set(String(id), mergeProps(item, p))
    this.changed()
  }

  private descendants(extensionId: string, id: string): Set<string> {
    const out = new Set<string>()
    const walk = (parent: string): void => {
      for (const [key, i] of this.of(extensionId)) {
        if (i.parentId === undefined || String(i.parentId) !== parent || out.has(key)) continue
        out.add(key)
        walk(key)
      }
    }
    walk(id)
    return out
  }

  remove(extensionId: string, id: unknown): void {
    const items = this.of(extensionId)
    if (!items.has(String(id))) throw new ApiError(`Cannot find menu item with id ${String(id)}`)
    for (const key of this.descendants(extensionId, String(id))) items.delete(key)
    items.delete(String(id))
    this.changed()
  }

  removeAll(extensionId: string): void {
    this.items.delete(extensionId)
    this.changed()
  }

  has(extensionId: string): boolean {
    return (this.items.get(extensionId)?.size ?? 0) > 0
  }

  /** The tree to show for an extension: matching items, a child only under a parent that is shown. */
  tree(extensionId: string, contexts: ReadonlySet<string>, target?: MenuTarget): MenuNode[] {
    const all = this.items.get(extensionId)
    if (!all) return []
    const shown = new Map<string, MenuNode>()
    for (const [key, item] of all)
      if (itemMatches(item, contexts, target)) shown.set(key, { item, label: labelFor(item.title, target?.selectionText), children: [] })
    const roots: MenuNode[] = []
    for (const node of shown.values()) {
      const parentId = node.item.parentId
      if (parentId === undefined) roots.push(node)
      else shown.get(String(parentId))?.children.push(node)
    }
    return roots
  }

  /** A click: flips a checkbox, selects a radio (and clears its neighbours), and reports the state before and after. */
  click(extensionId: string, id: string | number): ClickInfo {
    const items = this.of(extensionId)
    const item = items.get(String(id))
    const info: ClickInfo = { menuItemId: id, ...(item?.parentId !== undefined && { parentMenuItemId: item.parentId }) }
    if (!item) return info
    if (item.type === 'checkbox') {
      info.wasChecked = item.checked
      item.checked = !item.checked
      info.checked = item.checked
    } else if (item.type === 'radio') {
      info.wasChecked = item.checked
      for (const other of items.values()) if (other.type === 'radio' && String(other.parentId) === String(item.parentId)) other.checked = false
      item.checked = true
      info.checked = true
    }
    return info
  }

  /** Tells the extension an item was clicked (wakes its worker). */
  clicked(extensionId: string, info: Record<string, unknown>, tab: unknown): void {
    this.emitter.emit(extensionId, MENU_CLICK_EVENT, tab ? [info, tab] : [info], true)
  }

  /** The extension is uninstalled: its items go, on disk too. */
  unload(extensionId: string): void {
    this.items.delete(extensionId)
    this.saved.delete(extensionId)
    this.changed()
  }

  methods(): Record<string, ApiHandler> {
    return {
      'contextMenus.create': ({ extensionId }: ApiCaller, props) => this.create(extensionId, props),
      'contextMenus.update': ({ extensionId }, id, props) => this.update(extensionId, id, props),
      'contextMenus.remove': ({ extensionId }, id) => this.remove(extensionId, id),
      'contextMenus.removeAll': ({ extensionId }) => this.removeAll(extensionId)
    }
  }
}

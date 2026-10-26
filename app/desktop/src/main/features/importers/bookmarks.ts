// Favorites from Chrome, Brave, Edge (JSON) and Safari (plist converted to JSON) -> imported folder.
import { isWebUrl, pruneEmpty, type ImportedNode } from './types'
import { t } from '../../app/i18n'

type Obj = Record<string, unknown>
const isObj = (v: unknown): v is Obj => !!v && typeof v === 'object' && !Array.isArray(v)
const MAX_DEPTH = 20

function chromeNode(n: unknown, depth: number): ImportedNode[] {
  if (!isObj(n) || depth > MAX_DEPTH) return []
  if (n.type === 'url') {
    if (!isWebUrl(n.url)) return []
    const url = n.url.trim()
    return [{ kind: 'tab', url, title: typeof n.name === 'string' && n.name.trim() ? n.name.trim() : url }]
  }
  if (n.type === 'folder' || Array.isArray(n.children)) {
    const items = (Array.isArray(n.children) ? n.children : []).flatMap((c) => chromeNode(c, depth + 1))
    const name = typeof n.name === 'string' && n.name.trim() ? n.name.trim() : t('defaults.folderFallback')
    return [{ kind: 'folder', name, items }]
  }
  return []
}

/** Content of the `Bookmarks` file -> folder named after the browser (or `null` if nothing to import). */
export function parseChromeBookmarks(json: unknown, browserName: string): ImportedNode | null {
  const roots = isObj(json) && isObj(json.roots) ? json.roots : null
  if (!roots) throw new Error(t('import.error.format'))
  const content = (key: string) => {
    const r = roots[key]
    return isObj(r) && Array.isArray(r.children) ? pruneEmpty(r.children.flatMap((c) => chromeNode(c, 0))) : []
  }
  const items = content('bookmark_bar')
  for (const [key, name] of [
    ['other', t('import.folder.otherBookmarks')],
    ['synced', t('import.folder.syncedBookmarks')]
  ] as const) {
    const sub = content(key)
    if (sub.length) items.push({ kind: 'folder', name, items: sub })
  }
  return items.length ? { kind: 'folder', name: browserName, items } : null
}

function safariNode(n: unknown, depth: number): ImportedNode[] {
  if (!isObj(n) || depth > MAX_DEPTH) return []
  const title = typeof n.Title === 'string' ? n.Title : ''
  if (n.WebBookmarkType === 'WebBookmarkTypeLeaf') {
    if (!isWebUrl(n.URLString)) return []
    const url = n.URLString.trim()
    const dict = isObj(n.URIDictionary) ? n.URIDictionary : {}
    const t = [dict.title, title].find((x): x is string => typeof x === 'string' && !!x.trim())
    return [{ kind: 'tab', url, title: t?.trim() ?? url }]
  }
  if (n.WebBookmarkType === 'WebBookmarkTypeList') {
    if (title === 'com.apple.ReadingList' || title === 'History' || n.WebBookmarkIdentifier === 'History') return []
    const items = (Array.isArray(n.Children) ? n.Children : []).flatMap((c) => safariNode(c, depth + 1))
    return [{ kind: 'folder', name: title || t('defaults.folderFallback'), items }]
  }
  return []
}

/** Safari plist (already converted to JSON) -> "Safari" folder. */
export function parseSafariBookmarks(json: unknown): ImportedNode | null {
  if (!isObj(json) || !Array.isArray(json.Children)) throw new Error(t('import.error.format'))
  const children: unknown[] = json.Children
  const top = (title: string) => {
    const list = children.find((c) => isObj(c) && c.Title === title)
    return isObj(list) ? pruneEmpty((Array.isArray(list.Children) ? list.Children : []).flatMap((c) => safariNode(c, 0))) : []
  }
  const items = top('BookmarksBar')
  const menu = top('BookmarksMenu')
  if (menu.length) items.push({ kind: 'folder', name: t('import.folder.bookmarksMenu'), items: menu })
  return items.length ? { kind: 'folder', name: 'Safari', items } : null
}

/** XML plist -> JS value. Fallback when `plutil -convert json` rejects the file (binary data, dates). */
export function parseXmlPlist(xml: string): unknown {
  const tokens = [...xml.matchAll(/<(\/?)([A-Za-z]+)\s*(\/?)>|([^<]+)/g)]
  let pos = 0
  const decode = (s: string) =>
    s
      .replace(/&lt;/g, '<')
      .replace(/&gt;/g, '>')
      .replace(/&quot;/g, '"')
      .replace(/&apos;/g, "'")
      .replace(/&amp;/g, '&')
  const text = (): string => {
    const t = tokens[pos]
    if (t && t[4] !== undefined) {
      pos++
      return decode(t[4])
    }
    return ''
  }
  const value = (depth: number): unknown => {
    while (pos < tokens.length && (tokens[pos][4] !== undefined || tokens[pos][2] === 'plist' || tokens[pos][2] === '?xml')) pos++
    const t = tokens[pos++]
    if (!t || depth > 60) return null
    const [, close, tag, self] = t
    if (close) return null
    if (tag === 'dict' || tag === 'array') {
      const isDict = tag === 'dict'
      const obj: Obj = {}
      const arr: unknown[] = []
      if (self) return isDict ? obj : arr
      let key = ''
      while (pos < tokens.length) {
        const n = tokens[pos]
        if (n[4] !== undefined) {
          pos++
          continue
        }
        if (n[1] && n[2] === tag) {
          pos++
          break
        }
        if (isDict && n[2] === 'key') {
          pos++
          key = text()
          pos++ // </key>
          continue
        }
        const v = value(depth + 1)
        if (isDict) obj[key] = v
        else arr.push(v)
      }
      return isDict ? obj : arr
    }
    if (tag === 'true') return true
    if (tag === 'false') return false
    if (self) return ''
    const content = text()
    if (tokens[pos]?.[1] && tokens[pos][2] === tag) pos++
    if (tag === 'integer' || tag === 'real') return Number(content)
    return content
  }
  return value(0)
}

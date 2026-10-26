// Bookmarks import / export in the "Netscape Bookmark File" format (HTML), read by all browsers.
import { newId } from '../ids'
import { MAX_BOOKMARK_DEPTH, type BookmarkFolder, type BookmarkLink } from './bookmarks'
import type { Bookmark } from '../types'

const escapeHtml = (s: string) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;')

const NAMED: Record<string, string> = { amp: '&', lt: '<', gt: '>', quot: '"', apos: "'", nbsp: ' ' }
function unescapeHtml(s: string): string {
  return s.replace(/&(#x[0-9a-f]+|#\d+|[a-z]+);/gi, (m, code: string) => {
    if (code[0] === '#') {
      const n = code[1].toLowerCase() === 'x' ? parseInt(code.slice(2), 16) : parseInt(code.slice(1), 10)
      return Number.isFinite(n) && n > 0 && n <= 0x10ffff ? String.fromCodePoint(n) : m
    }
    return NAMED[code.toLowerCase()] ?? m
  })
}

/** Only web addresses are exported and imported (no `javascript:` slipped into a file). */
const isWebUrl = (url: string) => /^https?:\/\/[^\s/]+/i.test(url)

/** Bookmarks -> HTML file. `now` (ms) is used as the added date. */
export function exportBookmarksHtml(tree: readonly Bookmark[], now = Date.now()): string {
  const stamp = Math.floor(now / 1000)
  const lines: string[] = []
  const walk = (nodes: readonly Bookmark[], indent: string) => {
    for (const n of nodes) {
      if (n.kind === 'folder') {
        lines.push(`${indent}<DT><H3 ADD_DATE="${stamp}">${escapeHtml(n.name)}</H3>`, `${indent}<DL><p>`)
        walk(n.items, indent + '    ')
        lines.push(`${indent}</DL><p>`)
      } else if (isWebUrl(n.url)) {
        lines.push(`${indent}<DT><A HREF="${escapeHtml(n.url)}" ADD_DATE="${stamp}">${escapeHtml(n.title || n.url)}</A>`)
      }
    }
  }
  walk(tree, '    ')
  return [
    '<!DOCTYPE NETSCAPE-Bookmark-file-1>',
    '<!-- This is an automatically generated file. It will be read and overwritten. DO NOT EDIT! -->',
    '<META HTTP-EQUIV="Content-Type" CONTENT="text/html; charset=UTF-8">',
    '<TITLE>Bookmarks</TITLE>',
    '<H1>Bookmarks</H1>',
    '<DL><p>',
    ...lines,
    '</DL><p>',
    ''
  ].join('\n')
}

/** HTML bookmarks file -> tree (new identifiers). Tolerant: unknown tags ignored, non-web links dropped, nesting limited. */
export function parseBookmarksHtml(html: string): Bookmark[] {
  const root: Bookmark[] = []
  // Stack of open lists; `null` = list without a folder (the file root), which adds no level.
  const stack: (BookmarkFolder | null)[] = []
  const current = (): Bookmark[] => {
    for (let i = stack.length - 1; i >= 0; i--) {
      const f = stack[i]
      if (f) return f.items
    }
    return root
  }
  let depth = 0
  let pending: BookmarkFolder | null = null
  const tag = /<(\/?)(dl|h3|a)\b([^>]*)>([^<]*)/gi
  for (let m = tag.exec(html); m; m = tag.exec(html)) {
    const closing = m[1] === '/'
    const name = m[2].toLowerCase()
    if (name === 'dl') {
      if (closing) {
        const top = stack.pop()
        if (top) depth--
      } else if (pending) {
        stack.push(pending)
        pending = null
        depth++
      } else stack.push(null)
    } else if (name === 'h3' && !closing) {
      const folder: BookmarkFolder = { kind: 'folder', id: newId(), name: unescapeHtml(m[4]).trim() || '…', items: [] }
      // Beyond the allowed depth, the content ends up in the parent folder.
      if (depth + 1 < MAX_BOOKMARK_DEPTH) {
        current().push(folder)
        pending = folder
      } else pending = null
    } else if (name === 'a' && !closing) {
      const href = /\bhref\s*=\s*(?:"([^"]*)"|'([^']*)')/i.exec(m[3])
      const url = href ? unescapeHtml(href[1] ?? href[2]).trim() : ''
      if (!isWebUrl(url)) continue
      const link: BookmarkLink = { kind: 'link', id: newId(), title: unescapeHtml(m[4]).replace(/\s+/g, ' ').trim() || url, url }
      current().push(link)
    }
  }
  return root
}

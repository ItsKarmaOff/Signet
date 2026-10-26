// Long-press menu on a link: the injected script (see app/mobile/src/features/browser/scripts.ts) sends
// `{ type: 'linkMenu', href, text, x, y }`; these pure functions validate the message and prepare what the menu shows.
import { siteOf } from '../site'

export interface LinkMenuMessage {
  type: 'linkMenu'
  /** Absolute address of the link (http or https). */
  href: string
  /** Text of the link (or description of its image), empty if there is none. */
  text: string
  x: number
  y: number
}

/** Only web links open the menu: `javascript:`, `mailto:`, `data:`, etc. are ignored. */
export const isMenuUrl = (url: string): boolean => /^https?:\/\/[^\s/]+/i.test(url.trim())

/** Resolves a (possibly relative) link against the page; `null` if the result is not a web address. */
export function resolveHref(href: string, base?: string): string | null {
  const raw = href.trim()
  if (!raw || /^javascript:/i.test(raw)) return null
  try {
    const url = base ? new URL(raw, base) : new URL(raw)
    return isMenuUrl(url.href) ? url.href : null
  } catch {
    return null
  }
}

/** Reads a WebView message; `null` if it is not a valid long-press on a web link. */
export function parseLinkMenuMessage(data: string): LinkMenuMessage | null {
  try {
    const m = JSON.parse(data) as Partial<LinkMenuMessage> | null
    if (!m || m.type !== 'linkMenu' || typeof m.href !== 'string') return null
    const href = resolveHref(m.href)
    if (!href) return null
    const text = typeof m.text === 'string' ? m.text.replace(/\s+/g, ' ').trim().slice(0, 200) : ''
    const x = typeof m.x === 'number' && Number.isFinite(m.x) ? m.x : 0
    const y = typeof m.y === 'number' && Number.isFinite(m.y) ? m.y : 0
    return { type: 'linkMenu', href, text, x, y }
  } catch {
    return null
  }
}

/** Menu header title: the link text, otherwise the site. */
export const linkMenuTitle = (link: Pick<LinkMenuMessage, 'href' | 'text'>): string => link.text || siteOf(link.href) || link.href

/** Shortens an address in the middle to fit in the header (`https://example.com/…/end`). */
export function truncateMiddle(text: string, max = 80): string {
  if (text.length <= max || max < 8) return text
  const keep = max - 1
  const head = Math.ceil(keep * 0.65)
  return `${text.slice(0, head)}…${text.slice(text.length - (keep - head))}`
}

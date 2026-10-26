// Note and Easel text HTML: cleaned on display and on paste (text tags only, http(s) links).
// The page has access to its documents: no script, no event handler, no javascript: address gets in.

/** Removed along with their content. */
const DROP = new Set([
  'SCRIPT',
  'STYLE',
  'IFRAME',
  'FRAME',
  'OBJECT',
  'EMBED',
  'TEMPLATE',
  'NOSCRIPT',
  'SVG',
  'MATH',
  'FORM',
  'INPUT',
  'BUTTON',
  'SELECT',
  'TEXTAREA',
  'LINK',
  'META',
  'BASE',
  'VIDEO',
  'AUDIO',
  'CANVAS',
  'HEAD',
  'TITLE'
])
/** Kept; other elements are replaced by their content. */
const KEEP = new Set([
  'P',
  'DIV',
  'BR',
  'H1',
  'H2',
  'H3',
  'B',
  'STRONG',
  'I',
  'EM',
  'U',
  'S',
  'STRIKE',
  'DEL',
  'CODE',
  'PRE',
  'BLOCKQUOTE',
  'UL',
  'OL',
  'LI',
  'A',
  'IMG',
  'SPAN',
  'HR'
])

const safeHref = (v: string) => /^(https?:|mailto:)/i.test(v.trim())
const safeSrc = (v: string) => /^(https?:|data:image\/(png|jpeg|webp|gif);base64,)/i.test(v.trim())

function cleanAttrs(el: Element): void {
  for (const attr of [...el.attributes]) {
    const name = attr.name.toLowerCase()
    const keep =
      (el.tagName === 'A' && name === 'href' && safeHref(attr.value)) ||
      (el.tagName === 'IMG' && name === 'src' && safeSrc(attr.value)) ||
      (el.tagName === 'LI' && name === 'data-checked' && (attr.value === 'true' || attr.value === 'false')) ||
      (el.tagName === 'UL' && name === 'class' && attr.value === 'checklist')
    if (!keep) el.removeAttribute(attr.name)
  }
}

function walk(parent: ParentNode): void {
  for (const node of [...parent.childNodes]) {
    if (node.nodeType === Node.COMMENT_NODE) {
      node.remove()
      continue
    }
    if (node.nodeType !== Node.ELEMENT_NODE) continue
    const el = node as Element
    if (DROP.has(el.tagName)) {
      el.remove()
      continue
    }
    walk(el)
    if (KEEP.has(el.tagName)) cleanAttrs(el)
    else el.replaceWith(...el.childNodes)
  }
}

/** Cleaned HTML (in an inert template: nothing loads or runs during parsing). */
export function cleanHtml(html: string): string {
  const tpl = document.createElement('template')
  tpl.innerHTML = html
  walk(tpl.content)
  return tpl.innerHTML
}

/** First non-empty line of an element, for the document title. */
export function firstLine(el: HTMLElement, max = 80): string {
  const line = (el.innerText || '').split('\n').find((l) => l.trim()) ?? ''
  const text = line.trim()
  return text.length > max ? `${text.slice(0, max - 1)}…` : text
}

/** Tab icon (favicon) of a document: pictogram drawn as SVG, in a data URL. */
export function docIcon(kind: 'note' | 'easel', hue: number): string {
  const color = `hsl(${hue} 60% 52%)`
  const glyph =
    kind === 'note'
      ? `<path d="M9 7h14M9 13h14M9 19h9" stroke="white" stroke-width="2.6" stroke-linecap="round"/>`
      : `<path d="M8 22c3-6 6 0 9-6s5-4 7-6" fill="none" stroke="white" stroke-width="2.6" stroke-linecap="round"/><circle cx="23" cy="9" r="2" fill="white"/>`
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32"><rect x="2" y="2" width="28" height="28" rx="8" fill="${color}"/>${glyph}</svg>`
  return `data:image/svg+xml,${encodeURIComponent(svg)}`
}

/** Sets the page title and icon: the sidebar tab picks them up. */
export function setTabLook(title: string, icon: string): void {
  document.title = title
  let link = document.querySelector<HTMLLinkElement>('link[rel="icon"]')
  if (!link) {
    link = document.createElement('link')
    link.rel = 'icon'
    document.head.append(link)
  }
  if (link.href !== icon) link.href = icon
}

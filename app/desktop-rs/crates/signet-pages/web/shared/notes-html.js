// Note and Easel text HTML: cleaned on display and on paste (text tags only, http(s) links).
// The page has access to its documents: no script, no event handler, no javascript: address gets in.
// Classic script: exposes `SignetNotesHtml` on the page, and through module.exports for node tests.
;(function (root) {
  'use strict'

  /** Removed along with their content. */
  const DROP = new Set([
    'SCRIPT', 'STYLE', 'IFRAME', 'FRAME', 'OBJECT', 'EMBED', 'TEMPLATE', 'NOSCRIPT', 'SVG', 'MATH', 'FORM', 'INPUT',
    'BUTTON', 'SELECT', 'TEXTAREA', 'LINK', 'META', 'BASE', 'VIDEO', 'AUDIO', 'CANVAS', 'HEAD', 'TITLE'
  ])
  /** Kept; other elements are replaced by their content. */
  const KEEP = new Set([
    'P', 'DIV', 'BR', 'H1', 'H2', 'H3', 'B', 'STRONG', 'I', 'EM', 'U', 'S', 'STRIKE', 'DEL', 'CODE', 'PRE', 'BLOCKQUOTE',
    'UL', 'OL', 'LI', 'A', 'IMG', 'SPAN', 'HR'
  ])

  const safeHref = (v) => /^(https?:|mailto:)/i.test(String(v).trim())
  const safeSrc = (v) => /^(https?:|data:image\/(png|jpeg|webp|gif);base64,)/i.test(String(v).trim())

  /** Is this attribute kept on this (upper case) tag? Everything else is removed. */
  function attrAllowed(tag, name, value) {
    name = String(name).toLowerCase()
    return (
      (tag === 'A' && name === 'href' && safeHref(value)) ||
      (tag === 'IMG' && name === 'src' && safeSrc(value)) ||
      (tag === 'LI' && name === 'data-checked' && (value === 'true' || value === 'false')) ||
      (tag === 'UL' && name === 'class' && value === 'checklist')
    )
  }

  function cleanAttrs(el) {
    for (const attr of [...el.attributes]) if (!attrAllowed(el.tagName, attr.name, attr.value)) el.removeAttribute(attr.name)
  }

  function walk(parent) {
    for (const node of [...parent.childNodes]) {
      if (node.nodeType === 8) {
        node.remove()
        continue
      }
      if (node.nodeType !== 1) continue
      if (DROP.has(node.tagName)) {
        node.remove()
        continue
      }
      walk(node)
      if (KEEP.has(node.tagName)) cleanAttrs(node)
      else node.replaceWith(...node.childNodes)
    }
  }

  /** Cleaned HTML (in an inert template: nothing loads or runs during parsing). */
  function cleanHtml(html) {
    const tpl = document.createElement('template')
    tpl.innerHTML = html
    walk(tpl.content)
    return tpl.innerHTML
  }

  /** First non-empty line of a text, cut at `max` characters. */
  function firstLineOf(text, max = 80) {
    const line = String(text || '').split('\n').find((l) => l.trim()) ?? ''
    const t = line.trim()
    return t.length > max ? `${t.slice(0, max - 1)}…` : t
  }

  /** First non-empty line of an element, for the document title. */
  function firstLine(el, max = 80) {
    return firstLineOf(el.innerText, max)
  }

  /** Tab icon (favicon) of a document: pictogram drawn as SVG, in a data URL. */
  function docIcon(kind, hue) {
    const color = `hsl(${hue} 60% 52%)`
    const glyph =
      kind === 'note'
        ? '<path d="M9 7h14M9 13h14M9 19h9" stroke="white" stroke-width="2.6" stroke-linecap="round"/>'
        : '<path d="M8 22c3-6 6 0 9-6s5-4 7-6" fill="none" stroke="white" stroke-width="2.6" stroke-linecap="round"/><circle cx="23" cy="9" r="2" fill="white"/>'
    const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32"><rect x="2" y="2" width="28" height="28" rx="8" fill="${color}"/>${glyph}</svg>`
    return `data:image/svg+xml,${encodeURIComponent(svg)}`
  }

  /** Sets the page title and icon: the sidebar tab picks them up. */
  function setTabLook(title, icon) {
    document.title = title
    let link = document.querySelector('link[rel="icon"]')
    if (!link) {
      link = document.createElement('link')
      link.rel = 'icon'
      document.head.append(link)
    }
    if (link.href !== icon) link.href = icon
  }

  const api = { DROP, KEEP, safeHref, safeSrc, attrAllowed, cleanHtml, firstLineOf, firstLine, docIcon, setTabLook }
  root.SignetNotesHtml = api
  if (typeof module !== 'undefined' && module.exports) module.exports = api
})(typeof window !== 'undefined' ? window : globalThis)

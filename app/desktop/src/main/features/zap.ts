/**
 * Script of the "Hide" tool, run in an isolated world (`executeJavaScriptInIsolatedWorld`).
 * Evaluated as an expression: it returns a promise resolved with a CSS selector,
 * or `null` on Esc / `window.__signetZapStop()`.
 * (String.raw: the backslashes of regular expressions stay intact. No backtick or dollar-brace inside.)
 */
export const ZAP_SCRIPT = String.raw`
new Promise((resolve) => {
  if (typeof window.__signetZapStop === 'function') window.__signetZapStop()

  const doc = document
  const root = doc.documentElement
  const host = doc.createElement('div')
  host.setAttribute('data-signet-zap', '')
  host.style.cssText = 'all:initial;position:fixed;inset:0;z-index:2147483647;pointer-events:none;'
  const box = doc.createElement('div')
  box.style.cssText =
    'position:fixed;display:none;pointer-events:none;box-sizing:border-box;' +
    'outline:2px solid #7c5cff;outline-offset:-2px;background:rgba(124,92,255,0.16);border-radius:3px;' +
    'transition:left 60ms,top 60ms,width 60ms,height 60ms;'
  const tag = doc.createElement('div')
  tag.style.cssText =
    'position:fixed;display:none;pointer-events:none;padding:2px 6px;border-radius:4px;' +
    'font:600 11px/1.3 -apple-system,system-ui,sans-serif;color:#fff;background:#7c5cff;white-space:nowrap;'
  const cursor = doc.createElement('style')
  cursor.textContent = '* { cursor: crosshair !important; }'
  host.append(box, tag)
  root.append(host, cursor)

  let current = null
  let done = false

  const isOurs = (el) => !el || el === host || host.contains(el)
  const isRoot = (el) => el === doc.documentElement || el === doc.body

  const place = (el) => {
    if (!el || isRoot(el)) {
      box.style.display = 'none'
      tag.style.display = 'none'
      return
    }
    const r = el.getBoundingClientRect()
    box.style.display = 'block'
    box.style.left = r.left + 'px'
    box.style.top = r.top + 'px'
    box.style.width = r.width + 'px'
    box.style.height = r.height + 'px'
    tag.textContent = el.tagName.toLowerCase()
    tag.style.display = 'block'
    tag.style.left = Math.max(0, r.left) + 'px'
    tag.style.top = (r.top > 22 ? r.top - 21 : r.top + 3) + 'px'
  }

  const esc = (s) => (window.CSS && CSS.escape ? CSS.escape(s) : s.replace(/[^\w-]/g, '\\$&'))
  const count = (sel) => {
    try {
      return doc.querySelectorAll(sel).length
    } catch (e) {
      return -1
    }
  }
  const stableClass = (c) => c.length <= 30 && /^[A-Za-z_-][A-Za-z_-]*$/.test(c) && !/^(css|sc|jsx|svelte|styled|emotion)-/i.test(c)
  const goodId = (el) => el.id && !/\d{4,}/.test(el.id) && count('#' + esc(el.id)) === 1

  const part = (el) => {
    if (goodId(el)) return { text: '#' + esc(el.id), anchored: true }
    let text = el.tagName.toLowerCase()
    const classes = Array.from(el.classList).filter(stableClass).slice(0, 2)
    for (const c of classes) text += '.' + esc(c)
    const parent = el.parentElement
    if (parent) {
      let same = 0
      for (const sibling of parent.children) if (sibling.matches(text)) same++
      if (same > 1) text += ':nth-of-type(' + (Array.from(parent.children).filter((s) => s.tagName === el.tagName).indexOf(el) + 1) + ')'
    }
    return { text, anchored: false }
  }

  const build = (el) => {
    const parts = []
    let node = el
    for (let depth = 0; node && node.nodeType === 1 && depth < 6 && !isRoot(node); depth++) {
      const p = part(node)
      parts.unshift(p.text)
      const sel = parts.join(' > ')
      if (p.anchored || count(sel) === 1) return sel
      node = node.parentElement
    }
    return parts.join(' > ')
  }

  const cleanup = () => {
    if (done) return
    done = true
    window.removeEventListener('mouseover', onMove, true)
    window.removeEventListener('mousemove', onMove, true)
    window.removeEventListener('scroll', onScroll, true)
    for (const type of ['click', 'mousedown', 'mouseup', 'pointerdown', 'pointerup', 'auxclick', 'contextmenu', 'dblclick']) {
      window.removeEventListener(type, onClick, true)
    }
    window.removeEventListener('keydown', onKey, true)
    host.remove()
    cursor.remove()
    if (window.__signetZapStop === stop) delete window.__signetZapStop
  }

  const stop = () => {
    cleanup()
    resolve(null)
  }

  function onMove(e) {
    const el = e.target
    if (isOurs(el) || el === current) return
    current = el
    place(el)
  }
  function onScroll() {
    if (current) place(current)
  }
  function onClick(e) {
    e.preventDefault()
    e.stopPropagation()
    e.stopImmediatePropagation()
    if (e.type !== 'click') return
    const el = isOurs(e.target) || isRoot(e.target) ? null : e.target
    if (!el) return
    const selector = build(el)
    cleanup()
    resolve(selector)
  }
  function onKey(e) {
    if (e.key === 'Escape') {
      e.preventDefault()
      e.stopPropagation()
      stop()
    }
  }

  window.addEventListener('mouseover', onMove, true)
  window.addEventListener('mousemove', onMove, true)
  window.addEventListener('scroll', onScroll, true)
  for (const type of ['click', 'mousedown', 'mouseup', 'pointerdown', 'pointerup', 'auxclick', 'contextmenu', 'dblclick']) {
    window.addEventListener(type, onClick, true)
  }
  window.addEventListener('keydown', onKey, true)
  window.__signetZapStop = stop
})
`

/** Stops the tool in the page (no effect if it is not active). */
export const ZAP_STOP_SCRIPT = '(typeof window.__signetZapStop === "function" && window.__signetZapStop(), null)'

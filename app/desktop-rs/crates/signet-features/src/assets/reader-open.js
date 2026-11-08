(() => {
  if (window.__signetReader) return 'already'
  __READABILITY__
  const PREFS = __PREFS__
  const L = __LABELS__
  let article = null
  try { article = new Readability(document.cloneNode(true), { charThreshold: 300 }).parse() } catch {}
  if (!article || !article.content) return 'none'

  // Cleanup: no script, no event handler, no javascript: address, no form.
  const tpl = document.createElement('template')
  tpl.innerHTML = article.content
  tpl.content.querySelectorAll('script, style, link, meta, base, iframe, frame, object, embed, form, input, button, textarea, select, noscript, template').forEach((el) => el.remove())
  tpl.content.querySelectorAll('*').forEach((el) => {
    for (const attr of [...el.attributes]) {
      const name = attr.name.toLowerCase()
      if (name.startsWith('on') || name === 'style' || name === 'class' || name === 'width' || name === 'height') el.removeAttribute(attr.name)
      else if (/^(href|src|xlink:href|action|formaction|poster)$/.test(name) && /^\s*(javascript|vbscript|data:text\/html)/i.test(attr.value)) el.removeAttribute(attr.name)
    }
  })

  const host = document.createElement('signet-reader')
  host.style.cssText = 'all: initial; position: fixed; inset: 0; z-index: 2147483647; display: block'
  const shadow = host.attachShadow({ mode: 'closed' })
  const style = document.createElement('style')
  style.textContent = __CSS__
  const root = document.createElement('div')
  root.className = 'root'
  root.setAttribute('role', 'document')
  const prefs = { ...PREFS }
  const apply = () => {
    root.dataset.theme = prefs.theme
    root.dataset.font = prefs.font
    root.dataset.width = prefs.width
    root.style.setProperty('--size', prefs.size + 'px')
    shadow.querySelectorAll('[data-theme-choice]').forEach((b) => b.setAttribute('aria-pressed', String(b.dataset.themeChoice === prefs.theme)))
    shadow.querySelectorAll('[data-font-choice]').forEach((b) => b.setAttribute('aria-pressed', String(b.dataset.fontChoice === prefs.font)))
    shadow.querySelectorAll('[data-width-choice]').forEach((b) => b.setAttribute('aria-pressed', String(b.dataset.widthChoice === prefs.width)))
  }

  const el = (tag, props, ...children) => {
    const node = document.createElement(tag)
    Object.assign(node, props)
    for (const c of children) node.append(c)
    return node
  }
  const button = (label, title, data, extra) => {
    const b = el('button', { type: 'button', title }, ...(Array.isArray(label) ? label : [label]))
    for (const [k, v] of Object.entries(data || {})) b.dataset[k] = v
    if (extra) b.className = extra
    return b
  }
  const dot = (color) => {
    const d = el('span', { className: 'dot' })
    d.style.background = color
    return d
  }
  const bar = el('div', { className: 'bar' },
    el('div', { className: 'group' },
      button(el('span', { className: 'small', textContent: 'A' }), L.smaller, { act: 'smaller' }),
      button(el('span', { className: 'big', textContent: 'A' }), L.bigger, { act: 'bigger' })),
    el('div', { className: 'group' },
      button(L.serif, L.serif, { fontChoice: 'serif' }),
      button(L.sans, L.sans, { fontChoice: 'sans' })),
    el('div', { className: 'group' },
      button(dot('linear-gradient(135deg, #fff 50%, #1c1c1e 50%)'), L.themeAuto, { themeChoice: 'auto' }),
      button(dot('#fff'), L.themeLight, { themeChoice: 'light' }),
      button(dot('#f6eedd'), L.themeSepia, { themeChoice: 'sepia' }),
      button(dot('#1c1c1e'), L.themeDark, { themeChoice: 'dark' })),
    el('div', { className: 'group' },
      button(L.narrow, L.narrowTitle, { widthChoice: 'narrow' }),
      button(L.normal, L.normalTitle, { widthChoice: 'normal' }),
      button(L.wide, L.wideTitle, { widthChoice: 'wide' })),
    button(L.close, L.closeTitle, { act: 'close' }, 'close'))

  const minutes = Math.max(1, Math.round((article.length || 0) / 1200))
  const meta = [article.byline, L.minutes.replace('{n}', String(minutes))].filter(Boolean).join(' · ')
  const content = el('div', { className: 'content' })
  content.append(document.importNode(tpl.content, true))
  const body = el('article', {},
    ...(article.siteName ? [el('div', { className: 'site', textContent: article.siteName })] : []),
    el('h1', { className: 'title', textContent: article.title || document.title }),
    el('div', { className: 'meta', textContent: meta }),
    content)
  if (article.lang) body.lang = article.lang
  if (article.dir) body.dir = article.dir
  root.append(bar, body)
  shadow.append(style, root)

  // Messages to main (preferences, closing), read by successive calls to next().
  let waiting = null
  const pending = []
  const send = (msg) => {
    if (waiting) {
      const w = waiting
      waiting = null
      w(msg)
    } else {
      if (msg.type === 'prefs') for (let i = pending.length - 1; i >= 0; i--) if (pending[i].type === 'prefs') pending.splice(i, 1)
      pending.push(msg)
    }
  }
  const changed = () => {
    apply()
    send({ type: 'prefs', prefs: { ...prefs } })
  }
  bar.addEventListener('click', (e) => {
    const b = e.target.closest('button')
    if (!b) return
    if (b.dataset.act === 'smaller') prefs.size = Math.max(14, prefs.size - 1)
    else if (b.dataset.act === 'bigger') prefs.size = Math.min(28, prefs.size + 1)
    else if (b.dataset.act === 'close') return close()
    else if (b.dataset.fontChoice) prefs.font = b.dataset.fontChoice
    else if (b.dataset.themeChoice) prefs.theme = b.dataset.themeChoice
    else if (b.dataset.widthChoice) prefs.width = b.dataset.widthChoice
    changed()
  })
  // Anchor links (notes, table of contents): scroll within the view instead of letting the page navigate.
  content.addEventListener('click', (e) => {
    const a = e.target.closest ? e.target.closest('a[href]') : null
    if (!a) return
    let u
    try { u = new URL(a.getAttribute('href'), location.href) } catch { return }
    if (!u.hash || u.origin + u.pathname + u.search !== location.origin + location.pathname + location.search) return
    e.preventDefault()
    let id = u.hash.slice(1)
    try { id = decodeURIComponent(id) } catch {}
    const target = shadow.getElementById(id) || [...content.querySelectorAll('[name]')].find((n) => n.getAttribute('name') === id)
    if (target) target.scrollIntoView()
  })
  const onKey = (e) => {
    if (e.key === 'Escape' && !e.defaultPrevented) {
      e.preventDefault()
      close()
    }
  }
  const html = document.documentElement
  const overflow = html.style.overflow
  // The original page stays under the view: make it inert and invisible (search, translation).
  const pageBody = document.body
  const prevVisibility = pageBody ? pageBody.style.visibility : ''
  const prevInert = pageBody ? pageBody.inert : false
  const close = () => {
    if (window.__signetReader !== api) return
    window.__signetReader = null
    window.removeEventListener('keydown', onKey, true)
    host.remove()
    html.style.overflow = overflow
    if (pageBody) {
      pageBody.style.visibility = prevVisibility
      pageBody.inert = prevInert
    }
    send({ type: 'close' })
  }
  const api = {
    next: () => new Promise((resolve) => {
      if (pending.length) resolve(pending.shift())
      else waiting = resolve
    }),
    close
  }
  window.__signetReader = api
  apply()
  html.style.overflow = 'hidden'
  if (pageBody) {
    pageBody.style.visibility = 'hidden'
    pageBody.inert = true
  }
  html.append(host)
  window.addEventListener('keydown', onKey, true)
  root.tabIndex = -1
  root.focus({ preventScroll: true })
  return 'open'
})()

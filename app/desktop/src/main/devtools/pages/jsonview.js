// Signet's JSON viewer, run in an isolated world (see src/main/devtools/jsonview.ts).
// Replaces the raw display of a JSON response with a collapsible tree, with search, raw view and formatted view.
// The UI lives in a closed shadow root; styling goes through a constructed stylesheet (not subject to the page's CSP).

// eslint-disable-next-line @typescript-eslint/no-unused-vars -- called by the injected code (jsonview.ts)
function signetJsonView(L) {
  if (window.__signetJson) return 'already'
  const type = (document.contentType || '').toLowerCase()
  if (!/^(application\/([\w.+-]*\+)?json|text\/json)$/.test(type)) return 'not-json'
  const pre = document.querySelector('body > pre')
  if (!pre) return 'not-json'
  const raw = pre.textContent || ''
  if (raw.length > L.maxBytes) return 'too-big'
  let data
  try {
    data = JSON.parse(raw)
  } catch {
    return 'invalid'
  }

  const plural = (n, one, other) => (n === 1 || (L.zeroIsOne && n === 0) ? one : other).replace('{{count}}', n.toLocaleString(L.locale))
  const isObj = (v) => v !== null && typeof v === 'object'
  const childEntries = (v) => (Array.isArray(v) ? v.map((x, i) => [i, x]) : Object.entries(v))
  const IDENT = /^[A-Za-z_$][\w$]*$/
  const childPath = (path, key) => (typeof key === 'number' ? `${path}[${key}]` : IDENT.test(key) ? `${path}.${key}` : `${path}[${JSON.stringify(key)}]`)
  const CHUNK = 500

  // Node count (bounded): small document = fully expanded, large = first level only.
  let count = 0
  const countNodes = (v) => {
    if (count > 3000) return
    count++
    if (isObj(v)) for (const [, x] of childEntries(v)) countNodes(x)
  }
  countNodes(data)
  const open = new Set(['$'])
  const openAll = (v, path) => {
    if (!isObj(v)) return
    open.add(path)
    for (const [k, x] of childEntries(v)) openAll(x, childPath(path, k))
  }
  if (count <= 3000) openAll(data, '$')
  else if (isObj(data)) for (const [k] of childEntries(data)) open.add(childPath('$', k))
  /** Number of children shown per container (large arrays arrive in slices). */
  const shown = new Map()
  let hits = new Set()
  let hitList = []
  let hitIndex = -1
  let selected = null

  // ───────── Interface ─────────
  const host = document.createElement('signet-json')
  host.style.cssText = 'all: initial; position: fixed; inset: 0; z-index: 2147483647; display: block'
  const shadow = host.attachShadow({ mode: 'closed' })
  const sheet = new CSSStyleSheet()
  sheet.replaceSync(L.css)
  shadow.adoptedStyleSheets = [sheet]
  const root = document.createElement('div')
  root.className = 'root'
  shadow.append(root)
  // "Already shown" marker; the root is only visible in this isolated world, not to the page (also used by tests).
  window.__signetJson = shadow

  const el = (tag, cls, text) => {
    const e = document.createElement(tag)
    if (cls) e.className = cls
    if (text !== undefined) e.textContent = text
    return e
  }
  const button = (text, onClick, cls = '') => {
    const b = el('button', cls, text)
    b.type = 'button'
    b.addEventListener('click', onClick)
    return b
  }

  const bar = el('div', 'bar')
  const tabs = el('div', 'tabs')
  let mode = 'tree'
  const tabButtons = {}
  for (const [id, label] of [
    ['tree', L.tree],
    ['raw', L.raw],
    ['pretty', L.pretty]
  ]) {
    tabButtons[id] = button(label, () => setMode(id), 'tab')
    tabs.append(tabButtons[id])
  }
  const treeTools = el('div', 'tools')
  treeTools.append(
    button(L.expandAll, () => {
      openAll(data, '$')
      render()
    }),
    button(L.collapseAll, () => {
      open.clear()
      open.add('$')
      render()
    })
  )
  const search = el('input', 'search')
  search.type = 'search'
  search.placeholder = L.search
  search.spellcheck = false
  const hitCount = el('span', 'hits')
  treeTools.append(search, hitCount)
  const copyAll = button(L.copy, () => copy(JSON.stringify(data, null, 2), copyAll, L.copy), 'copy')
  bar.append(tabs, treeTools, el('span', 'spacer'), copyAll)

  const body = el('div', 'body')
  const status = el('div', 'status')
  const statusPath = el('code', 'path')
  const copyPath = button(L.copyPath, () => selected && copy(selected.path, copyPath, L.copyPath))
  const copyValue = button(L.copyValue, () => selected && copy(valueText(selected.value), copyValue, L.copyValue))
  status.append(statusPath, el('span', 'spacer'), copyPath, copyValue)
  root.append(bar, body, status)
  document.documentElement.append(host)
  document.documentElement.style.overflow = 'hidden'

  // Copy without clipboard permission: "copy" command on a temporary text area (user gesture).
  function copy(text, btn, label) {
    const ta = el('textarea')
    ta.value = text
    ta.style.cssText = 'position: fixed; opacity: 0'
    shadow.append(ta)
    ta.select()
    document.execCommand('copy')
    ta.remove()
    btn.textContent = L.copied
    setTimeout(() => (btn.textContent = label), 1200)
  }
  const valueText = (v) => (isObj(v) ? JSON.stringify(v, null, 2) : typeof v === 'string' ? v : JSON.stringify(v))

  function select(path, value, row) {
    shadow.querySelectorAll('.row.selected').forEach((r) => r.classList.remove('selected'))
    row.classList.add('selected')
    selected = { path, value }
    statusPath.textContent = path
    status.classList.add('on')
  }

  function primitive(v) {
    if (v === null) return el('span', 'v null', 'null')
    if (typeof v === 'string') {
      const span = el('span', 'v str')
      if (/^https?:\/\/\S+$/i.test(v)) {
        const a = el('a', '', JSON.stringify(v))
        a.href = v
        span.append(a)
      } else span.textContent = JSON.stringify(v)
      return span
    }
    return el('span', `v ${typeof v}`, String(v))
  }

  function summary(v) {
    const n = Array.isArray(v) ? v.length : Object.keys(v).length
    const text = Array.isArray(v) ? plural(n, L.items_one, L.items_other) : plural(n, L.keys_one, L.keys_other)
    return el('span', 'sum', `${Array.isArray(v) ? '[' : '{'}…${Array.isArray(v) ? ']' : '}'} ${text}`)
  }

  function node(key, value, path) {
    const wrap = el('div', 'node')
    const row = el('div', 'row')
    if (hits.has(path)) row.classList.add('hit')
    row.dataset.path = path
    // Empty container: shown as is ([] or {}), without an arrow.
    const container = isObj(value) && childEntries(value).length > 0
    const expanded = container && open.has(path)
    const toggle = el('span', container ? (expanded ? 'tw open' : 'tw') : 'tw none')
    row.append(toggle)
    if (key !== undefined)
      row.append(el('span', typeof key === 'number' ? 'k idx' : 'k', typeof key === 'number' ? String(key) : JSON.stringify(key)), el('span', 'colon', ': '))
    if (!container) row.append(isObj(value) ? el('span', 'brace', Array.isArray(value) ? '[]' : '{}') : primitive(value))
    else if (!expanded) row.append(summary(value))
    else row.append(el('span', 'brace', Array.isArray(value) ? '[' : '{'))
    row.addEventListener('click', (e) => {
      if (e.target.closest('a')) return
      if (container && (e.target === toggle || e.detail === 2 || e.altKey)) {
        if (open.has(path)) open.delete(path)
        else open.add(path)
        const fresh = node(key, value, path)
        wrap.replaceWith(fresh)
        select(path, value, fresh.querySelector('.row'))
        return
      }
      select(path, value, row)
    })
    wrap.append(row)
    if (expanded) {
      const kids = el('div', 'kids')
      const entries = childEntries(value)
      const limit = shown.get(path) || CHUNK
      for (const [k, x] of entries.slice(0, limit)) kids.append(node(k, x, childPath(path, k)))
      if (entries.length > limit) {
        const more = button(
          plural(entries.length - limit, L.more_one, L.more_other),
          () => {
            shown.set(path, limit + CHUNK)
            const fresh = node(key, value, path)
            wrap.replaceWith(fresh)
          },
          'more'
        )
        kids.append(more)
      }
      wrap.append(kids, el('div', 'row close', Array.isArray(value) ? ']' : '}'))
    }
    return wrap
  }

  function render() {
    body.replaceChildren(node(undefined, data, '$'))
    body.classList.remove('text')
  }

  function setMode(next) {
    mode = next
    for (const [id, b] of Object.entries(tabButtons)) b.classList.toggle('on', id === mode)
    treeTools.classList.toggle('hidden', mode !== 'tree')
    status.classList.toggle('hidden', mode !== 'tree')
    if (mode === 'tree') {
      render()
      // Back to the tree during a search: return to the current result.
      if (hitIndex >= 0) focusHit()
      return
    }
    const text = el('pre', 'text-view', mode === 'raw' ? raw : JSON.stringify(data, null, 2))
    body.replaceChildren(text)
    body.classList.add('text')
  }

  // Search: keys and values containing the text; their parents open, Enter moves to the next.
  let timer = 0
  search.addEventListener('input', () => {
    clearTimeout(timer)
    timer = setTimeout(runSearch, 150)
  })
  search.addEventListener('keydown', (e) => {
    if (e.key !== 'Enter' || !hitList.length) return
    e.preventDefault()
    hitIndex = (hitIndex + (e.shiftKey ? hitList.length - 1 : 1)) % hitList.length
    focusHit()
  })
  function runSearch() {
    const q = search.value.trim().toLowerCase()
    hits = new Set()
    hitList = []
    hitIndex = -1
    if (q) {
      // `parents`: paths of the containers above, with the position of the followed child (for arrays in slices).
      const walk = (v, path, key, parents) => {
        if (hitList.length >= 1000) return
        const keyHit = key !== undefined && String(key).toLowerCase().includes(q)
        const valueHit = !isObj(v) && String(v).toLowerCase().includes(q)
        if (keyHit || valueHit) {
          hits.add(path)
          hitList.push(path)
          for (const p of parents) {
            open.add(p.path)
            if (typeof p.index === 'number' && p.index >= (shown.get(p.path) || CHUNK)) shown.set(p.path, Math.ceil((p.index + 1) / CHUNK) * CHUNK)
          }
        }
        if (isObj(v)) for (const [k, x] of childEntries(v)) walk(x, childPath(path, k), k, [...parents, { path, index: k }])
      }
      walk(data, '$', undefined, [])
    }
    hitCount.textContent = q ? (hitList.length ? plural(hitList.length, L.results_one, L.results_other) : L.noResults) : ''
    render()
    if (hitList.length) {
      hitIndex = 0
      focusHit()
    }
  }
  function focusHit() {
    const path = hitList[hitIndex]
    const row = [...shadow.querySelectorAll('.row')].find((r) => r.dataset.path === path)
    if (!row) return
    shadow.querySelectorAll('.row.current').forEach((r) => r.classList.remove('current'))
    row.classList.add('current')
    row.scrollIntoView({ block: 'center' })
  }

  setMode('tree')
  return 'ok'
}

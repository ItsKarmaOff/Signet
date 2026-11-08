// Easel page: infinite board of texts, images and drawings. Port of Easel.tsx (vanilla, no framework).
// Bridge used: window.signetNotes.load() / save({title, items, view}) / remove() (see README).
;(function () {
  'use strict'
  const { t, h, icon, tool, clamp } = window.Signet
  const { cleanHtml, docIcon, setTabLook } = window.SignetNotesHtml
  const { project, rubberband, spring } = window.SignetSpring

  /** Pen and text colors (the first follows the theme). */
  const COLORS = ['var(--text)', '#e5484d', '#f5a524', '#30a46c', '#3e63dd', '#8e4ec6']
  const PEN_SIZE = 3
  const SAVE_DELAY = 600
  /** Maximum number of undo steps kept. */
  const HISTORY_MAX = 100
  /** Longest side of an added image (px): beyond it, it is downscaled before entering the Easel. */
  const IMAGE_MAX = 1600
  const ZOOM_MIN = 0.1
  const ZOOM_MAX = 8
  /** Threshold (screen px) before an element starts to move: a click or double-click does not shift it. */
  const MOVE_THRESHOLD = 4
  /** Window (ms) over which the release velocity of the view pan is measured. */
  const VELOCITY_WINDOW = 100
  /** Wheel silence (ms) after which the zoom returns within its limits. */
  const WHEEL_SETTLE = 100
  /** Size limit of an Easel (JSON, images included), same as the app's. */
  const EASEL_MAX = 30 * 1024 * 1024

  const reducedMotion = window.matchMedia('(prefers-reduced-motion: reduce)')
  const uid = () => crypto.randomUUID()
  const blank = (html) => !String(html ?? '').replace(/<br\s*\/?>|&nbsp;|\s/g, '')
  /** Pointer capture, with no effect if the pointer is no longer active. */
  const capture = (el, id) => {
    try {
      el.setPointerCapture(id)
    } catch {
      // unknown pointer
    }
  }

  /** Smoothed path (curves through segment midpoints) of a stroke. */
  function pathOf(points) {
    if (points.length < 4) return points.length >= 2 ? `M${points[0]} ${points[1]}l0.01 0` : ''
    let d = `M${points[0]} ${points[1]}`
    for (let i = 2; i < points.length - 2; i += 2) {
      const mx = (points[i] + points[i + 2]) / 2
      const my = (points[i + 1] + points[i + 3]) / 2
      d += `Q${points[i]} ${points[i + 1]} ${mx} ${my}`
    }
    return `${d}L${points[points.length - 2]} ${points[points.length - 1]}`
  }

  /** Image read, downscaled to IMAGE_MAX on a side (WebP): data URL and size. */
  async function readImage(file) {
    const url = URL.createObjectURL(file)
    try {
      const img = new Image()
      img.src = url
      await img.decode()
      const k = Math.min(1, IMAGE_MAX / Math.max(img.naturalWidth, img.naturalHeight))
      const width = Math.max(1, Math.round(img.naturalWidth * k))
      const height = Math.max(1, Math.round(img.naturalHeight * k))
      const canvas = document.createElement('canvas')
      canvas.width = width
      canvas.height = height
      canvas.getContext('2d').drawImage(img, 0, 0, width, height)
      return { src: canvas.toDataURL('image/webp', 0.9), width, height }
    } catch {
      return null
    } finally {
      URL.revokeObjectURL(url)
    }
  }

  // ───────── State ─────────
  const S = {
    ready: false,
    hue: 250,
    title: '',
    items: [],
    view: { x: 0, y: 0, zoom: 1 },
    tool: 'select',
    color: COLORS[0],
    selected: null,
    editing: null,
    draft: null,
    panning: false,
    tooBig: false,
    confirmDelete: false
  }
  let drag = null
  /** Bytes of images added but not yet rendered. */
  let addedSize = 0

  // ───────── DOM ─────────
  const app = document.getElementById('app')
  const page = h('div', 'easel-page')
  const titleInput = h('input', 'easel-title', { type: 'text', placeholder: t('easel.untitled'), 'aria-label': t('easel.title') })
  const head = h('div', 'easel-head', null, titleInput)
  const bar = h('div', 'easel-bar', { role: 'toolbar', 'aria-label': t('easel.toolbar') })
  const port = h('div', 'easel-port tool-select')
  const board = h('div', 'easel-board')
  const draftSvg = document.createElementNS('http://www.w3.org/2000/svg', 'svg')
  draftSvg.setAttribute('class', 'easel-draft')
  draftSvg.setAttribute('width', '1')
  draftSvg.setAttribute('height', '1')
  draftSvg.setAttribute('overflow', 'visible')
  const draftPath = document.createElementNS('http://www.w3.org/2000/svg', 'path')
  draftPath.setAttribute('fill', 'none')
  draftPath.setAttribute('stroke-width', String(PEN_SIZE))
  draftPath.setAttribute('stroke-linecap', 'round')
  draftPath.setAttribute('stroke-linejoin', 'round')
  draftSvg.append(draftPath)
  draftSvg.hidden = true
  const empty = h('div', 'easel-empty', null, t('easel.empty'))
  empty.hidden = true
  board.append(draftSvg)
  port.append(board, empty)
  page.append(head, bar, port)
  app.append(page)

  const fileInput = h('input', null, { type: 'file', accept: 'image/*', multiple: '', hidden: '' })
  fileInput.addEventListener('change', () => {
    const at = center()
    ;[...(fileInput.files ?? [])].forEach((f, i) => void addImage(f, { x: at.x + i * 24, y: at.y + i * 24 }))
    fileInput.value = ''
  })

  // Toolbar: built once, state toggled in render().
  const toolButtons = {}
  const mkTool = (id, label, iconName) => {
    const b = tool(label, iconName, 15, () => setTool(id), { keepFocus: false })
    toolButtons[id] = b
    return b
  }
  const imageBtn = tool(t('easel.image'), 'imagePlus', 15, () => fileInput.click(), { keepFocus: false })
  const colorButtons = COLORS.map((c) => {
    const b = h('button', 'easel-color', { type: 'button', title: t('easel.color'), 'aria-label': t('easel.color') })
    b.style.background = c
    b.addEventListener('click', () => pickColor(c))
    return b
  })
  const zoomBtn = h('button', 'easel-zoom', { type: 'button', title: t('easel.zoomReset') })
  zoomBtn.addEventListener('click', () => zoomCenter(1 / S.view.zoom))
  const delItemBtn = tool(t('easel.deleteItem'), 'erase', 15, () => S.selected && removeItem(S.selected), { keepFocus: false })
  const warn = h('span', 'note-warn', null, t('easel.tooBig'))
  bar.append(
    mkTool('select', t('easel.select'), 'pointer'),
    mkTool('text', t('easel.text'), 'type'),
    mkTool('pen', t('easel.pen'), 'pencil'),
    imageBtn,
    h('span', 'note-sep'),
    ...colorButtons,
    h('span', 'note-sep'),
    tool(t('easel.zoomOut'), 'minus', 15, () => zoomCenter(1 / 1.25), { keepFocus: false }),
    zoomBtn,
    tool(t('easel.zoomIn'), 'plus', 15, () => zoomCenter(1.25), { keepFocus: false }),
    h('span', 'note-sep'),
    delItemBtn,
    warn,
    fileInput
  )

  // Top-right trash with in-place confirmation.
  const trashBtn = tool(t('easel.deleteEasel'), 'trash', 15, () => {
    S.confirmDelete = true
    render()
  }, { keepFocus: false })
  const cancelBtn = h('button', null, { type: 'button' }, t('common.cancel'))
  cancelBtn.addEventListener('click', () => {
    S.confirmDelete = false
    render()
  })
  const confirmDel = h('button', 'danger', { type: 'button' }, t('notes.delete'))
  confirmDel.addEventListener('click', () => void window.signetNotes?.remove())
  const confirmBox = h('span', 'note-confirm', null, t('easel.deleteConfirm'), cancelBtn, confirmDel)
  head.append(trashBtn, confirmBox)

  // ───────── Item views ─────────
  /** id -> { el, item, selected, editing, key } */
  const views = new Map()

  const svgNS = 'http://www.w3.org/2000/svg'
  function strokeSvg(item) {
    const svg = document.createElementNS(svgNS, 'svg')
    svg.setAttribute('width', '100%')
    svg.setAttribute('height', '100%')
    svg.setAttribute('viewBox', `0 0 ${item.vw ?? item.w} ${item.vh ?? item.h}`)
    svg.setAttribute('preserveAspectRatio', 'none')
    const path = document.createElementNS(svgNS, 'path')
    path.setAttribute('d', pathOf(item.points ?? []))
    path.setAttribute('fill', 'none')
    path.style.stroke = item.color ?? 'currentColor'
    path.setAttribute('stroke-width', String(item.size ?? PEN_SIZE))
    path.setAttribute('stroke-linecap', 'round')
    path.setAttribute('stroke-linejoin', 'round')
    svg.append(path)
    return svg
  }

  /** Key of what the content of an item depends on (the position and selection are handled apart). */
  function contentKey(item) {
    if (item.type === 'text') return `${item.html ?? ''}|${item.color ?? ''}`
    if (item.type === 'image') return item.src ?? ''
    return [item.w, item.h, item.vw, item.vh, item.color, item.size, item.points].map((v) => (Array.isArray(v) ? v.length + ':' + v[0] + ':' + v[v.length - 1] : v)).join('|')
  }

  function buildContent(item, editing) {
    if (item.type === 'text') {
      if (editing) {
        const el = h('div', 'easel-text-edit', { contenteditable: 'true', 'data-item-id': item.id })
        el.style.color = item.color ?? ''
        el.innerHTML = item.html ?? ''
        el.addEventListener('input', () => patch(item.id, { html: cleanHtml(el.innerHTML) }, `text:${item.id}`))
        el.addEventListener('blur', () => commitText(item.id, cleanHtml(el.innerHTML)))
        el.addEventListener('paste', (e) => {
          const html = e.clipboardData.getData('text/html')
          if (!html) return
          e.preventDefault()
          document.execCommand('insertHTML', false, cleanHtml(html))
        })
        return el
      }
      const el = h('div', 'easel-text-view')
      el.style.color = item.color ?? ''
      el.innerHTML = item.html ?? ''
      return el
    }
    if (item.type === 'image') {
      const img = h('img', null, { alt: '', draggable: 'false' })
      img.src = item.src ?? ''
      return img
    }
    return strokeSvg(item)
  }

  function focusEditor(el) {
    el.focus()
    const sel = getSelection()
    sel.selectAllChildren(el)
    sel.collapseToEnd()
  }

  function updateView(v, item, selected, editing) {
    const el = v.el
    el.className = `easel-item easel-${item.type}${selected ? ' selected' : ''}${editing ? ' editing' : ''}`
    el.style.left = `${item.x}px`
    el.style.top = `${item.y}px`
    el.style.width = `${item.w}px`
    el.style.height = item.type === 'text' ? '' : `${item.h}px`
    const key = contentKey(item)
    const editChanged = v.editing !== editing
    // While typing, the editable node keeps its own content (and the caret).
    if (editChanged || (!editing && key !== v.key)) {
      const content = buildContent(item, editing)
      if (el.firstChild && !el.firstChild.classList?.contains('easel-handle')) el.firstChild.replaceWith(content)
      else el.prepend(content)
      if (editing && editChanged) v.focus = content
    }
    const wantHandle = selected && !editing
    const handle = el.querySelector(':scope > .easel-handle')
    if (wantHandle && !handle) {
      const hd = h('div', 'easel-handle')
      hd.addEventListener('pointerdown', (e) => onResizeDown(e, v.item))
      el.append(hd)
    } else if (!wantHandle && handle) handle.remove()
    v.item = item
    v.key = key
    v.selected = selected
    v.editing = editing
  }

  function createView(item) {
    const el = h('div')
    const v = { el, item, selected: false, editing: false, key: null }
    el.addEventListener('pointerdown', (e) => onItemDown(e, v.item))
    el.addEventListener('dblclick', () => {
      if (v.item.type !== 'text') return
      lastKey = null
      S.editing = v.item.id
      render()
    })
    return v
  }

  // ───────── Render ─────────
  function render() {
    const { view, tool: tl } = S
    // Header and tab look
    if (document.activeElement !== titleInput && titleInput.value !== S.title) titleInput.value = S.title
    trashBtn.hidden = S.confirmDelete
    confirmBox.hidden = !S.confirmDelete
    for (const [id, b] of Object.entries(toolButtons)) {
      b.classList.toggle('on', id === tl)
      b.setAttribute('aria-pressed', String(id === tl))
    }
    colorButtons.forEach((b, i) => {
      b.classList.toggle('on', COLORS[i] === S.color)
      b.setAttribute('aria-pressed', String(COLORS[i] === S.color))
    })
    zoomBtn.textContent = `${Math.round(view.zoom * 100)} %`
    delItemBtn.hidden = !S.selected
    warn.hidden = !S.tooBig
    port.className = `easel-port tool-${tl}${S.panning ? ' panning' : ''}`
    const grid = 24 * view.zoom
    port.style.backgroundSize = `${grid}px ${grid}px`
    port.style.backgroundPosition = `${view.x}px ${view.y}px`
    board.style.transform = `translate(${view.x}px, ${view.y}px) scale(${view.zoom})`

    // Items
    addedSize = 0
    const seen = new Set()
    for (const item of S.items) {
      seen.add(item.id)
      const sel = S.selected === item.id
      const edit = S.editing === item.id
      let v = views.get(item.id)
      if (!v) {
        v = createView(item)
        views.set(item.id, v)
        updateView(v, item, sel, edit)
      } else if (v.item !== item || v.selected !== sel || v.editing !== edit) updateView(v, item, sel, edit)
    }
    for (const [id, v] of views)
      if (!seen.has(id)) {
        v.el.remove()
        views.delete(id)
      }
    // DOM order = list order, the draft stroke last. Walk backwards and move only what is out of place.
    let ref = draftSvg
    for (let i = S.items.length - 1; i >= 0; i--) {
      const el = views.get(S.items[i].id).el
      if (el.nextSibling !== ref) board.insertBefore(el, ref)
      ref = el
    }
    // The editable text takes focus once it is in the page.
    for (const v of views.values())
      if (v.focus) {
        focusEditor(v.focus)
        v.focus = null
      }

    // Draft stroke
    draftSvg.hidden = !S.draft
    if (S.draft) {
      draftPath.setAttribute('d', pathOf(S.draft))
      draftPath.style.stroke = S.color
    }
    empty.hidden = !(S.ready && S.items.length === 0 && !S.draft)
    afterChange()
  }

  // Tab title and deferred save of any change.
  let savedKey = null
  let saveTimer = 0
  let lastLook = ''
  function afterChange() {
    if (!S.ready) return
    const look = `${S.title}|${S.hue}`
    if (look !== lastLook) {
      lastLook = look
      setTabLook(S.title.trim() || t('easel.untitled'), docIcon('easel', S.hue))
    }
    const key = [S.title, S.items, S.view.x, S.view.y, S.view.zoom]
    if (savedKey && savedKey[0] === key[0] && savedKey[1] === key[1] && savedKey[2] === key[2] && savedKey[3] === key[3] && savedKey[4] === key[4]) return
    savedKey = key
    clearTimeout(saveTimer)
    saveTimer = setTimeout(() => save(S.items), SAVE_DELAY)
  }
  function save(items) {
    Promise.resolve(window.signetNotes?.save({ title: S.title.trim(), items, view: S.view })).then((ok) => {
      S.tooBig = ok === false
      render()
    })
  }

  // Page closing: immediate save (text being typed included).
  function flush() {
    if (!S.ready) return
    const live = document.querySelector('.easel-text-edit')
    const id = live?.dataset.itemId
    const items = live && id ? S.items.map((i) => (i.id === id ? { ...i, html: cleanHtml(live.innerHTML) } : i)) : S.items
    void window.signetNotes?.save({ title: S.title.trim(), items, view: S.view })
  }
  window.addEventListener('pagehide', flush)
  document.addEventListener('visibilitychange', () => document.visibilityState === 'hidden' && flush())

  titleInput.addEventListener('input', () => {
    S.title = titleInput.value
    render()
  })

  // ───────── Geometry ─────────
  const toBoard = (clientX, clientY) => {
    const r = port.getBoundingClientRect()
    const v = S.view
    return { x: (clientX - r.left - v.x) / v.zoom, y: (clientY - r.top - v.y) / v.zoom }
  }
  function center() {
    const r = port.getBoundingClientRect()
    return toBoard(r.left + r.width / 2, r.top + r.height / 2)
  }

  // ───────── History ─────────
  const past = []
  const future = []
  /** Key of the last change: a same gesture or a same input creates only one entry. */
  let lastKey = null
  /** Applies a board change with a history entry (merged if the key is that of the previous change). */
  function change(fn, key) {
    const prev = S.items
    const next = fn(prev)
    if (next === prev) return
    if (!key || key !== lastKey) {
      past.push(prev)
      if (past.length > HISTORY_MAX) past.shift()
    }
    lastKey = key ?? null
    future.length = 0
    S.items = next
    render()
  }
  const sameList = (a, b) => a.length === b.length && a.every((x, i) => x === b[i])
  function travel(from, to) {
    const cur = S.items
    let target
    while ((target = from.pop()) && sameList(target, cur));
    if (!target) return
    to.push(cur)
    lastKey = null
    S.items = target
    const ids = new Set(target.map((i) => i.id))
    if (S.selected && !ids.has(S.selected)) S.selected = null
    if (S.editing && !ids.has(S.editing)) S.editing = null
    render()
  }
  const undo = () => travel(past, future)
  const redo = () => travel(future, past)
  const patch = (id, p, key) => change((list) => list.map((i) => (i.id === id ? { ...i, ...p } : i)), key)
  function removeItem(id) {
    change((list) => list.filter((i) => i.id !== id))
    if (S.selected === id) S.selected = null
    if (S.editing === id) S.editing = null
    render()
  }
  function setTool(id) {
    S.tool = id
    render()
  }

  /** Text left (blur): empty = deleted (in the same history entry if it was just typed). */
  function commitText(id, html) {
    if (S.editing !== id) return
    S.editing = null
    const typed = lastKey === `text:${id}`
    lastKey = null
    const item = S.items.find((i) => i.id === id)
    if (!item) return render()
    if (blank(html)) {
      if (typed) {
        S.items = S.items.filter((i) => i.id !== id)
        if (S.selected === id) S.selected = null
        render()
      } else removeItem(id)
    } else if (html !== item.html) patch(id, { html })
    else render()
  }

  // ───────── View: zoom, pan, glide ─────────
  let glide = []
  const stopGlide = () => {
    glide.forEach((stop) => stop())
    glide = []
  }
  /** Raw zoom (no limits) of the current wheel gesture, and its anchor point. */
  let rawZoom = null
  let velocityHistory = []

  function zoomAt(clientX, clientY, k) {
    const r = port.getBoundingClientRect()
    const px = clientX - r.left
    const py = clientY - r.top
    stopGlide()
    rawZoom = null
    const v = S.view
    const zoom = clamp(v.zoom * k, ZOOM_MIN, ZOOM_MAX)
    S.view = { zoom, x: px - (px - v.x) * (zoom / v.zoom), y: py - (py - v.y) * (zoom / v.zoom) }
    render()
  }
  function applyZoom(px, py, zoom) {
    const v = S.view
    S.view = { zoom, x: px - (px - v.x) * (zoom / v.zoom), y: py - (py - v.y) * (zoom / v.zoom) }
    render()
  }
  /** Wheel / pinch zoom: rubber-band resistance beyond the limits, spring back after a short silence. */
  function zoomWheel(clientX, clientY, k) {
    if (reducedMotion.matches) return zoomAt(clientX, clientY, k)
    stopGlide()
    const r = port.getBoundingClientRect()
    const raw = rawZoom ?? { z: S.view.zoom, px: 0, py: 0, timer: 0 }
    window.clearTimeout(raw.timer)
    raw.px = clientX - r.left
    raw.py = clientY - r.top
    raw.z = clamp(raw.z * k, ZOOM_MIN / 3, ZOOM_MAX * 3)
    // Rubber band on a logarithmic scale: a zoom is multiplicative.
    const limit = raw.z > ZOOM_MAX ? ZOOM_MAX : raw.z < ZOOM_MIN ? ZOOM_MIN : raw.z
    const shown = limit * Math.exp(rubberband(Math.log(raw.z / limit), 0.5))
    applyZoom(raw.px, raw.py, shown)
    raw.timer = window.setTimeout(() => {
      const z = S.view.zoom
      const to = clamp(z, ZOOM_MIN, ZOOM_MAX)
      rawZoom = null
      if (to === z) return
      glide = [spring({ from: z, to, response: 0.3, onUpdate: (v) => applyZoom(raw.px, raw.py, v), onDone: () => {} })]
    }, WHEEL_SETTLE)
    rawZoom = raw
  }
  function zoomCenter(k) {
    const r = port.getBoundingClientRect()
    zoomAt(r.left + r.width / 2, r.top + r.height / 2, k)
  }

  // Two-finger scroll: pan; pinch or Cmd/Ctrl + scroll: zoom.
  port.addEventListener(
    'wheel',
    (e) => {
      if (S.editing && e.target instanceof Element && e.target.closest('.easel-text.editing')) return
      e.preventDefault()
      if (e.ctrlKey || e.metaKey) zoomWheel(e.clientX, e.clientY, Math.exp(-e.deltaY * 0.01))
      else {
        stopGlide()
        S.view = { ...S.view, x: S.view.x - e.deltaX, y: S.view.y - e.deltaY }
        render()
      }
    },
    { passive: false }
  )

  // ───────── Adding content ─────────
  async function addImage(file, at) {
    const img = await readImage(file)
    if (!img) return
    // Cumulative size: several images from a single drop go through before the next render.
    if (JSON.stringify(S.items).length + addedSize + img.src.length > EASEL_MAX * 0.95) {
      S.tooBig = true
      return render()
    }
    addedSize += img.src.length
    const w = Math.min(img.width, 640)
    const hh = Math.round((w * img.height) / img.width)
    const item = { id: uid(), type: 'image', x: at.x - w / 2, y: at.y - hh / 2, w, h: hh, src: img.src }
    S.selected = item.id
    S.tool = 'select'
    change((list) => [...list, item])
  }
  function addText(at, html = '') {
    const item = { id: uid(), type: 'text', x: at.x, y: at.y - 12, w: 260, h: 40, html, color: S.color === COLORS[0] ? undefined : S.color }
    S.selected = item.id
    S.editing = html ? null : item.id
    S.tool = 'select'
    // Text key: the typing that follows merges into the addition.
    change((list) => [...list, item], `text:${item.id}`)
  }

  // Paste: image or text, at the center of the view (except while typing a text).
  document.addEventListener('paste', (e) => {
    const target = e.target instanceof Element ? e.target : null
    if (S.editing || target?.closest('input, [contenteditable="true"]')) return
    const file = [...(e.clipboardData?.files ?? [])].find((f) => f.type.startsWith('image/'))
    if (file) {
      e.preventDefault()
      void addImage(file, center())
      return
    }
    const text = e.clipboardData?.getData('text/plain')?.trim()
    if (text) {
      e.preventDefault()
      addText(center(), text.replace(/[&<>]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;' })[c]).replace(/\n/g, '<br>'))
    }
  })

  // Keyboard: tools (V, T, P), undo / redo, delete the chosen element, Escape.
  window.addEventListener('keydown', (e) => {
    // Escape during a move or a resize: the element returns to its original place.
    const d = drag
    if (e.key === 'Escape' && d && (d.kind === 'move' || d.kind === 'resize')) {
      e.preventDefault()
      // The cancelled gesture leaves no history entry.
      if (lastKey === `g:${d.item}`) {
        const before = past.pop()
        lastKey = null
        if (before) {
          S.items = before
          render()
        }
      } else patch(d.item, d.kind === 'move' ? { x: d.ox, y: d.oy } : { w: d.ow, h: d.oh })
      drag = null
      return
    }
    const target = e.target instanceof Element ? e.target : null
    if (target?.closest('input, [contenteditable="true"]')) {
      if (e.key === 'Escape') target.blur()
      return
    }
    // Undo / redo (text input keeps its native undo, above).
    if ((e.metaKey || e.ctrlKey) && !e.altKey && !e.isComposing) {
      const k = e.key.toLowerCase()
      if (k === 'z' || k === 'y') {
        e.preventDefault()
        if (k === 'y' || e.shiftKey) redo()
        else undo()
      }
      return
    }
    if (e.metaKey || e.ctrlKey || e.altKey) return
    if ((e.key === 'Backspace' || e.key === 'Delete') && S.selected) {
      e.preventDefault()
      removeItem(S.selected)
    } else if (e.key === 'Escape') {
      S.selected = null
      S.tool = 'select'
      render()
    } else if (e.key === 'v') setTool('select')
    else if (e.key === 't') setTool('text')
    else if (e.key === 'p') setTool('pen')
  })

  // ───────── Pointer on the background: drawing, text, view pan ─────────
  port.addEventListener('pointerdown', (e) => {
    if (e.button !== 0) return
    const p = toBoard(e.clientX, e.clientY)
    if (S.tool === 'text') {
      // Without this, the default click action gives focus back to the background right after: the empty text disappeared.
      e.preventDefault()
      addText(p)
      return
    }
    capture(port, e.pointerId)
    if (S.tool === 'pen') {
      drag = { kind: 'draw', id: e.pointerId, points: [p.x, p.y] }
      S.draft = [p.x, p.y]
      render()
      return
    }
    S.selected = null
    stopGlide()
    velocityHistory = [{ t: performance.now(), x: S.view.x, y: S.view.y }]
    drag = { kind: 'pan', id: e.pointerId, sx: e.clientX, sy: e.clientY, vx: S.view.x, vy: S.view.y }
    S.panning = true
    render()
  })

  port.addEventListener('pointermove', (e) => {
    const d = drag
    if (!d || d.id !== e.pointerId) return
    const z = S.view.zoom
    if (d.kind === 'pan') {
      const x = d.vx + e.clientX - d.sx
      const y = d.vy + e.clientY - d.sy
      const now = performance.now()
      velocityHistory = [...velocityHistory.filter((p) => now - p.t <= VELOCITY_WINDOW), { t: now, x, y }]
      S.view = { ...S.view, x, y }
      render()
    } else if (d.kind === 'draw') {
      const p = toBoard(e.clientX, e.clientY)
      const n = d.points.length
      // Points too close are ignored: the stroke stays light.
      if (Math.hypot(p.x - d.points[n - 2], p.y - d.points[n - 1]) * z < 2) return
      d.points.push(p.x, p.y)
      S.draft = [...d.points]
      render()
    } else if (d.kind === 'move') {
      // The move only starts after a few pixels: a click does not shift the element.
      if (!d.active && Math.hypot(e.clientX - d.sx, e.clientY - d.sy) < MOVE_THRESHOLD) return
      d.active = true
      patch(d.item, { x: d.ox + (e.clientX - d.sx) / z, y: d.oy + (e.clientY - d.sy) / z }, `g:${d.item}`)
    } else if (d.kind === 'resize') {
      const w = Math.max(d.keep ? 20 : 80, d.ow + (e.clientX - d.sx) / z)
      patch(d.item, d.keep ? { w, h: (w * d.oh) / d.ow } : { w }, `g:${d.item}`)
    }
  })

  function onUp(e) {
    const d = drag
    if (!d || d.id !== e.pointerId) return
    drag = null
    S.panning = false
    if (d.kind === 'pan' && !reducedMotion.matches) {
      // Glide: one spring per axis, launched at the release velocity.
      const now = performance.now()
      const hist = velocityHistory.filter((p) => now - p.t <= VELOCITY_WINDOW)
      const first = hist[0]
      const last = hist[hist.length - 1]
      const dt = first && last ? (last.t - first.t) / 1000 : 0
      // Finger held still before letting go: no momentum.
      if (dt > 0.015 && now - last.t < 60) {
        const vx = (last.x - first.x) / dt
        const vy = (last.y - first.y) / dt
        const v = S.view
        glide = ['x', 'y'].map((axis) =>
          spring({
            from: v[axis],
            to: v[axis] + project(axis === 'x' ? vx : vy),
            velocity: axis === 'x' ? vx : vy,
            response: 0.6,
            onUpdate: (value) => {
              S.view = { ...S.view, [axis]: value }
              render()
            },
            onDone: () => {}
          })
        )
      }
    }
    if (d.kind === 'draw') {
      S.draft = null
      const xs = d.points.filter((_, i) => i % 2 === 0)
      const ys = d.points.filter((_, i) => i % 2 === 1)
      const pad = PEN_SIZE
      const x = Math.min(...xs) - pad
      const y = Math.min(...ys) - pad
      const w = Math.max(...xs) - Math.min(...xs) + pad * 2
      const hh = Math.max(...ys) - Math.min(...ys) + pad * 2
      const points = d.points.map((v, i) => (i % 2 === 0 ? v - x : v - y))
      const color = S.color
      change((list) => [...list, { id: uid(), type: 'stroke', x, y, w, h: hh, vw: w, vh: hh, points, color, size: PEN_SIZE }])
      return
    }
    render()
  }
  port.addEventListener('pointerup', onUp)
  port.addEventListener('pointercancel', onUp)

  function onItemDown(e, item) {
    if (S.tool !== 'select' || e.button !== 0) return // pen and text: the background handles it (we draw on top)
    e.stopPropagation()
    stopGlide()
    S.selected = item.id
    if (S.editing === item.id) return render() // input: the text cursor keeps control
    capture(port, e.pointerId)
    lastKey = null
    drag = { kind: 'move', id: e.pointerId, item: item.id, sx: e.clientX, sy: e.clientY, ox: item.x, oy: item.y, active: false }
    render()
  }
  function onResizeDown(e, item) {
    e.stopPropagation()
    capture(port, e.pointerId)
    lastKey = null
    drag = { kind: 'resize', id: e.pointerId, item: item.id, sx: e.clientX, ow: item.w, oh: item.h, keep: item.type !== 'text' }
  }

  port.addEventListener('dblclick', (e) => {
    if (S.tool === 'select' && (e.target === port || (e.target instanceof Element && e.target.classList.contains('easel-board'))))
      addText(toBoard(e.clientX, e.clientY))
  })
  port.addEventListener('dragover', (e) => e.dataTransfer.types.includes('Files') && e.preventDefault())
  port.addEventListener('drop', (e) => {
    const files = [...e.dataTransfer.files].filter((f) => f.type.startsWith('image/'))
    if (!files.length) return
    e.preventDefault()
    const p = toBoard(e.clientX, e.clientY)
    files.forEach((f, i) => void addImage(f, { x: p.x + i * 24, y: p.y + i * 24 }))
  })

  function pickColor(c) {
    S.color = c
    const item = S.items.find((i) => i.id === S.selected)
    if (item && item.type !== 'image') patch(item.id, { color: c === COLORS[0] && item.type === 'text' ? undefined : c })
    else render()
  }

  // ───────── Loading ─────────
  render()
  Promise.resolve(window.signetNotes?.load()).then((e) => {
    if (!e) return
    S.hue = e.hue
    document.documentElement.style.setProperty('--h', String(e.hue))
    if (e.doc?.kind === 'easel') {
      S.title = e.doc.title
      // Cleaned texts; a text left empty (page closed while typing) is not kept.
      S.items = e.doc.items
        .map((i) => (i.type === 'text' ? { ...i, html: cleanHtml(i.html ?? '') } : i))
        .filter((i) => i.type !== 'text' || !blank(i.html))
      S.view = e.doc.view
    }
    S.ready = true
    render()
  })
})()

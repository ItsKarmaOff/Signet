(() => {
  if (window.__signetTr) return true
  const SKIP = new Set(['SCRIPT', 'STYLE', 'NOSCRIPT', 'CODE', 'PRE', 'TEXTAREA', 'INPUT', 'SELECT', 'OPTION', 'SVG', 'MATH', 'KBD', 'SAMP', 'VAR', 'TEMPLATE', 'IFRAME', 'CANVAS'])
  const INLINE = new Set(['A', 'ABBR', 'B', 'BDI', 'BDO', 'BIG', 'CITE', 'DEL', 'DFN', 'EM', 'FONT', 'I', 'INS', 'LABEL', 'MARK', 'NOBR', 'Q', 'S', 'SMALL', 'SPAN', 'STRONG', 'SUB', 'SUP', 'TIME', 'U'])
  const LETTER = /\p{L}/u
  const MAX = __BATCH_CHARS__
  /** Text nodes at most per sent paragraph (a very long block is split into several). */
  const PER_UNIT = 40
  const units = []
  const originals = new Map()
  const written = new Map()
  const waitingByEl = new Map()
  const ready = []
  let waiting = null
  let titleOriginal = null

  const accept = (node) => {
    const value = node.nodeValue
    if (!value || !LETTER.test(value)) return false
    const el = node.parentElement
    if (!el || SKIP.has(el.tagName) || el.closest('[translate="no"], .notranslate, [contenteditable=""], [contenteditable="true"], svg, math')) return false
    return true
  }
  /** Paragraph of a text node: its nearest ancestor that is not an inline text element. */
  const containerOf = (node) => {
    let el = node.parentElement
    while (el && INLINE.has(el.tagName) && el.parentElement) el = el.parentElement
    return el
  }
  const queue = (node) => {
    if (written.get(node) === node.nodeValue || !accept(node)) return
    const el = containerOf(node)
    if (!el) return
    let set = waitingByEl.get(el)
    if (!set) {
      set = new Set()
      waitingByEl.set(el, set)
      io.observe(el)
    }
    set.add(node)
  }
  const scan = (root) => {
    if (root.nodeType === 3) return queue(root)
    if (root.nodeType !== 1 && root.nodeType !== 9 && root.nodeType !== 11) return
    const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT)
    for (let n = walker.nextNode(); n; n = walker.nextNode()) queue(n)
  }
  const escape = (s) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')
  /** Size (markup included) of a piece of an overly long text. */
  const PART = 2000
  /** Splits an overly long text into sentences (otherwise at a space, otherwise bluntly), never splitting a surrogate pair. */
  const splitText = (text) => {
    const parts = []
    let rest = text.trim()
    while (escape(rest).length + 12 > PART) {
      let w = Math.min(rest.length, PART - 12)
      while (w > 1 && escape(rest.slice(0, w)).length + 12 > PART) w--
      const win = rest.slice(0, w)
      let cut = 0
      const sentence = /[.!?…]["')\]»”’]*\s+|[。！？！？]["')\]»”’」』]*/g
      for (let m = sentence.exec(win); m; m = sentence.exec(win)) cut = m.index + m[0].length
      if (cut < w / 4) {
        const space = /\s+/g
        cut = 0
        for (let m = space.exec(win); m; m = space.exec(win)) cut = m.index + m[0].length
      }
      if (cut < 1) cut = w
      const last = rest.charCodeAt(cut - 1)
      if (last >= 0xd800 && last <= 0xdbff && cut > 1) cut--
      parts.push(rest.slice(0, cut).trim())
      rest = rest.slice(cut).trim()
    }
    if (rest) parts.push(rest)
    return parts.filter(Boolean)
  }
  const isLive = (e) => (e.shared ? e.shared.node : e).isConnected
  const pump = () => {
    if (!waiting || ready.length === 0) return
    const batch = { ids: [], texts: [] }
    let size = 0
    let full = false
    while (ready.length && size < MAX && !full) {
      const raw = ready.shift().filter((n) => n.isConnected && written.get(n) !== n.nodeValue && n.nodeValue.trim())
      if (!raw.length) continue
      // A node longer than MAX is split into sentences; its pieces come back into the same node (see apply).
      const nodes = []
      for (const n of raw) {
        if (escape(n.nodeValue.trim()).length + 12 <= MAX) nodes.push(n)
        else {
          const shared = { node: n, got: [], left: 0 }
          for (const text of splitText(n.nodeValue)) nodes.push({ shared, text, k: shared.left++ })
        }
      }
      // Splits the unit into pieces that fit in a batch.
      const chunks = []
      let cur = []
      let len = 0
      for (const n of nodes) {
        const h = escape(n.text ?? n.nodeValue.trim()).length + 12
        if (cur.length && len + h > MAX) {
          chunks.push(cur)
          cur = []
          len = 0
        }
        cur.push(n)
        len += h
      }
      if (cur.length) chunks.push(cur)
      for (let c = 0; c < chunks.length; c++) {
        const html = chunks[c].map((n, k) => '<a i=' + k + '>' + escape(n.text ?? n.nodeValue.trim()) + '</a>').join(' ')
        if (batch.ids.length && size + html.length > MAX) {
          ready.unshift(...chunks.slice(c))
          full = true
          break
        }
        batch.ids.push(units.length)
        batch.texts.push(html)
        units.push(chunks[c])
        size += html.length
      }
    }
    if (batch.ids.length === 0) return
    const resolve = waiting
    waiting = null
    resolve(batch)
  }
  const io = new IntersectionObserver((entries) => {
    for (const entry of entries) {
      if (!entry.isIntersecting) continue
      const set = waitingByEl.get(entry.target)
      waitingByEl.delete(entry.target)
      io.unobserve(entry.target)
      if (!set) continue
      const nodes = [...set]
      for (let i = 0; i < nodes.length; i += PER_UNIT) ready.push(nodes.slice(i, i + PER_UNIT))
    }
    pump()
  }, { rootMargin: '100% 0px' })
  const mo = new MutationObserver((records) => {
    for (const r of records) {
      if (r.type === 'characterData') {
        // The page rewrote an already translated text: it is a new original.
        if (written.get(r.target) !== r.target.nodeValue) {
          written.delete(r.target)
          originals.delete(r.target)
          queue(r.target)
        }
      } else r.addedNodes.forEach(scan)
    }
  })
  const set = (node, text) => {
    const value = node.nodeValue
    if (!originals.has(node)) originals.set(node, value)
    // No original space before punctuation that opens the translated piece ("horse , which" → "horse, which").
    const lead = /^[,.;:!?)\]}»”]/.test(text) ? '' : /^\s*/.exec(value)[0]
    const next = lead + text + /\s*$/.exec(value)[0]
    written.set(node, next)
    node.nodeValue = next
  }

  window.__signetTr = {
    next: () => new Promise((resolve) => {
      if (waiting) waiting(null)
      waiting = resolve
      pump()
    }),
    title: () => (titleOriginal ?? document.title),
    setTitle: (text) => {
      if (titleOriginal === null) titleOriginal = document.title
      document.title = text
    },
    apply: (list, htmls) => {
      list.forEach((id, i) => {
        const nodes = units[id]
        const html = htmls[i]
        units[id] = null
        if (!nodes || typeof html !== 'string') return
        // Translated pieces in translation order; text outside a tag joins the next piece.
        const tpl = document.createElement('template')
        tpl.innerHTML = html
        const pieces = []
        let carry = ''
        for (const n of tpl.content.childNodes) {
          if (n.nodeType === 1 && n.hasAttribute('i')) {
            pieces.push((carry + ' ' + n.textContent).replace(/\s+/g, ' ').trim())
            carry = ''
          } else carry += n.textContent
        }
        if (carry.trim()) {
          if (pieces.length) pieces[pieces.length - 1] += ' ' + carry.trim()
          else pieces.push(carry.trim())
        }
        // One piece per node, in document order: the sentence reads right even if the language moves words around.
        // If pieces are missing, the remaining nodes keep their text; if there are too many, the last node takes the rest.
        const live = nodes.filter(isLive)
        let previous = ''
        live.forEach((node, k) => {
          if (k >= pieces.length) return
          let text = k === live.length - 1 ? pieces.slice(k).join(' ') : pieces[k]
          // Same punctuation on both sides of a piece boundary ("viola,, cello"): only one.
          const mark = text.charAt(0)
          if ((mark === ',' || mark === '.') && previous.endsWith(mark)) text = text.slice(1).trimStart()
          if (text && node.shared) {
            // Piece of a long text: the node is only written once all its pieces have come back.
            const sh = node.shared
            sh.got[node.k] = text
            if (--sh.left === 0 && sh.node.isConnected && !sh.got.includes(undefined) && sh.got.length) set(sh.node, sh.got.join(' '))
          } else if (text) set(node, text)
          previous = text
        })
      })
    },
    restore: () => {
      mo.disconnect()
      io.disconnect()
      for (const [node, value] of originals) if (node.isConnected) node.nodeValue = value
      if (titleOriginal !== null) document.title = titleOriginal
      if (waiting) waiting(null)
      waiting = null
      window.__signetTr = null
    }
  }
  scan(document.body || document.documentElement)
  mo.observe(document.documentElement, { childList: true, subtree: true, characterData: true })
  return true
})()

(() => {
  if (window.__signetEmoji) return
  const ATTR = 'data-signet-emoji'
  // Emoji shown in color: emoji presentation by default or forced (FE0F), modifiers, tags, ZWJ sequences,
  // flags and keycaps (1️⃣). Symbols with text presentation (©, ™, ♀…) stay text and follow the inversion.
  const PART = '(?:\\p{Emoji_Presentation}|\\p{Extended_Pictographic}\\uFE0F)(?:\\p{Emoji_Modifier}|\\uFE0F|[\\u{E0020}-\\u{E007F}])*'
  const SOURCE =
    '(?:\\p{Regional_Indicator}{2}|[#*0-9]\\uFE0F?\\u20E3|' + PART +
    '(?:\\u200D(?:\\p{Extended_Pictographic})(?:\\p{Emoji_Modifier}|\\uFE0F)*)*)+'
  const FIND = new RegExp(SOURCE, 'gu')
  const TEST = new RegExp(SOURCE, 'u')
  const SKIP = new Set(['SCRIPT', 'STYLE', 'NOSCRIPT', 'TEXTAREA', 'TITLE', 'TEMPLATE', 'OPTION', 'SELECT'])

  /** Original text node → pieces added after it and full text. */
  const pieces = new WeakMap()
  /** Added piece → original text node. */
  const owner = new WeakMap()

  const skipped = (el) =>
    !el || SKIP.has(el.tagName) || owner.has(el) || el.isContentEditable || !!el.closest('svg')

  const split = (node) => {
    const value = node.nodeValue
    if (!value || !TEST.test(value) || skipped(node.parentElement)) return
    const added = []
    let head = null
    let last = 0
    for (const m of value.matchAll(FIND)) {
      const before = value.slice(last, m.index)
      if (head === null) head = before
      else if (before) added.push(document.createTextNode(before))
      const span = document.createElement('span')
      span.setAttribute(ATTR, '')
      span.textContent = m[0]
      added.push(span)
      last = m.index + m[0].length
    }
    const tail = value.slice(last)
    if (tail) added.push(document.createTextNode(tail))
    node.nodeValue = head
    node.after(...added)
    for (const a of added) owner.set(a, node)
    pieces.set(node, { added, full: value })
  }

  /** Removes the pieces; with restore, puts the full text back in the original node. */
  const unsplit = (node, restore) => {
    const p = pieces.get(node)
    if (!p) return
    pieces.delete(node)
    if (restore) node.nodeValue = p.full
    for (const a of p.added) a.remove()
  }

  const scan = (root) => {
    if (root.nodeType === 3) return split(root)
    if (root.nodeType !== 1 && root.nodeType !== 9) return
    const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT)
    const found = []
    for (let n = walker.nextNode(); n; n = walker.nextNode()) if (!owner.has(n) && TEST.test(n.nodeValue)) found.push(n)
    for (const n of found) split(n)
  }

  const observer = new MutationObserver((records) => {
    const todo = new Set()
    for (const r of records) {
      if (r.type === 'characterData') {
        const node = r.target
        if (owner.has(node)) continue
        unsplit(node, false) // the page rewrote the whole text of the node
        todo.add(node)
      } else {
        for (const n of r.removedNodes) if (pieces.has(n)) unsplit(n, false)
        for (const n of r.addedNodes) if (!owner.has(n)) todo.add(n)
      }
    }
    for (const n of todo) if (n.isConnected) scan(n)
    observer.takeRecords() // our own changes
  })

  scan(document)
  observer.observe(document, { childList: true, subtree: true, characterData: true })

  window.__signetEmoji = () => {
    observer.disconnect()
    for (const el of document.querySelectorAll('span[' + ATTR + ']')) {
      const node = owner.get(el)
      if (node) unsplit(node, true)
    }
    delete window.__signetEmoji
  }
})()

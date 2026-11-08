// Note page: rich text note, saved as you type. Port of Note.tsx.
// Bridge used: window.signetNotes.load() / save({title, html}) / remove() (see README).
;(function () {
  'use strict'
  const { t, h, icon, tool } = window.Signet
  const { cleanHtml, docIcon, firstLine, setTabLook } = window.SignetNotesHtml

  /** Save delay after the last keystroke (ms). */
  const SAVE_DELAY = 400
  const BLOCKS = /^(P|DIV|H1|H2|H3|UL|OL|BLOCKQUOTE|PRE|HR)$/
  const BLOCK_TAGS = /^(P|DIV|H1|H2|H3|LI|BLOCKQUOTE|PRE)$/

  /** Block (paragraph, heading, list item...) that contains the selection, in the editor. */
  function currentBlock(root) {
    let node = getSelection()?.anchorNode ?? null
    while (node && node !== root) {
      if (node instanceof HTMLElement && BLOCK_TAGS.test(node.tagName)) return node
      node = node.parentNode
    }
    return null
  }

  /** Text of the block between its start and the cursor. */
  function textBeforeCaret(block) {
    const sel = getSelection()
    if (!sel || !sel.rangeCount || !sel.isCollapsed) return ''
    const r = document.createRange()
    r.setStart(block, 0)
    r.setEnd(sel.anchorNode, sel.anchorOffset)
    return r.toString()
  }

  /**
   * Text placed directly in the editor (first keystroke, old content) goes into a paragraph: the first line
   * is then a block, which the stylesheet shows as a title. The cursor stays where it was (same nodes, moved).
   */
  function wrapLooseText(root) {
    const loose = (n) => (n.nodeType === Node.TEXT_NODE && !!n.textContent) || (n instanceof HTMLElement && !BLOCKS.test(n.tagName))
    if (![...root.childNodes].some(loose)) return
    const sel = getSelection()
    const anchor = sel?.anchorNode
    const offset = sel?.anchorOffset ?? 0
    let run = null
    for (const node of [...root.childNodes]) {
      if (!loose(node)) {
        run = null
        continue
      }
      if (!run) {
        run = document.createElement('p')
        root.insertBefore(run, node)
      }
      run.append(node)
    }
    if (anchor && root.contains(anchor))
      sel.collapse(anchor, Math.min(offset, anchor.nodeType === Node.TEXT_NODE ? (anchor.textContent?.length ?? 0) : anchor.childNodes.length))
  }

  let env = null
  let timer = null
  let savedRange = null
  let tooBig = false

  const app = document.getElementById('app')
  const page = h('div', 'note-page')
  const bar = h('div', 'note-bar', { role: 'toolbar', 'aria-label': t('notes.toolbar') })
  const ed = h('div', 'note-editor is-empty', {
    contenteditable: 'true',
    spellcheck: 'true',
    role: 'textbox',
    'aria-multiline': 'true',
    'aria-label': t('notes.editor'),
    'data-placeholder': t('notes.placeholder')
  })
  const scroll = h('div', 'note-scroll', null, ed)
  page.append(bar, scroll)
  app.append(page)

  const untitled = t('notes.untitled')
  function refreshLook() {
    if (!env) return
    setTabLook(firstLine(ed) || untitled, docIcon('note', env.hue))
    const empty = !(ed.textContent ?? '').trim() && !ed.querySelector('img, hr, li')
    ed.classList.toggle('is-empty', empty)
  }

  function saveNow() {
    if (timer) clearTimeout(timer)
    timer = null
    if (!env) return
    Promise.resolve(window.signetNotes?.save({ title: firstLine(ed, 200), html: ed.innerHTML })).then((ok) => {
      tooBig = ok === false
      renderBar()
    })
  }
  function schedule() {
    wrapLooseText(ed)
    refreshLook()
    if (timer) clearTimeout(timer)
    timer = setTimeout(saveNow, SAVE_DELAY)
  }

  const exec = (command, value) => {
    ed.focus()
    document.execCommand(command, false, value)
    schedule()
  }
  /** Title, subtitle, quote, code: a second click goes back to a paragraph. */
  function toggleBlock(tag) {
    const block = currentBlock(ed)
    exec('formatBlock', block?.tagName.toLowerCase() === tag ? 'p' : tag)
  }
  /** Checklist: a bulleted list marked `checklist` (a second click goes back to the bulleted list). */
  function toggleChecklist() {
    let ul = currentBlock(ed)?.closest('ul')
    if (ul && ed.contains(ul)) {
      ul.classList.toggle('checklist')
      if (!ul.classList.length) ul.removeAttribute('class')
    } else {
      exec('insertUnorderedList')
      ul = currentBlock(ed)?.closest('ul')
      if (ul) ul.className = 'checklist'
    }
    if (ul) for (const li of ul.querySelectorAll(':scope > li')) if (!li.hasAttribute('data-checked')) li.setAttribute('data-checked', 'false')
    schedule()
  }

  // Link: the selection is kept while the address is typed in the bar.
  let linking = false
  let confirmDelete = false
  function startLink() {
    const sel = getSelection()
    savedRange = sel && sel.rangeCount ? sel.getRangeAt(0).cloneRange() : null
    linking = true
    renderBar()
  }
  function applyLink(value) {
    linking = false
    renderBar()
    const range = savedRange
    const raw = value.trim()
    if (!range || !raw) return ed.focus()
    const url = /^(https?:|mailto:)/i.test(raw) ? raw : `https://${raw}`
    const sel = getSelection()
    sel.removeAllRanges()
    sel.addRange(range)
    if (range.collapsed) exec('insertHTML', `<a href="${url.replace(/"/g, '&quot;')}">${raw.replace(/[<>&]/g, '')}</a>`)
    else exec('createLink', url)
  }

  function renderBar() {
    bar.replaceChildren()
    if (linking) {
      const input = h('input', null, { type: 'text', placeholder: t('notes.linkPlaceholder'), 'aria-label': t('notes.link') })
      const form = h('form', 'note-link', null, icon('link', 14), input)
      form.addEventListener('submit', (e) => {
        e.preventDefault()
        applyLink(input.value)
      })
      input.addEventListener('keydown', (e) => {
        if (e.key === 'Escape') {
          linking = false
          renderBar()
          ed.focus()
        }
      })
      bar.append(form)
      input.focus()
      return
    }
    const sep = () => h('span', 'note-sep')
    bar.append(
      tool(t('notes.heading'), 'h1', 16, () => toggleBlock('h1')),
      tool(t('notes.subheading'), 'h2', 16, () => toggleBlock('h2')),
      sep(),
      tool(t('notes.bold'), 'bold', 15, () => exec('bold')),
      tool(t('notes.italic'), 'italic', 15, () => exec('italic')),
      tool(t('notes.strike'), 'strike', 15, () => exec('strikeThrough')),
      sep(),
      tool(t('notes.bullets'), 'list', 16, () => exec('insertUnorderedList')),
      tool(t('notes.numbers'), 'ordered', 16, () => exec('insertOrderedList')),
      tool(t('notes.checklist'), 'checks', 16, toggleChecklist),
      sep(),
      tool(t('notes.quote'), 'quote', 15, () => toggleBlock('blockquote')),
      tool(t('notes.code'), 'code', 15, () => toggleBlock('pre')),
      tool(t('notes.link'), 'link', 15, startLink),
      h('span', 'note-grow')
    )
    if (tooBig) bar.append(h('span', 'note-warn', null, t('notes.tooBig')))
    if (confirmDelete) {
      const cancel = h('button', null, { type: 'button' }, t('common.cancel'))
      cancel.addEventListener('click', () => {
        confirmDelete = false
        renderBar()
      })
      const del = h('button', 'danger', { type: 'button' }, t('notes.delete'))
      del.addEventListener('click', () => void window.signetNotes?.remove())
      bar.append(h('span', 'note-confirm', null, t('notes.deleteConfirm'), cancel, del))
    } else {
      bar.append(
        tool(t('notes.deleteNote'), 'trash', 15, () => {
          confirmDelete = true
          renderBar()
        })
      )
    }
  }

  ed.addEventListener('input', schedule)
  ed.addEventListener('blur', () => timer && saveNow())

  ed.addEventListener('keydown', (e) => {
    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'k') {
      e.preventDefault()
      return startLink()
    }
    // Markdown-style shortcuts at the start of a line: "# ", "## ", "- ", "1. ", "[] ", "> ", "```".
    if (e.key === ' ') {
      const block = currentBlock(ed)
      const before = block ? textBeforeCaret(block) : ''
      const rules = {
        '#': () => document.execCommand('formatBlock', false, 'h1'),
        '##': () => document.execCommand('formatBlock', false, 'h2'),
        '-': () => document.execCommand('insertUnorderedList'),
        '*': () => document.execCommand('insertUnorderedList'),
        '1.': () => document.execCommand('insertOrderedList'),
        '[]': toggleChecklist,
        '[ ]': toggleChecklist,
        '>': () => document.execCommand('formatBlock', false, 'blockquote'),
        '```': () => document.execCommand('formatBlock', false, 'pre')
      }
      const rule = Object.hasOwn(rules, before) ? rules[before] : null
      if (block && rule && block.tagName !== 'LI') {
        e.preventDefault()
        const sel = getSelection()
        const r = document.createRange()
        r.setStart(block, 0)
        r.setEnd(sel.anchorNode, sel.anchorOffset)
        sel.removeAllRanges()
        sel.addRange(r)
        document.execCommand('delete')
        rule()
        schedule()
      }
    }
    // New checklist box: never checked by default (Enter copies the previous item).
    if (e.key === 'Enter')
      setTimeout(() => {
        const li = currentBlock(ed)
        if (li?.tagName === 'LI' && li.parentElement?.classList.contains('checklist')) li.setAttribute('data-checked', 'false')
      }, 0)
  })

  // Click in the margin of a checklist item: checks or unchecks. Cmd/Ctrl click on a link: new tab.
  ed.addEventListener('click', (e) => {
    const target = e.target
    if (!(target instanceof Element)) return
    const a = target.closest('a')
    if (a && (e.metaKey || e.ctrlKey) && /^https?:/i.test(a.getAttribute('href') ?? '')) {
      e.preventDefault()
      window.open(a.href, '_blank')
      return
    }
    const li = target.closest('li')
    if (li && li.parentElement?.classList.contains('checklist')) {
      if (e.clientX - li.getBoundingClientRect().left < 22) {
        li.setAttribute('data-checked', li.getAttribute('data-checked') === 'true' ? 'false' : 'true')
        schedule()
      }
    }
  })

  // Paste: cleaned HTML; pasted images rejected (text is enough for a note).
  ed.addEventListener('paste', (e) => {
    const html = e.clipboardData.getData('text/html')
    if (!html) return
    e.preventDefault()
    document.execCommand('insertHTML', false, cleanHtml(html))
    schedule()
  })

  // HTML drag and drop: same cleaning as paste; files and other drops unchanged.
  ed.addEventListener('drop', (e) => {
    const html = e.dataTransfer.getData('text/html')
    if (!html || e.dataTransfer.files.length) return
    e.preventDefault()
    const pos = document.caretPositionFromPoint?.(e.clientX, e.clientY)
    if (pos) {
      const r = document.createRange()
      r.setStart(pos.offsetNode, pos.offset)
      getSelection().removeAllRanges()
      getSelection().addRange(r)
    }
    document.execCommand('insertHTML', false, cleanHtml(html))
    schedule()
  })

  // Also saves when leaving the page (tab closed, discarded) or hiding it.
  const flush = () => timer && saveNow()
  window.addEventListener('pagehide', flush)
  document.addEventListener('visibilitychange', flush)

  renderBar()
  setTabLook(untitled, docIcon('note', 250))

  // Loading: cleaned content, space tint, tab title and icon.
  Promise.resolve(window.signetNotes?.load()).then((e) => {
    if (!e) return
    env = e
    document.documentElement.style.setProperty('--h', String(e.hue))
    document.execCommand('defaultParagraphSeparator', false, 'p')
    ed.innerHTML = e.doc?.kind === 'note' ? cleanHtml(e.doc.html) : ''
    wrapLooseText(ed)
    if (!ed.firstChild) ed.innerHTML = '<p><br></p>'
    refreshLook()
    ed.focus()
  })
})()

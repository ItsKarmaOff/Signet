import { useEffect, useRef, useState } from 'react'
import type { ClipboardEvent, DragEvent as ReactDragEvent, KeyboardEvent, MouseEvent, ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { Bold, Code, Heading1, Heading2, Italic, Link, List, ListChecks, ListOrdered, Quote, Strikethrough, Trash2 } from 'lucide-react'
import type { NotesEnv } from '@shared/notes'
import { cleanHtml, docIcon, firstLine, setTabLook } from './notesHtml'
import './notes.css'

/** Save delay after the last keystroke (ms). */
const SAVE_DELAY = 400

type Block = 'h1' | 'h2' | 'blockquote' | 'pre' | 'p'

/** Block (paragraph, heading, list item…) that contains the selection, in the editor. */
function currentBlock(root: HTMLElement): HTMLElement | null {
  const sel = getSelection()
  let node: Node | null = sel?.anchorNode ?? null
  while (node && node !== root) {
    if (node instanceof HTMLElement && /^(P|DIV|H1|H2|H3|LI|BLOCKQUOTE|PRE)$/.test(node.tagName)) return node
    node = node.parentNode
  }
  return null
}

/** Text of the block between its start and the cursor. */
function textBeforeCaret(block: HTMLElement): string {
  const sel = getSelection()
  if (!sel || !sel.rangeCount || !sel.isCollapsed) return ''
  const r = document.createRange()
  r.setStart(block, 0)
  r.setEnd(sel.anchorNode!, sel.anchorOffset)
  return r.toString()
}

const BLOCKS = /^(P|DIV|H1|H2|H3|UL|OL|BLOCKQUOTE|PRE|HR)$/

/**
 * Text placed directly in the editor (first keystroke, old content) goes into a paragraph: the first line
 * is then a block, which the stylesheet shows as a title. The cursor stays where it was (same nodes, moved).
 */
function wrapLooseText(root: HTMLElement): void {
  const loose = (n: Node) => (n.nodeType === Node.TEXT_NODE && !!n.textContent) || (n instanceof HTMLElement && !BLOCKS.test(n.tagName))
  if (![...root.childNodes].some(loose)) return
  const sel = getSelection()
  const anchor = sel?.anchorNode
  const offset = sel?.anchorOffset ?? 0
  let run: HTMLParagraphElement | null = null
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
    sel?.collapse(anchor, Math.min(offset, anchor.nodeType === Node.TEXT_NODE ? (anchor.textContent?.length ?? 0) : anchor.childNodes.length))
}

/** Route `#note/<id>`: rich text note, saved as you type. */
export function Note() {
  const { t } = useTranslation()
  const [empty, setEmpty] = useState(true)
  const [linking, setLinking] = useState(false)
  const [link, setLink] = useState('')
  const [confirmDelete, setConfirmDelete] = useState(false)
  const [tooBig, setTooBig] = useState(false)
  const ed = useRef<HTMLDivElement>(null)
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null)
  const savedRange = useRef<Range | null>(null)
  const envRef = useRef<NotesEnv | null>(null)

  const untitled = t('notes.untitled')
  const refreshLook = () => {
    const el = ed.current
    const e = envRef.current
    if (!el || !e) return
    setTabLook(firstLine(el) || untitled, docIcon('note', e.hue))
    setEmpty(!(el.textContent ?? '').trim() && !el.querySelector('img, hr, li'))
  }

  // Loading: cleaned content, space tint, tab title and icon.
  useEffect(() => {
    let alive = true
    void window.signetNotes?.load().then((e) => {
      if (!alive) return
      envRef.current = e
      document.documentElement.style.setProperty('--h', String(e.hue))
      document.execCommand('defaultParagraphSeparator', false, 'p')
      if (ed.current) {
        ed.current.innerHTML = e.doc?.kind === 'note' ? cleanHtml(e.doc.html) : ''
        wrapLooseText(ed.current)
        if (!ed.current.firstChild) ed.current.innerHTML = '<p><br></p>'
        refreshLook()
        ed.current.focus()
      }
    })
    return () => {
      alive = false
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps -- chargement unique
  }, [])

  const saveNow = () => {
    if (timer.current) clearTimeout(timer.current)
    timer.current = null
    const el = ed.current
    if (!el || !envRef.current) return
    void window.signetNotes?.save({ title: firstLine(el, 200), html: el.innerHTML }).then((ok) => setTooBig(!ok))
  }
  const schedule = () => {
    if (ed.current) wrapLooseText(ed.current)
    refreshLook()
    if (timer.current) clearTimeout(timer.current)
    timer.current = setTimeout(saveNow, SAVE_DELAY)
  }
  // Also saves when leaving the page (tab closed, discarded) or hiding it.
  useEffect(() => {
    const flush = () => timer.current && saveNow()
    window.addEventListener('pagehide', flush)
    document.addEventListener('visibilitychange', flush)
    return () => {
      window.removeEventListener('pagehide', flush)
      document.removeEventListener('visibilitychange', flush)
    }
  }, [])

  const exec = (command: string, value?: string) => {
    ed.current?.focus()
    document.execCommand(command, false, value)
    schedule()
  }
  /** Title, subtitle, quote, code: a second click goes back to a paragraph. */
  const toggleBlock = (tag: Block) => {
    const el = ed.current
    if (!el) return
    const block = currentBlock(el)
    exec('formatBlock', block?.tagName.toLowerCase() === tag ? 'p' : tag)
  }
  /** Checklist: a bulleted list marked `checklist` (a second click goes back to the bulleted list). */
  const toggleChecklist = () => {
    const el = ed.current
    if (!el) return
    let ul = currentBlock(el)?.closest('ul')
    if (ul && el.contains(ul)) {
      ul.classList.toggle('checklist')
      if (!ul.classList.length) ul.removeAttribute('class')
    } else {
      exec('insertUnorderedList')
      ul = currentBlock(el)?.closest('ul')
      if (ul) ul.className = 'checklist'
    }
    if (ul) for (const li of ul.querySelectorAll(':scope > li')) if (!li.hasAttribute('data-checked')) li.setAttribute('data-checked', 'false')
    schedule()
  }

  // Link: the selection is kept while the address is typed in the bar.
  const startLink = () => {
    const sel = getSelection()
    savedRange.current = sel && sel.rangeCount ? sel.getRangeAt(0).cloneRange() : null
    setLink('')
    setLinking(true)
  }
  const applyLink = () => {
    setLinking(false)
    const range = savedRange.current
    const raw = link.trim()
    if (!range || !raw) return ed.current?.focus()
    const url = /^(https?:|mailto:)/i.test(raw) ? raw : `https://${raw}`
    const sel = getSelection()
    sel?.removeAllRanges()
    sel?.addRange(range)
    if (range.collapsed) exec('insertHTML', `<a href="${url.replace(/"/g, '&quot;')}">${raw.replace(/[<>&]/g, '')}</a>`)
    else exec('createLink', url)
  }

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    const el = ed.current
    if (!el) return
    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'k') {
      e.preventDefault()
      return startLink()
    }
    // Markdown-style shortcuts at the start of a line: "# ", "## ", "- ", "1. ", "[] ", "> ", "```".
    if (e.key === ' ') {
      const block = currentBlock(el)
      const before = block ? textBeforeCaret(block) : ''
      const rules: Record<string, () => void> = {
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
      const rule = rules[before]
      if (block && rule && block.tagName !== 'LI') {
        e.preventDefault()
        const sel = getSelection()!
        const r = document.createRange()
        r.setStart(block, 0)
        r.setEnd(sel.anchorNode!, sel.anchorOffset)
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
        const li = currentBlock(el)
        if (li?.tagName === 'LI' && li.parentElement?.classList.contains('checklist')) li.setAttribute('data-checked', 'false')
      }, 0)
  }

  // Click in the margin of a checklist item: checks or unchecks. ⌘ click on a link: new tab.
  const onClick = (e: MouseEvent<HTMLDivElement>) => {
    const target = e.target as HTMLElement
    const a = target.closest('a')
    if (a && (e.metaKey || e.ctrlKey) && /^https?:/i.test(a.getAttribute('href') ?? '')) {
      e.preventDefault()
      window.open(a.href, '_blank')
      return
    }
    const li = target.closest('li')
    if (li && li.parentElement?.classList.contains('checklist')) {
      const left = li.getBoundingClientRect().left
      if (e.clientX - left < 22) {
        li.setAttribute('data-checked', li.getAttribute('data-checked') === 'true' ? 'false' : 'true')
        schedule()
      }
    }
  }

  // Paste: cleaned HTML; pasted images rejected (text is enough for a note).
  const onPaste = (e: ClipboardEvent<HTMLDivElement>) => {
    const html = e.clipboardData.getData('text/html')
    if (!html) return
    e.preventDefault()
    document.execCommand('insertHTML', false, cleanHtml(html))
    schedule()
  }

  // HTML drag and drop: same cleaning as paste; files and other drops unchanged.
  const onDrop = (e: ReactDragEvent<HTMLDivElement>) => {
    const html = e.dataTransfer.getData('text/html')
    if (!html || e.dataTransfer.files.length) return
    e.preventDefault()
    const pos = document.caretPositionFromPoint?.(e.clientX, e.clientY)
    if (pos) {
      const r = document.createRange()
      r.setStart(pos.offsetNode, pos.offset)
      getSelection()?.removeAllRanges()
      getSelection()?.addRange(r)
    }
    document.execCommand('insertHTML', false, cleanHtml(html))
    schedule()
  }

  const remove = () => void window.signetNotes?.remove()

  const tool = (label: string, icon: ReactNode, run: () => void) => (
    <button type="button" className="note-tool" title={label} aria-label={label} onMouseDown={(e) => e.preventDefault()} onClick={run}>
      {icon}
    </button>
  )

  return (
    <div className="note-page">
      <div className="note-bar" role="toolbar" aria-label={t('notes.toolbar')}>
        {linking ? (
          <form
            className="note-link"
            onSubmit={(e) => {
              e.preventDefault()
              applyLink()
            }}
          >
            <Link size={14} />
            <input
              autoFocus
              value={link}
              placeholder={t('notes.linkPlaceholder')}
              onChange={(e) => setLink(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Escape') {
                  setLinking(false)
                  ed.current?.focus()
                }
              }}
              aria-label={t('notes.link')}
            />
          </form>
        ) : (
          <>
            {tool(t('notes.heading'), <Heading1 size={16} />, () => toggleBlock('h1'))}
            {tool(t('notes.subheading'), <Heading2 size={16} />, () => toggleBlock('h2'))}
            <span className="note-sep" />
            {tool(t('notes.bold'), <Bold size={15} />, () => exec('bold'))}
            {tool(t('notes.italic'), <Italic size={15} />, () => exec('italic'))}
            {tool(t('notes.strike'), <Strikethrough size={15} />, () => exec('strikeThrough'))}
            <span className="note-sep" />
            {tool(t('notes.bullets'), <List size={16} />, () => exec('insertUnorderedList'))}
            {tool(t('notes.numbers'), <ListOrdered size={16} />, () => exec('insertOrderedList'))}
            {tool(t('notes.checklist'), <ListChecks size={16} />, toggleChecklist)}
            <span className="note-sep" />
            {tool(t('notes.quote'), <Quote size={15} />, () => toggleBlock('blockquote'))}
            {tool(t('notes.code'), <Code size={15} />, () => toggleBlock('pre'))}
            {tool(t('notes.link'), <Link size={15} />, startLink)}
            <span className="note-grow" />
            {tooBig && <span className="note-warn">{t('notes.tooBig')}</span>}
            {confirmDelete ? (
              <span className="note-confirm">
                {t('notes.deleteConfirm')}
                <button type="button" onClick={() => setConfirmDelete(false)}>
                  {t('common.cancel')}
                </button>
                <button type="button" className="danger" onClick={remove}>
                  {t('notes.delete')}
                </button>
              </span>
            ) : (
              tool(t('notes.deleteNote'), <Trash2 size={15} />, () => setConfirmDelete(true))
            )}
          </>
        )}
      </div>
      <div className="note-scroll">
        <div
          ref={ed}
          className={`note-editor${empty ? ' is-empty' : ''}`}
          contentEditable
          suppressContentEditableWarning
          spellCheck
          role="textbox"
          aria-multiline
          aria-label={t('notes.editor')}
          data-placeholder={t('notes.placeholder')}
          onInput={schedule}
          onKeyDown={onKeyDown}
          onClick={onClick}
          onPaste={onPaste}
          onDrop={onDrop}
          onBlur={() => timer.current && saveNow()}
        />
      </div>
    </div>
  )
}

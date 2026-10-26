import { BrowserWindow, dialog, net } from 'electron'
import type { Browser } from '../browser/browser'
import { currentLocale, t } from '../app/i18n'

/** Isolated world reserved for translation (1001: Boosts, 1002: media, 1003: reader). */
const WORLD = 1004
const ENDPOINT = 'https://translate.googleapis.com/translate_a/t'
/** Maximum size (characters) of a batch sent to the service. */
const BATCH_CHARS = 6000
/** With no visible text left to translate after this delay, the page is considered translated. */
const SETTLE_MS = 1500

/**
 * Page script (isolated world). Texts are grouped by paragraph (nearest ancestor that is not an inline text
 * element) so the service translates whole sentences: each text node is tagged there (`<a i=n>`), and the
 * translated pieces come back into the same nodes, in order. Only the nodes' value changes (never the structure),
 * which spares pages built with React and the like. Visible (or nearly visible) paragraphs go first, as the page
 * scrolls and adds content; `next()` returns the next batch, `apply()` sets the translations, `restore()` puts
 * the original page back. Evaluated as an expression.
 */
const INSTALL = String.raw`(() => {
  if (window.__signetTr) return true
  const SKIP = new Set(['SCRIPT', 'STYLE', 'NOSCRIPT', 'CODE', 'PRE', 'TEXTAREA', 'INPUT', 'SELECT', 'OPTION', 'SVG', 'MATH', 'KBD', 'SAMP', 'VAR', 'TEMPLATE', 'IFRAME', 'CANVAS'])
  const INLINE = new Set(['A', 'ABBR', 'B', 'BDI', 'BDO', 'BIG', 'CITE', 'DEL', 'DFN', 'EM', 'FONT', 'I', 'INS', 'LABEL', 'MARK', 'NOBR', 'Q', 'S', 'SMALL', 'SPAN', 'STRONG', 'SUB', 'SUP', 'TIME', 'U'])
  const LETTER = /\p{L}/u
  const MAX = ${BATCH_CHARS}
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
})()`

const NEXT = 'window.__signetTr ? window.__signetTr.next() : null'
const RESTORE = 'window.__signetTr && window.__signetTr.restore()'

/** Language code accepted by the service: the base language, except Chinese (simplified / traditional). */
export function serviceLang(code: string): string {
  const lower = code.toLowerCase()
  if (lower.startsWith('zh')) return /tw|hk|mo|hant/.test(lower) ? 'zh-TW' : 'zh-CN'
  return lower.split('-')[0]
}

/** Sends a batch to the service; returns the translations in the same order. */
async function translateBatch(texts: string[], target: string, format: 'text' | 'html' = 'text'): Promise<string[]> {
  const body = new URLSearchParams()
  for (const text of texts) body.append('q', text)
  let res: Response | null = null
  // A few attempts with growing wait on 429 / 5xx (honoring Retry-After) or network error.
  for (let attempt = 0; attempt < 4; attempt++) {
    let delay = 1000 * 2 ** attempt
    try {
      res = await net.fetch(`${ENDPOINT}?client=gtx&sl=auto&tl=${encodeURIComponent(target)}&format=${format}`, {
        method: 'POST',
        body: body.toString(),
        headers: { 'content-type': 'application/x-www-form-urlencoded;charset=UTF-8' }
      })
    } catch (error) {
      if (attempt === 3) throw error
      await new Promise((r) => setTimeout(r, delay))
      continue
    }
    if (res.ok || (res.status !== 429 && res.status < 500) || attempt === 3) break
    const after = Number(res.headers.get('retry-after'))
    if (Number.isFinite(after) && after > 0) delay = Math.min(after * 1000, 15000)
    await new Promise((r) => setTimeout(r, delay))
  }
  if (!res || !res.ok) throw new Error(`HTTP ${res?.status}`)
  const data: unknown = await res.json()
  // Expected shape: [[translation, detected language], …]; a single text may come back without an enclosing array.
  const list = Array.isArray(data) && texts.length === 1 && typeof data[0] === 'string' ? [data] : data
  if (!Array.isArray(list) || list.length !== texts.length) throw new Error('Unexpected response from the translation service')
  return list.map((item: unknown, i) => (typeof item === 'string' ? item : Array.isArray(item) && typeof item[0] === 'string' ? item[0] : texts[i]))
}

/** In-place page translation: the page text is replaced, links and scripts stay intact. */
export class Translator {
  /** Token of the translation in progress per tab: a navigation or a return to the original invalidates it. */
  private runs = new Map<string, number>()
  private seq = 0

  constructor(private b: Browser) {}

  /** Target language: the first preferred site language, otherwise the UI language. */
  target(): string {
    return serviceLang(this.b.store.state.settings.languages[0] ?? 'en')
  }

  /** Name of the target language in the UI language ("French"). */
  targetName(): string {
    const code = this.target()
    try {
      return new Intl.DisplayNames([currentLocale()], { type: 'language' }).of(code) ?? code
    } catch {
      return code
    }
  }

  /** Language of the page if it is not among the preferred languages (translation is then offered), otherwise undefined. */
  foreign(lang: string): string | undefined {
    const code = lang.trim().toLowerCase()
    if (!/^[a-z]{2,3}(-[a-z0-9]+)*$/.test(code)) return undefined
    const wanted = new Set(this.b.store.state.settings.languages.map(serviceLang))
    return wanted.has(serviceLang(code)) ? undefined : code
  }

  /** True if the tab's page can be translated. */
  canTranslate(tabId: string | null): boolean {
    const wc = tabId ? this.b.views.viewOf(tabId)?.webContents : undefined
    return !!wc && !wc.isDestroyed() && /^(https?|file):/i.test(wc.getURL())
  }

  toggle(tabId: string): void {
    const tab = this.b.store.state.tabs[tabId]
    if (!tab) return
    if (tab.translation) this.restore(tabId)
    else void this.start(tabId)
  }

  /** Main frame navigation: the original page comes back by itself, the state is forgotten. */
  onNavigate(tabId: string): void {
    this.runs.delete(tabId)
    if (this.b.store.state.tabs[tabId]?.translation) this.b.broadcast.patchTab(tabId, { translation: undefined }, { soft: true })
  }

  private restore(tabId: string): void {
    this.runs.delete(tabId)
    const wc = this.b.views.viewOf(tabId)?.webContents
    if (wc && !wc.isDestroyed()) wc.executeJavaScriptInIsolatedWorld(WORLD, [{ code: RESTORE }]).catch(() => {})
    this.b.broadcast.patchTab(tabId, { translation: undefined })
  }

  private async start(tabId: string): Promise<void> {
    const wc = this.b.views.viewOf(tabId)?.webContents
    if (!wc || wc.isDestroyed() || !this.canTranslate(tabId)) return
    // Reader mode leaves the original page under the view: nothing to translate.
    if (this.b.store.state.tabs[tabId]?.reader === 'on') return
    const run = ++this.seq
    this.runs.set(tabId, run)
    const alive = () => this.runs.get(tabId) === run && !wc.isDestroyed()
    const exec = (code: string) => wc.executeJavaScriptInIsolatedWorld(WORLD, [{ code }])
    const target = this.target()
    this.b.broadcast.patchTab(tabId, { translation: 'working' })
    let applied = 0
    const settle = setTimeout(() => alive() && this.b.broadcast.patchTab(tabId, { translation: 'on' }, { soft: true }), SETTLE_MS)
    try {
      await exec(INSTALL)
      // The tab title follows the page.
      const title = (await exec('window.__signetTr ? window.__signetTr.title() : ""')) as string
      if (title && alive()) {
        const [translated] = await translateBatch([title], target)
        if (alive()) await exec(`window.__signetTr && window.__signetTr.setTitle(${JSON.stringify(translated)})`)
      }
      while (alive()) {
        const batch = (await exec(NEXT)) as { ids: number[]; texts: string[] } | null
        if (!batch || !alive()) break
        const out = await translateBatch(batch.texts, target, 'html')
        if (!alive()) break
        await exec(`window.__signetTr && window.__signetTr.apply(${JSON.stringify(batch.ids)}, ${JSON.stringify(out)})`)
        applied++
        this.b.broadcast.patchTab(tabId, { translation: 'on' }, { soft: true })
      }
    } catch (error) {
      // Page closed or replaced during translation: nothing to report.
      if (!alive() || /disposed|destroyed/i.test(String(error))) return
      console.error('Translation:', error)
      // Lasting failure: what is already translated stays, we stop there.
      if (applied > 0) this.b.broadcast.patchTab(tabId, { translation: 'on' })
      else this.restore(tabId)
      const win = BrowserWindow.fromWebContents(wc) ?? this.b.win
      const options: Electron.MessageBoxOptions = { type: 'warning', message: t('translate.failed'), detail: t('translate.failedDetail') }
      void (win ? dialog.showMessageBox(win, options) : dialog.showMessageBox(options))
    } finally {
      clearTimeout(settle)
    }
  }
}

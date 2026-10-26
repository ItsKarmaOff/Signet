import type { WebContents } from 'electron'
import READABILITY from '@mozilla/readability/Readability.js?raw'
import READERABLE from '@mozilla/readability/Readability-readerable.js?raw'
import { DEFAULT_READER, type ReaderPrefs } from '@shared/types'
import type { Browser } from '../browser/browser'
import { t } from '../app/i18n'

/** Isolated world reserved for reader mode (1001: Boosts, 1002: media, 1004: translation). */
const WORLD = 1003
/** After an in-page navigation (single-page sites), wait until the content is in place before examining. */
const IN_PAGE_DELAY = 900
/** Between two examinations triggered by in-page navigations: at least this interval (the latest state is always examined). */
const IN_PAGE_MIN_GAP = 5000

/** Does the page look like an article, and what language does it declare? Evaluated as an expression. */
const PROBE = `(() => {
  ${READERABLE}
  let readerable = false
  try { readerable = isProbablyReaderable(document) } catch {}
  return { readerable, lang: document.documentElement.lang || '' }
})()`

const CLOSE = 'window.__signetReader ? (window.__signetReader.close(), true) : false'
const NEXT = 'window.__signetReader ? window.__signetReader.next() : null'

/** Reader view styling (inside a closed shadow root: the page cannot touch it, nor can its styles). */
const CSS = `
:host { all: initial }
.root { position: fixed; inset: 0; overflow-y: auto; overscroll-behavior: contain; background: var(--bg); color: var(--text);
  --bg: #fff; --text: #1d1d1f; --muted: #6e6e73; --line: rgb(0 0 0 / .1); --link: #0a66d6; --bar: rgb(255 255 255 / .82); --btn: rgb(0 0 0 / .05);
  font-size: var(--size); -webkit-font-smoothing: antialiased; color-scheme: light; outline: none }
.root[data-theme="sepia"] { --bg: #f6eedd; --text: #4b3b29; --muted: #8a7559; --line: rgb(75 59 41 / .16); --link: #9a5418; --bar: rgb(246 238 221 / .85); --btn: rgb(75 59 41 / .07) }
.root[data-theme="dark"] { --bg: #1c1c1e; --text: #e8e8ed; --muted: #98989f; --line: rgb(255 255 255 / .12); --link: #6cb4ff; --bar: rgb(28 28 30 / .82); --btn: rgb(255 255 255 / .08); color-scheme: dark }
@media (prefers-color-scheme: dark) {
  .root[data-theme="auto"] { --bg: #1c1c1e; --text: #e8e8ed; --muted: #98989f; --line: rgb(255 255 255 / .12); --link: #6cb4ff; --bar: rgb(28 28 30 / .82); --btn: rgb(255 255 255 / .08); color-scheme: dark }
}
.bar { position: sticky; top: 0; z-index: 1; display: flex; gap: 6px; align-items: center; justify-content: center; padding: 8px 12px;
  background: var(--bar); backdrop-filter: saturate(1.6) blur(18px); border-bottom: .5px solid var(--line);
  font: 13px/1 -apple-system, BlinkMacSystemFont, system-ui, sans-serif; opacity: .55; transition: opacity .15s }
.bar:hover, .bar:focus-within { opacity: 1 }
.group { display: flex; gap: 2px; padding: 2px; border-radius: 9px; background: var(--btn) }
button { all: unset; box-sizing: border-box; height: 26px; min-width: 30px; padding: 0 9px; border-radius: 7px; display: inline-flex; align-items: center; justify-content: center;
  gap: 6px; color: var(--text); cursor: default; font: inherit; white-space: nowrap }
button:hover { background: var(--btn) }
button:active { transform: scale(.97) }
button:focus-visible { outline: 2px solid var(--link); outline-offset: 1px }
button[aria-pressed="true"] { background: var(--bg); box-shadow: 0 0 0 .5px var(--line), 0 1px 2px rgb(0 0 0 / .12) }
.small { font-size: 12px } .big { font-size: 16px }
.dot { width: 14px; height: 14px; border-radius: 50%; box-shadow: inset 0 0 0 1px rgb(0 0 0 / .18) }
.close { margin-left: 8px }
article { box-sizing: border-box; max-width: var(--measure); margin: 0 auto; padding: 48px 24px 96px; line-height: 1.6 }
.root[data-width="narrow"] { --measure: 34em } .root[data-width="normal"] { --measure: 40em } .root[data-width="wide"] { --measure: 50em }
.root[data-font="serif"] article { font-family: ui-serif, "New York", "Iowan Old Style", Charter, Georgia, serif }
.root[data-font="sans"] article { font-family: -apple-system, BlinkMacSystemFont, system-ui, sans-serif }
.site { font: 600 .72em/1.3 -apple-system, BlinkMacSystemFont, system-ui, sans-serif; text-transform: uppercase; letter-spacing: .06em; color: var(--muted); margin-bottom: .8em }
h1.title { font-size: 1.9em; line-height: 1.15; letter-spacing: -.012em; margin: 0 0 .45em }
.meta { color: var(--muted); font-size: .82em; margin-bottom: 2.2em; padding-bottom: 1.6em; border-bottom: .5px solid var(--line) }
.content h1, .content h2, .content h3, .content h4 { line-height: 1.25; margin: 1.6em 0 .5em }
.content h1 { font-size: 1.5em } .content h2 { font-size: 1.32em } .content h3 { font-size: 1.14em } .content h4 { font-size: 1em }
.content p, .content ul, .content ol, .content blockquote, .content figure, .content table, .content pre { margin: 0 0 1.05em }
.content a { color: var(--link); text-decoration: none } .content a:hover { text-decoration: underline }
.content img, .content video, .content picture, .content svg { max-width: 100%; height: auto; border-radius: 6px; display: block; margin: 1.2em auto }
.content figcaption { color: var(--muted); font-size: .8em; text-align: center; margin-top: .5em }
.content blockquote { border-left: 3px solid var(--line); margin-left: 0; padding-left: 1em; color: var(--muted) }
.content pre, .content code { font-family: ui-monospace, "SF Mono", Menlo, monospace; font-size: .86em }
.content pre { background: var(--btn); padding: .9em 1em; border-radius: 8px; overflow-x: auto; line-height: 1.45 }
.content table { border-collapse: collapse; display: block; overflow-x: auto; font-size: .88em }
.content td, .content th { border: .5px solid var(--line); padding: .35em .6em }
.content hr { border: 0; border-top: .5px solid var(--line); margin: 2em 0 }
@media (prefers-reduced-motion: reduce) { .bar { transition: none } button:active { transform: none } }
`

/** Opens the reader view; returns `open`, `none` (no article found) or `already`. Evaluated as an expression. */
function openScript(prefs: ReaderPrefs, labels: Record<string, string>): string {
  return `(() => {
  if (window.__signetReader) return 'already'
  ${READABILITY}
  const PREFS = ${JSON.stringify(prefs)}
  const L = ${JSON.stringify(labels)}
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
      else if (/^(href|src|xlink:href|action|formaction|poster)$/.test(name) && /^\\s*(javascript|vbscript|data:text\\/html)/i.test(attr.value)) el.removeAttribute(attr.name)
    }
  })

  const host = document.createElement('signet-reader')
  host.style.cssText = 'all: initial; position: fixed; inset: 0; z-index: 2147483647; display: block'
  const shadow = host.attachShadow({ mode: 'closed' })
  const style = document.createElement('style')
  style.textContent = ${JSON.stringify(CSS)}
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
})()`
}

/** Validates preferences coming from the page. */
function cleanPrefs(value: unknown): ReaderPrefs | null {
  if (!value || typeof value !== 'object') return null
  const v = value as Partial<ReaderPrefs>
  const size = Number(v.size)
  return {
    font: v.font === 'sans' ? 'sans' : 'serif',
    size: Number.isFinite(size) ? Math.min(28, Math.max(14, Math.round(size))) : DEFAULT_READER.size,
    theme: v.theme === 'light' || v.theme === 'sepia' || v.theme === 'dark' ? v.theme : 'auto',
    width: v.width === 'narrow' || v.width === 'wide' ? v.width : 'normal'
  }
}

/** Address without its anchor. */
function pageKey(url: string): string {
  try {
    const u = new URL(url)
    return u.origin + u.pathname + u.search
  } catch {
    return url
  }
}

/** Reader mode: the page's article, without the rest, in a view laid over the page (its address and state do not change). */
export class Reader {
  /** Token of the examination and of the reading in progress per tab: a navigation invalidates them. */
  private runs = new Map<string, number>()
  private timers = new Map<string, NodeJS.Timeout>()
  private seq = 0
  /** Address without anchor of the page being read, per tab: only another page closes the reading. */
  private bases = new Map<string, string>()
  /** Last examination per tab: page (origin + path) and date, to avoid re-examining on every pushState. */
  private probed = new Map<string, { key: string; at: number }>()

  constructor(private b: Browser) {}

  private contents(tabId: string): WebContents | null {
    const wc = this.b.views.viewOf(tabId)?.webContents
    return wc && !wc.isDestroyed() ? wc : null
  }

  /** Examines the loaded page: readable article? foreign language? (updates the tab). */
  async probe(tabId: string, wc: WebContents): Promise<void> {
    const url = wc.getURL()
    if (!/^(https?|file):/i.test(url)) {
      this.b.broadcast.patchTab(tabId, { reader: undefined, foreignLang: undefined }, { soft: true })
      return
    }
    const run = this.runs.get(tabId)
    this.probed.set(tabId, { key: pageKey(url), at: Date.now() })
    try {
      const res = (await wc.executeJavaScriptInIsolatedWorld(WORLD, [{ code: PROBE }])) as { readerable: boolean; lang: string } | undefined
      if (wc.isDestroyed() || wc.getURL() !== url || this.runs.get(tabId) !== run || !res) return
      const tab = this.b.store.state.tabs[tabId]
      if (!tab) return
      this.b.broadcast.patchTab(
        tabId,
        {
          reader: tab.reader === 'on' ? 'on' : res.readerable ? 'available' : undefined,
          foreignLang: this.b.translator.foreign(res.lang)
        },
        { soft: true }
      )
    } catch {
      // page gone in the meantime
    }
  }

  /** Main frame navigation: the page changes, the reading closes by itself. */
  onNavigate(tabId: string): void {
    this.runs.set(tabId, ++this.seq)
    const timer = this.timers.get(tabId)
    if (timer) clearTimeout(timer)
    this.timers.delete(tabId)
    const tab = this.b.store.state.tabs[tabId]
    if (tab && (tab.reader || tab.foreignLang)) this.b.broadcast.patchTab(tabId, { reader: undefined, foreignLang: undefined }, { soft: true })
  }

  /** In-page navigation (single-page site): the content changes under the reading, which is closed, then we re-examine. */
  onNavigateInPage(tabId: string, wc: WebContents): void {
    if (this.b.store.state.tabs[tabId]?.reader === 'on') {
      // Simple anchor change: the reading stays open.
      if (wc.isDestroyed() || this.bases.get(tabId) === wc.getURL().split('#')[0]) return
      this.close(tabId)
    }
    // Same address up to the fragment as the last examination: the content has not changed, no need to re-examine.
    const last = this.probed.get(tabId)
    if (!wc.isDestroyed() && last && last.key === pageKey(wc.getURL())) return
    const timer = this.timers.get(tabId)
    if (timer) clearTimeout(timer)
    const delay = Math.max(IN_PAGE_DELAY, last ? last.at + IN_PAGE_MIN_GAP - Date.now() : 0)
    this.timers.set(
      tabId,
      setTimeout(() => {
        this.timers.delete(tabId)
        if (!wc.isDestroyed()) void this.probe(tabId, wc)
      }, delay)
    )
  }

  /** The tab no longer has a view: forget what we know about it. */
  forget(tabId: string): void {
    const timer = this.timers.get(tabId)
    if (timer) clearTimeout(timer)
    this.timers.delete(tabId)
    this.runs.delete(tabId)
    this.bases.delete(tabId)
    this.probed.delete(tabId)
  }

  /** True if the tab shows a page where reading can be attempted (even if the examination did not offer it). */
  canRead(tabId: string | null): boolean {
    const wc = tabId ? this.contents(tabId) : null
    return !!wc && /^(https?|file):/i.test(wc.getURL())
  }

  toggle(tabId: string): void {
    const tab = this.b.store.state.tabs[tabId]
    if (!tab) return
    if (tab.reader === 'on') this.close(tabId)
    else void this.open(tabId)
  }

  private close(tabId: string): void {
    const wc = this.contents(tabId)
    if (wc) wc.executeJavaScriptInIsolatedWorld(WORLD, [{ code: CLOSE }]).catch(() => {})
    this.b.broadcast.patchTab(tabId, { reader: 'available' })
  }

  private async open(tabId: string): Promise<void> {
    const wc = this.contents(tabId)
    if (!wc || !this.canRead(tabId)) return
    let run = this.runs.get(tabId)
    const prefs = cleanPrefs(this.b.store.state.settings.reader) ?? DEFAULT_READER
    const labels = {
      smaller: t('reader.smaller'),
      bigger: t('reader.bigger'),
      serif: t('reader.serif'),
      sans: t('reader.sans'),
      themeAuto: t('reader.themeAuto'),
      themeLight: t('reader.themeLight'),
      themeSepia: t('reader.themeSepia'),
      themeDark: t('reader.themeDark'),
      narrow: t('reader.narrow'),
      narrowTitle: t('reader.narrowTitle'),
      normal: t('reader.normal'),
      normalTitle: t('reader.normalTitle'),
      wide: t('reader.wide'),
      wideTitle: t('reader.wideTitle'),
      close: t('reader.close'),
      closeTitle: t('reader.closeTitle'),
      minutes: t('reader.minutes', { n: '{n}' })
    }
    let result: unknown
    try {
      result = await wc.executeJavaScriptInIsolatedWorld(WORLD, [{ code: openScript(prefs, labels) }], true)
      if (result === 'already') {
        // View already present without a listener: close it (the old listener stops) then reopen it.
        run = ++this.seq
        this.runs.set(tabId, run)
        await wc.executeJavaScriptInIsolatedWorld(WORLD, [{ code: CLOSE }])
        result = await wc.executeJavaScriptInIsolatedWorld(WORLD, [{ code: openScript(prefs, labels) }], true)
      }
    } catch (error) {
      console.error('Reader mode:', error)
      return
    }
    if (this.runs.get(tabId) !== run || wc.isDestroyed()) return
    if (result === 'none') {
      this.b.broadcast.patchTab(tabId, { reader: undefined })
      return
    }
    this.bases.set(tabId, wc.getURL().split('#')[0])
    this.b.broadcast.patchTab(tabId, { reader: 'on' })
    if (result === 'open') void this.listen(tabId, wc, run)
  }

  /** Reads the reader view's messages (preferences changed, closing) until it closes. */
  private async listen(tabId: string, wc: WebContents, run: number | undefined): Promise<void> {
    while (!wc.isDestroyed() && this.runs.get(tabId) === run) {
      let msg: { type?: string; prefs?: unknown } | null
      try {
        msg = (await wc.executeJavaScriptInIsolatedWorld(WORLD, [{ code: NEXT }])) as typeof msg
      } catch {
        return
      }
      if (!msg || this.runs.get(tabId) !== run) return
      if (msg.type === 'prefs') {
        const prefs = cleanPrefs(msg.prefs)
        if (prefs) {
          this.b.store.state.settings.reader = prefs
          this.b.broadcast.changed({ layout: false })
        }
      } else if (msg.type === 'close') {
        if (this.b.store.state.tabs[tabId]?.reader === 'on') this.b.broadcast.patchTab(tabId, { reader: 'available' })
        return
      }
    }
  }
}

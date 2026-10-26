// A tab's error pages: load failure (injected into the engine's error page) and crash.
import { t } from '../app/i18n'

export interface PageErrorText {
  title: string
  host: string
  reason: string
  code: string
  retry: string
}

const esc = (s: string) => s.replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c] as string)

/** Plain-language cause from the engine's network error code. */
function reasonOf(code: number): string {
  const key = (k: string) => t(`pageError.reason.${k}` as never)
  if (code === -106) return key('offline')
  if (code === -105 || code === -137) return key('dns')
  if (code === -102) return key('refused')
  if (code === -100 || code === -101 || code === -103 || code === -324) return key('reset')
  if (code === -7 || code === -118) return key('timeout')
  if (code === -109 || code === -21 || code === -104) return key('unreachable')
  if (code <= -200 && code >= -219) return key('certificate')
  return key('other')
}

function hostOf(url: string): string {
  try {
    return new URL(url).host || url
  } catch {
    return url
  }
}

/** Texts of the "load failed" page. */
export function loadErrorText(url: string, code: number): PageErrorText {
  const known = reasonOf(code) !== t('pageError.reason.other' as never)
  return {
    title: t('pageError.title'),
    host: hostOf(url),
    reason: reasonOf(code),
    // The raw code is only used when the cause could not be explained.
    code: known ? '' : t('pageError.code', { code }),
    retry: t('pageError.retry')
  }
}

/** Shared style: light/dark, system font, no animation. */
export const ERROR_CSS = `
:root { color-scheme: light dark; --bg: #fff; --text: rgb(29 29 31); --text-2: rgb(92 92 98); --text-3: rgb(134 134 142); --accent: hsl(250 70% 55%); --on-accent: #fff }
@media (prefers-color-scheme: dark) { :root { --bg: rgb(24 24 27); --text: rgb(236 236 240); --text-2: rgb(165 164 173); --text-3: rgb(120 120 128); --accent: hsl(250 75% 62%) } }
html, body { margin: 0 !important; height: 100% !important; background: var(--bg) !important; color: var(--text) !important; font: 13px/1.5 -apple-system, BlinkMacSystemFont, system-ui, sans-serif !important }
body { display: flex !important; align-items: center; justify-content: center }
.signet-error { max-width: 380px; padding: 24px; text-align: center }
.signet-error h1 { font-size: 20px; font-weight: 600; letter-spacing: -.01em; margin: 0 0 4px; color: var(--text) }
.signet-error .host { color: var(--text-2); margin: 0 0 14px; word-break: break-all }
.signet-error .reason { color: var(--text); margin: 0 0 4px; font-size: 14px }
.signet-error .code { color: var(--text-3); font-size: 12px; margin: 0 }
.signet-error .btn { display: inline-block; margin-top: 20px; padding: 6px 16px; border: 0; border-radius: 8px; background: var(--accent); color: var(--on-accent); font: inherit; font-weight: 500; cursor: default; text-decoration: none }
.signet-error .btn:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px }
`

/** Script that replaces the content of the engine's error page (DOM API only, no `innerHTML`). */
export function injectionScript(text: PageErrorText, css: string): string {
  return `(() => {
  if (location.protocol !== 'chrome-error:') return
  const d = ${JSON.stringify(text)}
  const style = document.createElement('style')
  style.textContent = ${JSON.stringify(css)}
  const parent = document.head || document.documentElement
  parent.append(style)
  const el = (tag, cls, txt) => { const e = document.createElement(tag); if (cls) e.className = cls; if (txt) e.textContent = txt; return e }
  document.title = d.host
  const box = el('main', 'signet-error')
  box.append(el('h1', '', d.title), el('p', 'host', d.host), el('p', 'reason', d.reason))
  if (d.code) box.append(el('p', 'code', d.code))
  const btn = el('button', 'btn', d.retry)
  btn.type = 'button'
  btn.addEventListener('click', () => location.reload())
  box.append(btn)
  document.body.className = ''
  document.body.replaceChildren(box)
  btn.focus()
})()`
}

/** Standalone "this page crashed" page (`data:` address, never shown: the tab keeps its original address). */
export function crashDocument(url: string): string {
  const host = hostOf(url)
  const link = /^(https?|file):/i.test(url) ? esc(url) : ''
  return `<!doctype html><html><head><meta charset="utf-8"><title>${esc(host)}</title><style>${ERROR_CSS}</style></head><body><main class="signet-error"><h1>${esc(t('pageError.crash.title'))}</h1><p class="host">${esc(host)}</p><p class="reason">${esc(t('pageError.crash.text'))}</p>${link ? `<a class="btn" href="${link}" autofocus>${esc(t('pageError.crash.retry'))}</a>` : ''}</main></body></html>`
}

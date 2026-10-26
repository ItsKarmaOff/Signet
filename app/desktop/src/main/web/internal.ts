// Internal `signet://` addresses: shortcuts to screens, information pages and the engine's technical pages.
// Only file allowed to know the engine's real addresses (naming exception, like the importers).
import { app, protocol, type Session } from 'electron'
import { join } from 'node:path'
import { arch } from 'node:os'
import type { PaletteResult } from '@shared/types'
import type { Browser } from '../browser/browser'
import { currentLocale, t } from '../app/i18n'
import { noteOfAppUrl, resolveNoteAddress } from '../features/notes/notes'
import { noteAddress } from '@shared/notes'

/** Engine technical pages that really load (checked one by one), exposed as `signet://<name>`. */
export const ENGINE_PAGES = [
  'gpu',
  'media-internals',
  'webrtc-internals',
  'process-internals',
  'accessibility',
  'serviceworker-internals',
  'indexeddb-internals',
  'quota-internals',
  'histograms',
  'tracing',
  'ukm'
] as const
type EnginePage = (typeof ENGINE_PAGES)[number]

const SETTINGS_SECTIONS = [
  'general',
  'appearance',
  'privacy',
  'passwords',
  'backup',
  'import',
  'boosts',
  'routing',
  'extensions',
  'apps',
  'shortcuts',
  'mods',
  'developer',
  'about'
] as const
const LIBRARY_SECTIONS = ['history', 'archive', 'downloads', 'spaces'] as const
const INFO_PAGES = ['urls', 'version'] as const

/** Scheme of the internal addresses. */
const INTERNAL_SCHEME = 'signet'

/** To call before `ready`. */
export function registerInternalScheme(): void {
  protocol.registerSchemesAsPrivileged([
    { scheme: INTERNAL_SCHEME, privileges: { standard: true, secure: true } },
    { scheme: 'signetui', privileges: { standard: true, secure: true, supportFetchAPI: true, codeCache: true } } // interface de l'app (production)
  ])
}

// ───────────────────────── Conversion d'adresses ─────────────────────────

const isEngine = (name: string): name is EnginePage => (ENGINE_PAGES as readonly string[]).includes(name)

/** `signet://name/path` (or `signet:name`) → { name, path }; null if it is not an internal address. */
function parseInternal(url: string): { name: string; path: string } | null {
  const m = /^signet:(?:\/\/)?([^/?#]*)(?:\/([^?#]*))?/i.exec(url.trim())
  return m ? { name: m[1].toLowerCase(), path: (m[2] ?? '').toLowerCase().replace(/\/+$/, '') } : null
}

const engineName = (url: string): EnginePage | null => {
  const m = /^chrome:\/\/([a-z0-9-]+)\/?/i.exec(url)
  const name = m?.[1].toLowerCase()
  return name && isEngine(name) ? name : null
}

/** Displayed address: an engine technical page appears under its `signet://` address. */
export function displayUrl(url: string): string {
  const note = noteOfAppUrl(url)
  if (note) return noteAddress(note.kind, note.id) // note or Easel: the real address is the app's interface
  const name = engineName(url)
  if (name) return `signet://${name}`
  return /^signet:\/\/[^/?#]+\/$/i.test(url) ? url.slice(0, -1) : url // `signet://urls/` → `signet://urls`
}

/** Real address to load for a displayed address (inverse of `displayUrl`). */
export function resolveInternal(url: string): string {
  const note = resolveNoteAddress(url)
  if (note) return note
  const parsed = parseInternal(url)
  return parsed && isEngine(parsed.name) ? `chrome://${parsed.name}` : url
}

/** True for a `signet:` address, the app's interface or an internal engine page. */
export function isInternalUrl(url: string): boolean {
  return /^signet(ui)?:/i.test(url) || /^chrome(-untrusted)?:/i.test(url)
}

/** True for the engine's internal error page (load failed). */
export function isEngineErrorPage(url: string): boolean {
  return /^chrome-error:/i.test(url)
}

/** True if the current page is itself internal (it may point to other internal addresses). */
const isInternalOrigin = (url: string) => /^signet:/i.test(url) || engineName(url) !== null

// ───────────────────────── Shortcuts to screens ─────────────────────────

/** If `url` is a shortcut (Settings, Library), runs it and returns true: no tab opens. */
export function openShortcut(b: Browser, url: string): boolean {
  const p = parseInternal(url)
  if (!p) return false
  const sub = p.path.split('/')[0]
  switch (p.name) {
    case 'settings':
      b.settings.open(sub || undefined)
      return true
    case 'extensions':
    case 'about':
      b.settings.open(p.name)
      return true
    case 'library':
      b.library.open((LIBRARY_SECTIONS as readonly string[]).includes(sub) ? (sub as (typeof LIBRARY_SECTIONS)[number]) : undefined)
      return true
    case 'history':
    case 'downloads':
      b.library.open(p.name)
      return true
    default:
      return false
  }
}

// ───────────────────────── Address list ─────────────────────────

interface Address {
  address: string
  description: string
  family: 'shortcuts' | 'pages' | 'engine'
}

function addresses(): Address[] {
  const list: Address[] = [
    { address: 'signet://settings', description: t('internal.desc.settings'), family: 'shortcuts' },
    ...SETTINGS_SECTIONS.map((s) => ({
      address: `signet://settings/${s}`,
      description: t(`internal.desc.settingsSection.${s}`),
      family: 'shortcuts' as const
    })),
    { address: 'signet://extensions', description: t('internal.desc.extensions'), family: 'shortcuts' },
    { address: 'signet://about', description: t('internal.desc.about'), family: 'shortcuts' },
    { address: 'signet://library', description: t('internal.desc.library'), family: 'shortcuts' },
    ...LIBRARY_SECTIONS.map((s) => ({ address: `signet://library/${s}`, description: t(`internal.desc.librarySection.${s}`), family: 'shortcuts' as const })),
    { address: 'signet://history', description: t('internal.desc.history'), family: 'shortcuts' },
    { address: 'signet://downloads', description: t('internal.desc.downloads'), family: 'shortcuts' },
    { address: 'signet://urls', description: t('internal.desc.urls'), family: 'pages' },
    { address: 'signet://version', description: t('internal.desc.version'), family: 'pages' },
    ...ENGINE_PAGES.map((n) => ({ address: `signet://${n}`, description: t(`internal.desc.engine.${n}`), family: 'engine' as const }))
  ]
  return list
}

/** Command bar suggestions for an input that starts with `signet:`. */
export function internalSuggestions(query: string): PaletteResult[] {
  const q = query
    .trim()
    .toLowerCase()
    .replace(/^signet:(\/\/)?/, 'signet://')
  if (!/^signet:\/\//.test(q)) return []
  return addresses()
    .filter((a) => a.address.startsWith(q))
    .map((a) => ({ kind: 'url' as const, id: `url:${a.address}`, title: a.address, subtitle: a.description, url: a.address }))
}

// ───────────────────────── Navigation security ─────────────────────────

/**
 * Filters a navigation initiated by a page (link, `location.href`, redirect, `window.open`).
 * Returns true if the address is internal (the navigation is then handled here: refused or redirected);
 * `navigate` loads a real address in the current tab.
 */
export function guardNavigation(b: Browser, from: string, target: string, navigate: (real: string) => void): boolean {
  if (!isInternalUrl(target)) return false
  if (!isInternalOrigin(from)) return true // ordinary web page: refused
  if (openShortcut(b, target)) return true
  const p = parseInternal(target)
  if (p && ((INFO_PAGES as readonly string[]).includes(p.name) || isEngine(p.name)) && !p.path) navigate(resolveInternal(target))
  else if (p) navigate('signet://not-found')
  return true
}

// ───────────────────────── Pages d'information ─────────────────────────

const esc = (s: string) => s.replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c] as string)

const CSS = `
:root { color-scheme: light dark; --bg: rgb(246 246 248); --surface: #fff; --border: rgb(0 0 0 / .09); --text: rgb(29 29 31); --text-2: rgb(92 92 98); --accent: hsl(250 70% 55%); }
@media (prefers-color-scheme: dark) { :root { --bg: rgb(24 24 27); --surface: rgb(31 31 35); --border: rgb(255 255 255 / .09); --text: rgb(236 236 240); --text-2: rgb(165 164 173); --accent: hsl(250 75% 70%); } }
* { box-sizing: border-box }
body { margin: 0; background: var(--bg); color: var(--text); font: 13px/1.5 -apple-system, BlinkMacSystemFont, system-ui, sans-serif; }
main { max-width: 760px; margin: 0 auto; padding: 40px 24px 64px }
h1 { font-size: 28px; margin: 0 0 4px; letter-spacing: -.01em }
h2 { font-size: 12px; font-weight: 600; text-transform: uppercase; letter-spacing: .04em; color: var(--text-2); margin: 28px 0 8px }
p.lead { color: var(--text-2); margin: 0 0 8px; font-size: 14px }
.card { background: var(--surface); border: .5px solid var(--border); border-radius: 12px; overflow: hidden }
.row { display: flex; gap: 16px; padding: 9px 14px; border-top: .5px solid var(--border); align-items: baseline }
.row:first-child { border-top: 0 }
.row a, .row .k { flex: 0 0 280px; font-family: ui-monospace, 'SF Mono', Menlo, monospace; font-size: 12px }
.row .k { flex-basis: 190px; font-family: inherit; color: var(--text-2) }
.row .v { word-break: break-word; user-select: text }
.row .d { color: var(--text-2) }
a { color: var(--accent); text-decoration: none }
a:hover { text-decoration: underline }
`

function document(title: string, body: string): string {
  return `<!doctype html><html lang="${currentLocale()}"><head><meta charset="utf-8"><title>${esc(title)}</title><meta name="viewport" content="width=device-width,initial-scale=1"><style>${CSS}</style></head><body><main>${body}</main></body></html>`
}

function urlsPage(): string {
  const groups: [Address['family'], string][] = [
    ['shortcuts', t('internal.urls.shortcuts')],
    ['pages', t('internal.urls.pages')],
    ['engine', t('internal.urls.engine')]
  ]
  const all = addresses()
  const sections = groups
    .map(([family, label]) => {
      const rows = all
        .filter((a) => a.family === family)
        .map((a) => `<div class="row"><a href="${esc(a.address)}">${esc(a.address)}</a><span class="d">${esc(a.description)}</span></div>`)
        .join('')
      return `<h2>${esc(label)}</h2><div class="card">${rows}</div>`
    })
    .join('')
  return document(t('internal.urls.title'), `<h1>${esc(t('internal.urls.title'))}</h1><p class="lead">${esc(t('internal.urls.lead'))}</p>${sections}`)
}

function versionPage(b: Browser, ses: Session): string {
  const v = process.versions
  const rows: [string, string][] = [
    [t('internal.version.app'), `${app.getName()} ${app.getVersion()}`],
    ['Electron', v.electron],
    [t('internal.version.engine'), v.chrome],
    ['Node.js', v.node],
    ['V8', v.v8],
    [
      t('internal.version.os'),
      `${process.platform === 'darwin' ? 'macOS' : process.platform === 'win32' ? 'Windows' : 'Linux'} ${process.getSystemVersion()} (${arch()})`
    ],
    [t('internal.version.uiLanguage'), currentLocale()],
    [t('internal.version.siteLanguages'), b.store.state.settings.languages.join(', ')],
    [t('internal.version.userAgent'), ses.getUserAgent()],
    [t('internal.version.profile'), app.getPath('userData')],
    [t('internal.version.extensions'), join(app.getPath('userData'), 'Extensions')]
  ]
  const html = rows.map(([k, val]) => `<div class="row"><span class="k">${esc(k)}</span><span class="v">${esc(val)}</span></div>`).join('')
  return document(
    t('internal.version.title'),
    `<h1>${esc(t('internal.version.title'))}</h1><p class="lead">${esc(app.getName())} ${esc(app.getVersion())}</p><h2>&nbsp;</h2><div class="card">${html}</div>`
  )
}

function notFoundPage(): string {
  return document(
    t('internal.notFound.title'),
    `<h1>${esc(t('internal.notFound.title'))}</h1><p class="lead">${esc(t('internal.notFound.text'))}</p><p><a href="signet://urls">signet://urls</a></p>`
  )
}

/** Serves `signet://urls`, `signet://version` and the "not found" page on a session. */
export function attachInternal(ses: Session, b: Browser): void {
  const handler = (req: Request): Response => {
    // No embedding in another page: only document navigations are served.
    const dest = req.headers.get('sec-fetch-dest')
    const headers = {
      'content-type': 'text/html; charset=utf-8',
      'content-security-policy': "default-src 'none'; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'"
    }
    if (dest && dest !== 'document') return new Response('', { status: 403 })
    const p = parseInternal(req.url)
    const name = p?.name === 'urls' || p?.name === 'version' ? p.name : null
    if (!name || p?.path) return new Response(notFoundPage(), { status: 404, headers })
    return new Response(name === 'urls' ? urlsPage() : versionPage(b, ses), { headers })
  }
  ses.protocol.handle(INTERNAL_SCHEME, handler)
}

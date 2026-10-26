// Sites installed as apps (Chrome-style): a separate window, without tabs or sidebar,
// and a launcher in ~/Applications/Signet Apps (Launchpad, Spotlight, Dock).
import {
  app,
  BrowserWindow,
  dialog,
  nativeImage,
  screen,
  shell,
  type NativeImage,
  type Rectangle,
  type Session,
  type WebContents,
  type WebContentsView
} from 'electron'
import { IS_MAC, windowChrome } from '../app/platform'
import { execFile } from 'node:child_process'
import { randomUUID } from 'node:crypto'
import { existsSync, readFileSync } from 'node:fs'
import { chmod, mkdir, mkdtemp, rename, rm, utimes, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { promisify } from 'node:util'
import type { OverlayPayload, WebAppInfo, WebAppsInfo, WebAppState } from '@shared/types'
import type { Browser } from '../browser/browser'
import { AtomicWriter } from '../core/atomic'
import { newTab } from '../core/store'
import { isIsolated, sessionFor } from '../web/profiles'
import { denyWindowOpen } from '../web/guard'
import { loadRoute, preloadPath } from '../core/urls'
import { t } from '../app/i18n'
import { pushUi } from '../ipc/push'

const run = promisify(execFile)

/** Isolated world of the script that reads the page's manifest and icons (see boost, media, reader, translate). */
const WORLD = 1005
/** Height of an app window's title bar (route #webapp). */
export const WEBAPP_BAR = 38
const CASCADE = 24
/** Signet's bundle identifier (package.json, build.appId). */
const MAIN_BUNDLE_ID = 'dev.signet.app'
/** Prefix of launcher identifiers: also used to recognize our own before deleting them. */
const BUNDLE_PREFIX = `${MAIN_BUNDLE_ID}.webapp.`
/** Launch argument (development version: the launcher starts Electron, which hands over to the open instance). */
export const LAUNCH_ARG = '--signet-webapp='
/** Launch address (packaged version: `open` hands it back to Signet, open or not). */
const LAUNCH_URL = /^signet:\/\/webapp\/([0-9a-f-]{36})\/?$/i
const MAX_ICON_BYTES = 5_000_000
const MAX_MANIFEST_BYTES = 512_000
const MAX_NAME = 60

interface WebApp {
  id: string
  name: string
  /** Address opened at launch. */
  startUrl: string
  /** Prefix of addresses that stay "in the app"; beyond it, the bar shows the page's domain. */
  scope: string
  /** Isolated space whose cookies the app shares; null = common profile. */
  spaceId: string | null
  /** Site color (manifest or `meta theme-color`), before the page provides its own. */
  themeColor?: string
  bounds?: Rectangle
  /** Launcher name in the apps folder (without `.app`). */
  bundle: string
  installedAt: number
}

interface AppWindow {
  appId: string
  win: BrowserWindow
  tabId: string
  view: WebContentsView
  /** Last state sent to the bar (JSON). */
  lastSent: string
}

/** What the page inspection found, while waiting for the overlay's answer. */
interface Proposal {
  name: string
  startUrl: string
  scope: string
  spaceId: string | null
  themeColor?: string
  icon: string | null
  maskable: boolean
}

interface PageInfo {
  manifest: string | null
  icons: { href: string; sizes: string; rel: string; type: string }[]
  appName: string
  themeColor: string
  title: string
}

interface Manifest {
  name?: unknown
  short_name?: unknown
  start_url?: unknown
  scope?: unknown
  theme_color?: unknown
  icons?: unknown
}

/** Reads the manifest, declared icons and app name from the page (isolated world: the page sees nothing). */
const PAGE_SCRIPT = `(() => {
  const abs = (h) => { try { return new URL(h, document.baseURI).href } catch { return '' } }
  const icons = [...document.querySelectorAll('link[rel][href]')]
    .filter((l) => /(^|\\s)(icon|apple-touch-icon|apple-touch-icon-precomposed)(\\s|$)/i.test(l.rel))
    .slice(0, 40)
    .map((l) => ({ href: abs(l.getAttribute('href')), sizes: l.getAttribute('sizes') || '', rel: l.rel.toLowerCase(), type: l.type || '' }))
  const manifest = document.querySelector('link[rel~="manifest"][href]')
  const meta = (n) => (document.querySelector('meta[name="' + n + '"]')?.content || '').trim()
  return {
    manifest: manifest ? abs(manifest.getAttribute('href')) : null,
    icons,
    appName: meta('application-name') || meta('apple-mobile-web-app-title'),
    themeColor: meta('theme-color'),
    title: document.title || ''
  }
})()`

const isHttp = (url: string) => /^https?:\/\//i.test(url)
const str = (v: unknown, max = 2048) => (typeof v === 'string' ? v.slice(0, max) : '')

/** Simple CSS color (#rgb, #rrggbb, rgb(…), hsl(…)); the rest is ignored. */
function cleanColor(v: unknown): string | undefined {
  const c = str(v, 64).trim()
  return /^#[0-9a-f]{3,8}$/i.test(c) || /^(rgb|hsl)a?\([\d\s.,%/+-]+\)$/i.test(c) ? c : undefined
}

export function cleanAppName(v: unknown): string {
  return str(v, 400)
    .replace(/\s+/g, ' ')
    .replace(/\p{Cc}/gu, '')
    .trim()
    .slice(0, MAX_NAME)
}

/** Absolute address resolved against `base`, or null. */
function resolve(href: unknown, base: string): string | null {
  try {
    return new URL(str(href), base).href
  } catch {
    return null
  }
}

function originOf(url: string): string {
  try {
    return new URL(url).origin
  } catch {
    return ''
  }
}

/** Address without fragment, normalized (host's trailing slash included). */
function normalize(url: string): string {
  try {
    const u = new URL(url)
    u.hash = ''
    return u.href
  } catch {
    return url
  }
}

function inScope(a: WebApp, url: string): boolean {
  return normalize(url).startsWith(a.scope)
}

const xml = (s: string) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;').replace(/'/g, '&apos;')
const shq = (s: string) => `'${s.replace(/'/g, `'\\''`)}'`

/** Launcher file name: without `/`, `:` or leading dot. */
function fileName(name: string): string {
  return (
    name
      .replace(/[/:\\]/g, '-')
      .replace(/^\.+/, '')
      .trim() || 'App'
  )
}

/** Useful side of a declared icon: `sizes` (the largest), `any` or SVG = vector. */
function declaredSize(sizes: string, url: string, type: string): number {
  if (/\bany\b/i.test(sizes) || /svg/i.test(type) || /\.svg([?#]|$)/i.test(url)) return 1024
  let best = 0
  for (const m of sizes.matchAll(/(\d+)\s*x\s*(\d+)/gi)) best = Math.max(best, Math.min(Number(m[1]), Number(m[2])))
  return best
}

/** Size rank: above 512, the smallest (sharp enough, lighter); below, the largest. */
const rank = (size: number) => (size >= 512 ? 100_000 - size : size)

interface Candidate {
  url: string
  size: number
  maskable: boolean
}

/** Reads at most `max` bytes of an address with the tab's session (cookies included), in 5 s at most. */
async function fetchLimited(ses: Session, url: string, max: number): Promise<{ body: Buffer; type: string } | null> {
  if (!isHttp(url)) return null
  try {
    const res = await ses.fetch(url, { signal: AbortSignal.timeout(5000) })
    if (!res.ok || !res.body) return null
    if (Number(res.headers.get('content-length') ?? 0) > max) return null
    const reader = res.body.getReader()
    const chunks: Uint8Array[] = []
    let size = 0
    for (;;) {
      const { done, value } = await reader.read()
      if (done) break
      size += value.byteLength
      if (size > max) {
        void reader.cancel().catch(() => {})
        return null
      }
      chunks.push(value)
    }
    return { body: Buffer.concat(chunks), type: (res.headers.get('content-type') ?? '').split(';')[0].trim().toLowerCase() }
  } catch {
    return null
  }
}

const EXT_TYPES: Record<string, string> = {
  svg: 'image/svg+xml',
  png: 'image/png',
  ico: 'image/x-icon',
  jpg: 'image/jpeg',
  jpeg: 'image/jpeg',
  webp: 'image/webp',
  gif: 'image/gif'
}

/** Icon as a data URL (for the overlay canvas, which must not be "tainted" by another origin). */
async function iconData(ses: Session, url: string): Promise<string | null> {
  if (/^data:image\/[a-z0-9.+-]+[;,]/i.test(url)) return url.length < MAX_ICON_BYTES * 1.4 ? url : null
  const got = await fetchLimited(ses, url, MAX_ICON_BYTES)
  if (!got || !got.body.length) return null
  const ext = /\.([a-z]{3,4})(?:[?#]|$)/i.exec(new URL(url).pathname)?.[1]?.toLowerCase() ?? ''
  const type = got.type.startsWith('image/') ? got.type : !got.type || got.type === 'application/octet-stream' ? EXT_TYPES[ext] : undefined
  return type ? `data:${type};base64,${got.body.toString('base64')}` : null
}

/** 1024 px square (the overlay sends one; what arrives is bounded anyway). */
function master1024(image: NativeImage): NativeImage {
  const { width, height } = image.getSize()
  return width === 1024 && height === 1024 ? image : image.resize({ width: 1024, height: 1024, quality: 'best' })
}

/** .icns file from the 1024 px image (`iconutil`, present on every Mac). */
async function writeIcns(image: NativeImage, dest: string): Promise<void> {
  const dir = await mkdtemp(join(tmpdir(), 'signet-icon-'))
  try {
    const set = join(dir, 'app.iconset')
    await mkdir(set)
    const sizes: [string, number][] = [
      ['16x16', 16],
      ['16x16@2x', 32],
      ['32x32', 32],
      ['32x32@2x', 64],
      ['128x128', 128],
      ['128x128@2x', 256],
      ['256x256', 256],
      ['256x256@2x', 512],
      ['512x512', 512],
      ['512x512@2x', 1024]
    ]
    for (const [name, size] of sizes) await writeFile(join(set, `icon_${name}.png`), image.resize({ width: size, height: size, quality: 'best' }).toPNG())
    await run('/usr/bin/iconutil', ['-c', 'icns', set, '-o', dest])
  } finally {
    await rm(dir, { recursive: true, force: true }).catch(() => {})
  }
}

/** Retained size and position, if they still fit on a connected screen. */
function onScreen(bounds: Rectangle | undefined): Rectangle | undefined {
  if (!bounds) return undefined
  const area = screen.getDisplayMatching(bounds).workArea
  // A size larger than the current screen is brought back to the work area.
  bounds = { ...bounds, width: Math.min(bounds.width, area.width), height: Math.min(bounds.height, area.height) }
  const visible =
    bounds.x < area.x + area.width - 80 &&
    bounds.x + bounds.width > area.x + 80 &&
    bounds.y >= area.y - 10 &&
    bounds.y + bounds.height <= area.y + area.height + 10
  return visible ? bounds : { ...bounds, x: area.x + Math.round((area.width - bounds.width) / 2), y: area.y + Math.round((area.height - bounds.height) / 2) }
}

export class WebApps {
  private apps: WebApp[] = []
  private file = join(app.getPath('userData'), 'webapps.json')
  private iconDir = join(app.getPath('userData'), 'webapps')
  private writer = new AtomicWriter(this.file, () => JSON.stringify({ version: 1, apps: this.apps }))
  private windows = new Map<BrowserWindow, AppWindow>()
  private owner = new Map<string, AppWindow>()
  /** 64 px icons (data URL), for the bar and Settings. */
  private small = new Map<string, string>()
  private pending: (Proposal & { token: string; tabId: string }) | null = null
  private seq = 0

  constructor(private b: Browser) {
    try {
      const raw = JSON.parse(readFileSync(this.file, 'utf8')) as { apps?: unknown }
      if (Array.isArray(raw.apps)) this.apps = raw.apps.map(cleanApp).filter((a): a is WebApp => a !== null)
    } catch {
      // no app installed
    }
  }

  /** Launchers folder; `SIGNET_WEBAPPS_DIR` overrides it (development checks). */
  get folder(): string {
    return process.env.SIGNET_WEBAPPS_DIR || join(app.getPath('home'), 'Applications', 'Signet Apps')
  }

  flush(): void {
    this.writer.flushSync()
  }

  private save(): void {
    this.writer.request()
  }

  private find(id: string): WebApp | undefined {
    return this.apps.find((a) => a.id === id)
  }

  /** Installed apps, for the command bar. */
  names(): { id: string; name: string }[] {
    return this.apps.map((a) => ({ id: a.id, name: a.name }))
  }

  /** Installed app whose site contains this address (most specific scope), for "Open in app". */
  appFor(url: string): { id: string; name: string } | null {
    if (!isHttp(url)) return null
    const a = this.apps.filter((x) => inScope(x, url)).sort((x, y) => y.scope.length - x.scope.length)[0]
    return a ? { id: a.id, name: a.name } : null
  }

  // ───────────────────────── Windows ─────────────────────────

  owns(tabId: string): boolean {
    return this.owner.has(tabId)
  }

  has(win: BrowserWindow | null): boolean {
    return !!win && this.windows.has(win)
  }

  tabIdOf(win: BrowserWindow | null): string | null {
    return (win && this.windows.get(win)?.tabId) || null
  }

  private fromBar(wc: WebContents): AppWindow | undefined {
    const win = BrowserWindow.fromWebContents(wc)
    const entry = win ? this.windows.get(win) : undefined
    return entry && entry.win.webContents === wc ? entry : undefined
  }

  ownsBar(wc: WebContents): boolean {
    return !!this.fromBar(wc)
  }

  tabIdForBar(wc: WebContents): string | null {
    return this.fromBar(wc)?.tabId ?? null
  }

  /** Launcher, Settings, command bar: the app's window brought to the front, or a new one on the start address. */
  launch(id: string): void {
    const a = this.find(id)
    if (!a) {
      this.b.window.show()
      void dialog.showMessageBox({ type: 'info', message: t('webapps.missing'), detail: t('webapps.missingDetail') })
      return
    }
    const open = [...this.windows.values()].filter((w) => w.appId === id)
    const last = open[open.length - 1]?.win
    if (last) {
      if (last.isMinimized()) last.restore()
      last.show()
      last.focus()
      return
    }
    this.openWindow(a, a.startUrl)
  }

  /** Received launch address (`signet://webapp/<id>` or `--signet-webapp=<id>`); false if it is not one. */
  launchFrom(arg: string): boolean {
    const id = LAUNCH_URL.exec(arg)?.[1] ?? (arg.startsWith(LAUNCH_ARG) ? arg.slice(LAUNCH_ARG.length) : null)
    if (!id) return false
    this.launch(id.toLowerCase())
    return true
  }

  private openWindow(a: WebApp, url: string): BrowserWindow {
    const { store } = this.b
    const tab = newTab(url)
    store.state.tabs[tab.id] = tab
    store.detached.add(tab.id)
    // Same session as the space where the app was installed (isolated profile), otherwise the common profile.
    this.b.views.inheritSession(tab.id, sessionFor(a.spaceId))

    const siblings = [...this.windows.values()].filter((w) => w.appId === a.id)
    const prev = siblings[siblings.length - 1]?.win.getBounds()
    const base = onScreen(prev ? { ...prev, x: prev.x + CASCADE, y: prev.y + CASCADE } : a.bounds)
    const win = new BrowserWindow({
      width: base?.width ?? 1060,
      height: base?.height ?? 760,
      x: base?.x,
      y: base?.y,
      minWidth: 380,
      minHeight: 300,
      title: a.name,
      ...windowChrome({ lights: { x: 14, y: Math.round((WEBAPP_BAR - 14) / 2) }, background: '#ffffff' }),
      webPreferences: { preload: preloadPath, sandbox: true, contextIsolation: true, nodeIntegration: false }
    })
    // The window title (Window menu, Mission Control) is the page's, set by `push`.
    win.on('page-title-updated', (e) => e.preventDefault())

    const view = this.b.views.ensureView(tab.id, win)
    view.setBorderRadius(0)
    win.contentView.addChildView(view)
    const entry: AppWindow = { appId: a.id, win, tabId: tab.id, view, lastSent: '' }
    this.windows.set(win, entry)
    this.owner.set(tab.id, entry)
    const place = () => {
      const { width, height } = win.getContentBounds()
      view.setBounds({ x: 0, y: WEBAPP_BAR, width, height: Math.max(0, height - WEBAPP_BAR) })
    }
    place()
    win.on('resize', place)
    const remember = () => {
      const current = this.find(a.id)
      if (!current || win.isFullScreen() || win.isMaximized() || win.isMinimized()) return
      current.bounds = win.getBounds()
      this.save()
    }
    win.on('resized', remember)
    win.on('moved', remember)
    // The bar has no field: the keyboard always goes to the page.
    win.on('focus', () => {
      if (!entry.view.webContents.isDestroyed()) entry.view.webContents.focus()
    })
    win.on('closed', () => {
      // Closing the window forgets the tab (unless it just moved into Signet).
      if (this.windows.get(win) !== entry) return
      this.windows.delete(win)
      this.owner.delete(entry.tabId)
      this.b.views.forget(entry.tabId)
      this.notify()
    })
    win.webContents.on('did-start-loading', () => (entry.lastSent = ''))
    denyWindowOpen(win.webContents)
    loadRoute(win.webContents, '#webapp')
    this.notify()
    return win
  }

  /**
   * Link opened from an app in a new window (`target=_blank`, `window.open`): within the app's site,
   * another window of the app; elsewhere, a Signet tab.
   */
  openLink(tabId: string, url: string): void {
    const entry = this.owner.get(tabId)
    const a = entry && this.find(entry.appId)
    if (a && inScope(a, url)) this.openWindow(a, url)
    else this.openInBrowser(url)
  }

  /** Link opened "in a new tab" from an app: in the main window. */
  openInBrowser(url: string): void {
    this.b.window.show()
    this.b.tabs.open(url, { activate: true })
  }

  // ───────────────────────── Bar (route #webapp) ─────────────────────────

  ready(wc: WebContents): void {
    const entry = this.fromBar(wc)
    if (!entry) return
    entry.lastSent = ''
    this.push(entry)
  }

  home(wc: WebContents): void {
    const entry = this.fromBar(wc)
    const a = entry && this.find(entry.appId)
    if (entry && a) this.b.tabs.loadUrl(entry.tabId, a.startUrl)
  }

  copyUrl(wc: WebContents): void {
    this.b.tabs.copyUrl(this.tabIdForBar(wc))
  }

  /** "Open in Signet": the page moves into a tab of the main window, the app window closes. */
  openInMain(wc: WebContents): void {
    const entry = this.fromBar(wc)
    if (!entry) return
    this.windows.delete(entry.win)
    this.owner.delete(entry.tabId)
    entry.win.contentView.removeChildView(entry.view)
    entry.view.setBorderRadius(12)
    this.b.tabs.adopt(entry.tabId)
    this.b.window.show()
    entry.win.close()
    this.notify()
  }

  pushStates(): void {
    for (const entry of this.windows.values()) this.push(entry)
  }

  private push(entry: AppWindow): void {
    const tab = this.b.store.state.tabs[entry.tabId]
    const a = this.find(entry.appId)
    const { win } = entry
    if (!tab || !a || win.isDestroyed() || win.webContents.isDestroyed()) return
    const title = tab.title || a.name
    if (win.getTitle() !== title) win.setTitle(title)
    const state: WebAppState = {
      appId: a.id,
      name: a.name,
      icon: this.smallIcon(a.id),
      url: tab.url,
      title: tab.title,
      themeColor: tab.themeColor ?? a.themeColor,
      loading: tab.loading,
      canGoBack: tab.canGoBack,
      inScope: inScope(a, tab.url),
      hue: this.b.space.hue
    }
    const json = JSON.stringify(state)
    if (entry.lastSent === json) return
    entry.lastSent = json
    pushUi(win.webContents, 'webapp:state', state)
  }

  // ───────────────────────── Installation ─────────────────────────

  /**
   * "Install as app…": reads the page's manifest and icons, then offers the installation (`webapp` overlay).
   * If an app already covers the page, the page goes there directly.
   */
  async promptInstall(tabId: string | null): Promise<void> {
    if (!IS_MAC) return // macOS launcher (iconutil, lsregister)
    const id = tabId ?? this.b.space.activeTabId
    const tab = id ? this.b.store.state.tabs[id] : undefined
    if (!id || !tab || !this.b.model.locate(id) || !isHttp(tab.url)) {
      void dialog.showMessageBox({ type: 'info', message: t('webapps.cannotInstall'), detail: t('webapps.cannotInstallDetail') })
      return
    }
    const existing = this.appFor(tab.url)
    if (existing) return this.moveToApp(id, existing.id)
    this.b.window.show()
    if (!this.b.model.displayedTabIds().includes(id)) this.b.tabs.activate(id)
    const seq = ++this.seq
    const proposal = await this.inspect(id)
    if (seq !== this.seq || !this.b.store.state.tabs[id]) return
    const token = randomUUID()
    this.pending = { ...proposal, token, tabId: id }
    const payload: Omit<Extract<OverlayPayload, { mode: 'webapp' }>, 'mode' | 'card'> = {
      token,
      name: proposal.name,
      url: proposal.startUrl,
      icon: proposal.icon,
      maskable: proposal.maskable,
      hue: this.b.space.hue
    }
    if (!this.b.overlay.openWebApp(id, payload)) this.pending = null
  }

  /** Opens a tab's page in an installed app; a today tab closes (the page changed window). */
  moveToApp(tabId: string, appId: string): void {
    const tab = this.b.store.state.tabs[tabId]
    const a = this.find(appId)
    if (!tab || !a) return
    this.openWindow(a, inScope(a, tab.url) ? tab.url : a.startUrl)
    if (this.b.model.locate(tabId)?.zone === 'today') this.b.tabs.close(tabId)
  }

  private async inspect(tabId: string): Promise<Proposal> {
    const tab = this.b.store.state.tabs[tabId]
    const loc = this.b.model.locate(tabId)
    const spaceId = loc?.space && isIsolated(loc.space) ? loc.space.id : null
    const ses = this.b.views.sessionOfTab(tabId)
    const wc = this.b.views.viewOf(tabId)?.webContents
    let page: PageInfo | null = null
    if (wc && !wc.isDestroyed()) {
      // Tab reloading (it was asleep): give it 3 s to declare its manifest and icons.
      if (wc.isLoading()) {
        await new Promise<void>((done) => {
          const timer = setTimeout(done, 3000)
          wc.once('did-stop-loading', () => {
            clearTimeout(timer)
            done()
          })
        })
      }
      if (!wc.isDestroyed()) page = cleanPage(await wc.executeJavaScriptInIsolatedWorld(WORLD, [{ code: PAGE_SCRIPT }]).catch(() => null))
    }
    const pageUrl = wc && !wc.isDestroyed() && isHttp(wc.getURL()) ? wc.getURL() : tab.url
    const origin = originOf(pageUrl)
    const manifestUrl = page?.manifest && isHttp(page.manifest) ? page.manifest : null
    const manifest = manifestUrl ? await readManifest(ses, manifestUrl) : null

    // Start address and manifest scope, if they stay on the page's site; otherwise the page and its origin.
    const declaredStart = manifest && manifestUrl ? resolve(manifest.start_url, manifestUrl) : null
    const startUrl = declaredStart && originOf(declaredStart) === origin ? normalize(declaredStart) : normalize(pageUrl)
    const declaredScope = manifest && manifestUrl && manifest.scope ? resolve(manifest.scope, manifestUrl) : null
    const scope = declaredScope && originOf(declaredScope) === origin && startUrl.startsWith(normalize(declaredScope)) ? normalize(declaredScope) : `${origin}/`

    const host = (() => {
      try {
        return new URL(pageUrl).hostname.replace(/^www\./, '')
      } catch {
        return ''
      }
    })()
    const name =
      cleanAppName(manifest?.name) ||
      cleanAppName(manifest?.short_name) ||
      cleanAppName(page?.appName) ||
      cleanAppName(page?.title) ||
      cleanAppName(tab.title) ||
      host ||
      'App'

    // Candidate icons: maskable (they fill the square), then the others, from sharpest to least sharp.
    const masks: Candidate[] = []
    const others: Candidate[] = []
    if (manifest && manifestUrl && Array.isArray(manifest.icons)) {
      for (const raw of manifest.icons.slice(0, 40) as Record<string, unknown>[]) {
        const url = raw && resolve(raw.src, manifestUrl)
        if (!url) continue
        const size = declaredSize(str(raw.sizes, 200), url, str(raw.type, 100))
        const purposes = str(raw.purpose, 100).toLowerCase().split(/\s+/).filter(Boolean)
        if (purposes.includes('maskable')) masks.push({ url, size, maskable: true })
        if (!purposes.length || purposes.includes('any')) others.push({ url, size, maskable: false })
      }
    }
    for (const l of page?.icons ?? []) {
      if (!l.href) continue
      const apple = l.rel.includes('apple-touch-icon')
      const size = declaredSize(l.sizes, l.href, l.type) || (apple ? 180 : 32)
      // Apple icon: a full square, designed to be rounded by the system.
      others.push({ url: l.href, size, maskable: apple })
    }
    if (tab.favicon) others.push({ url: tab.favicon, size: 16, maskable: false })
    if (origin && !others.length) others.push({ url: `${origin}/favicon.ico`, size: 16, maskable: false })
    const order = [
      ...masks.filter((c) => c.size >= 192).sort((x, y) => rank(y.size) - rank(x.size)),
      ...[...others, ...masks.filter((c) => c.size < 192)].sort((x, y) => rank(y.size) - rank(x.size) || Number(y.maskable) - Number(x.maskable))
    ]
    let icon: string | null = null
    let maskable = false
    const tried = new Set<string>()
    for (const c of order) {
      if (tried.has(c.url) || tried.size >= 4) continue
      tried.add(c.url)
      icon = await iconData(ses, c.url)
      if (icon) {
        maskable = c.maskable
        break
      }
    }
    return { name, startUrl, scope, spaceId, themeColor: cleanColor(manifest?.theme_color) ?? cleanColor(page?.themeColor), icon, maskable }
  }

  /** Overlay answer: saves the app, opens its window on the page, then creates its launcher. */
  async install(token: string, name: string, icon: string): Promise<void> {
    const p = this.pending
    if (!p || p.token !== token) return
    this.pending = null
    this.b.overlay.close()
    const image = typeof icon === 'string' && icon.length < 16_000_000 && icon.startsWith('data:image/png;base64,') ? nativeImage.createFromDataURL(icon) : null
    const master = image && !image.isEmpty() ? master1024(image) : null
    const a: WebApp = {
      id: randomUUID(),
      name: cleanAppName(name) || p.name,
      startUrl: p.startUrl,
      scope: p.scope,
      spaceId: p.spaceId,
      themeColor: p.themeColor,
      bundle: '',
      installedAt: Date.now()
    }
    a.bundle = this.freeBundle(a.name, a.id)
    this.apps.push(a)
    this.save()
    if (master) {
      await mkdir(this.iconDir, { recursive: true }).catch(() => {})
      await writeFile(this.iconPath(a.id), master.toPNG()).catch(() => {})
    }
    // As in Chrome, the page moves into the app's window.
    if (this.b.store.state.tabs[p.tabId]) this.moveToApp(p.tabId, a.id)
    else this.openWindow(a, a.startUrl)
    await this.writeLauncher(a).catch((e) => console.error('App launcher not created', e))
    this.notify()
  }

  // ───────────────────────── Settings › Apps ─────────────────────────

  list(): WebAppsInfo {
    const { spaces } = this.b.store.state
    const apps: WebAppInfo[] = this.apps.map((a) => {
      const space = a.spaceId ? spaces.find((s) => s.id === a.spaceId) : undefined
      return {
        id: a.id,
        name: a.name,
        startUrl: a.startUrl,
        icon: this.smallIcon(a.id),
        launcher: existsSync(this.bundlePath(a)),
        space: space && isIsolated(space) ? { name: space.name, icon: space.icon } : null,
        windows: [...this.windows.values()].filter((w) => w.appId === a.id).length
      }
    })
    return { apps, folder: this.folder.replace(app.getPath('home'), '~') }
  }

  async rename(id: string, name: string): Promise<void> {
    const a = this.find(id)
    const clean = cleanAppName(name)
    if (!a || !clean || clean === a.name) return
    const old = this.bundlePath(a)
    a.name = clean
    a.bundle = this.freeBundle(clean, id)
    this.save()
    for (const entry of this.windows.values()) if (entry.appId === id) this.push(entry)
    this.notify()
    if (old !== this.bundlePath(a) && this.ownBundle(old, id)) await rm(old, { recursive: true, force: true }).catch(() => {})
    await this.writeLauncher(a).catch((e) => console.error('App launcher not recreated', e))
    this.notify()
  }

  /** Shows the launcher in Finder; recreated first if it was deleted. */
  async reveal(id: string): Promise<void> {
    const a = this.find(id)
    if (!a) return
    if (!existsSync(this.bundlePath(a))) {
      a.bundle = this.freeBundle(a.name, id)
      this.save()
      await this.writeLauncher(a).catch((e) => console.error('App launcher not recreated', e))
      this.notify()
    }
    if (existsSync(this.bundlePath(a))) shell.showItemInFolder(this.bundlePath(a))
  }

  /** Uninstalls: closes its windows, deletes its launcher (if it is really ours) and its icon. */
  async remove(id: string): Promise<void> {
    const a = this.find(id)
    if (!a) return
    for (const entry of [...this.windows.values()]) if (entry.appId === id) entry.win.close()
    this.apps = this.apps.filter((x) => x.id !== id)
    this.small.delete(id)
    this.save()
    this.notify()
    const path = this.bundlePath(a)
    if (this.ownBundle(path, id)) await rm(path, { recursive: true, force: true }).catch(() => {})
    await rm(this.iconPath(id), { force: true }).catch(() => {})
  }

  /** Tells Settings (Apps section) that the list must be re-read. */
  private notify(): void {
    for (const w of BrowserWindow.getAllWindows()) if (!w.isDestroyed() && !w.webContents.isDestroyed()) pushUi(w.webContents, 'webapps:changed')
  }

  // ───────────────────────── Icons and launchers ─────────────────────────

  private iconPath(id: string): string {
    return join(this.iconDir, `${id}.png`)
  }

  private master(id: string): NativeImage | null {
    const image = nativeImage.createFromPath(this.iconPath(id))
    return image.isEmpty() ? null : image
  }

  private smallIcon(id: string): string {
    let url = this.small.get(id)
    if (url === undefined) {
      url = this.master(id)?.resize({ width: 64, height: 64, quality: 'best' }).toDataURL() ?? ''
      this.small.set(id, url)
    }
    return url
  }

  private bundlePath(a: WebApp): string {
    return join(this.folder, `${a.bundle}.app`)
  }

  /** True if the launcher at this location is the one of app `id` (an app that is not ours is never deleted). */
  private ownBundle(path: string, id: string): boolean {
    try {
      return readFileSync(join(path, 'Contents', 'Info.plist'), 'utf8').includes(`<string>${BUNDLE_PREFIX}${id}</string>`)
    } catch {
      return false
    }
  }

  /** Free launcher name: the app's, or followed by "2", "3"… if another app (or file) already has it. */
  private freeBundle(name: string, id: string): string {
    const base = fileName(name)
    for (let n = 1; ; n++) {
      const candidate = n === 1 ? base : `${base} ${n}`
      const taken = this.apps.some((a) => a.id !== id && a.bundle === candidate)
      const path = join(this.folder, `${candidate}.app`)
      if (!taken && (!existsSync(path) || this.ownBundle(path, id))) return candidate
    }
  }

  /** Launcher script: it hands the app's address back to Signet and exits immediately. */
  private script(a: WebApp): string {
    const head = `#!/bin/sh\n# Lanceur d'app Signet (${a.name.replace(/[\r\n]/g, ' ')}).\n`
    if (app.isPackaged) return `${head}exec /usr/bin/open -b ${MAIN_BUNDLE_ID} ${shq(`signet://webapp/${a.id}`)}\n`
    // Development: Electron relaunched with the argument, which hands over to the open instance (second-instance).
    const args = [process.execPath, app.getAppPath(), `${LAUNCH_ARG}${a.id}`]
    return `${head}${args.map(shq).join(' ')} >/dev/null 2>&1 &\n`
  }

  private plist(a: WebApp): string {
    const name = xml(a.name)
    return `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key><string>en</string>
  <key>CFBundleDisplayName</key><string>${name}</string>
  <key>CFBundleName</key><string>${name}</string>
  <key>CFBundleExecutable</key><string>launcher</string>
  <key>CFBundleIconFile</key><string>app</string>
  <key>CFBundleIdentifier</key><string>${BUNDLE_PREFIX}${a.id}</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>1.0</string>
  <key>CFBundleVersion</key><string>1</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>LSUIElement</key><true/>
</dict>
</plist>
`
  }

  /** Writes the launcher into a hidden folder then puts it in place in one go (never a half-written launcher). */
  private async writeLauncher(a: WebApp): Promise<void> {
    const dir = this.folder
    await mkdir(dir, { recursive: true })
    const final = this.bundlePath(a)
    const tmp = join(dir, `.${a.id}.tmp.app`)
    await rm(tmp, { recursive: true, force: true })
    await mkdir(join(tmp, 'Contents', 'MacOS'), { recursive: true })
    await mkdir(join(tmp, 'Contents', 'Resources'), { recursive: true })
    await writeFile(join(tmp, 'Contents', 'Info.plist'), this.plist(a))
    await writeFile(join(tmp, 'Contents', 'PkgInfo'), 'APPL????')
    const exe = join(tmp, 'Contents', 'MacOS', 'launcher')
    await writeFile(exe, this.script(a))
    await chmod(exe, 0o755)
    const image = this.master(a.id)
    if (image) await writeIcns(image, join(tmp, 'Contents', 'Resources', 'app.icns')).catch((e) => console.error('Launcher icon not created', e))
    if (existsSync(final)) {
      if (!this.ownBundle(final, a.id)) throw new Error(`${final} already exists`)
      await rm(final, { recursive: true, force: true })
    }
    await rename(tmp, final)
    // Finder and the Dock cache icons: the launcher's date and LaunchServices refresh them.
    const now = new Date()
    await utimes(final, now, now).catch(() => {})
    const lsregister = '/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister'
    await run(lsregister, ['-f', final]).catch(() => {})
  }
}

function cleanApp(raw: unknown): WebApp | null {
  const x = raw as Partial<WebApp> | null
  if (!x || typeof x.id !== 'string' || !/^[0-9a-f-]{36}$/i.test(x.id)) return null
  const startUrl = str(x.startUrl)
  const scope = str(x.scope)
  const name = cleanAppName(x.name)
  if (!isHttp(startUrl) || !isHttp(scope) || !name) return null
  const b = x.bounds
  const bounds = b && [b.x, b.y, b.width, b.height].every((n) => Number.isFinite(n)) && b.width >= 200 && b.height >= 150 ? b : undefined
  return {
    id: x.id.toLowerCase(),
    name,
    startUrl,
    scope,
    spaceId: typeof x.spaceId === 'string' ? x.spaceId : null,
    themeColor: cleanColor(x.themeColor),
    bounds,
    bundle: typeof x.bundle === 'string' && x.bundle ? fileName(x.bundle) : fileName(name),
    installedAt: Number.isFinite(x.installedAt) ? Number(x.installedAt) : Date.now()
  }
}

function cleanPage(raw: unknown): PageInfo | null {
  const x = raw as Partial<PageInfo> | null
  if (!x || typeof x !== 'object') return null
  const icons = Array.isArray(x.icons)
    ? x.icons.slice(0, 40).map((i) => ({ href: str(i?.href), sizes: str(i?.sizes, 200), rel: str(i?.rel, 100), type: str(i?.type, 100) }))
    : []
  return { manifest: str(x.manifest) || null, icons, appName: str(x.appName, 200), themeColor: str(x.themeColor, 64), title: str(x.title, 400) }
}

async function readManifest(ses: Session, url: string): Promise<Manifest | null> {
  const got = await fetchLimited(ses, url, MAX_MANIFEST_BYTES)
  if (!got) return null
  try {
    const json = JSON.parse(got.body.toString('utf8')) as unknown
    return json && typeof json === 'object' ? (json as Manifest) : null
  } catch {
    return null
  }
}

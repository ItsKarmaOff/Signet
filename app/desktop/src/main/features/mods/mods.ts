// Interface mods: user CSS sheets, applied to all the app's views (never to web pages).
import { app, BrowserWindow, dialog, type WebContents } from 'electron'
import { readFileSync } from 'node:fs'
import { readFile, writeFile } from 'node:fs/promises'
import { randomUUID } from 'node:crypto'
import { basename, join } from 'node:path'
import type { InterfaceMod, ModsInfo } from '@shared/types'
import type { Browser } from '../../browser/browser'
import { AtomicWriter } from '../../core/atomic'
import { isAppUrl } from '../../core/urls'
import { buildAppMenu } from '../../app/menu'
import { t } from '../../app/i18n'
import { sanitizeModCss } from './modscss'
import { pushUi } from '../../ipc/push'

/** Maximum size of a mod (characters): a style sheet, not an archive. */
const MAX_CSS = 200_000
const MAX_NAME = 60
/** Header of an exported file, re-read on import to recover the name. */
const NAME_HEADER = /^\s*\/\*\s*name:\s*(.+?)\s*\*\//i

/** Launch without mods for this session: `--safe-mode` (or SIGNET_SAFE_MODE=1), if a mod makes the interface unusable. */
const SAFE_MODE = process.argv.includes('--safe-mode') || process.env.SIGNET_SAFE_MODE === '1'

export class Mods {
  private mods: InterfaceMod[] = []
  private disabled = false
  private file = join(app.getPath('userData'), 'mods.json')
  private writer = new AtomicWriter(this.file, () => JSON.stringify({ version: 1, disabled: this.disabled, mods: this.mods }))
  /** Tracked interface views and the key of the sheet inserted in each. */
  private views = new Map<WebContents, string | null>()

  constructor(private b: Browser) {
    try {
      const raw = JSON.parse(readFileSync(this.file, 'utf8')) as { disabled?: unknown; mods?: unknown }
      this.disabled = raw.disabled === true
      if (Array.isArray(raw.mods)) this.mods = raw.mods.map(clean).filter((m): m is InterfaceMod => m !== null)
    } catch {
      // no mods yet
    }
    // Any view that displays the app's interface receives the mods on every load (reloads included).
    app.on('web-contents-created', (_e, wc) => {
      wc.on('dom-ready', () => {
        if (!isAppUrl(wc.getURL())) return
        if (!this.views.has(wc)) wc.once('destroyed', () => this.views.delete(wc))
        this.views.set(wc, null) // a navigation removes the inserted sheets
        void this.inject(wc)
      })
    })
  }

  /** Are mods turned off (setting or safe mode)? */
  get off(): boolean {
    return this.disabled || SAFE_MODE
  }

  get allDisabled(): boolean {
    return this.disabled
  }

  info(): ModsInfo {
    return { mods: this.mods, disabled: this.disabled, safeMode: SAFE_MODE }
  }

  /** Applied sheet: the active mods, in list order, without remote loading (even for already stored mods). */
  private sheet(): string {
    if (this.off) return ''
    return this.mods
      .filter((m) => m.enabled && m.css.trim())
      .map((m) => `/* ${m.name.replace(/\*\//g, '')} */\n${sanitizeModCss(m.css)}`)
      .join('\n\n')
  }

  /** Queued injections per view: two quick changes do not remove the same key twice. */
  private queue = new WeakMap<WebContents, Promise<void>>()

  private inject(wc: WebContents): Promise<void> {
    const next = (this.queue.get(wc) ?? Promise.resolve()).then(() => this.injectNow(wc)).catch(() => {})
    this.queue.set(wc, next)
    return next
  }

  private async injectNow(wc: WebContents): Promise<void> {
    if (wc.isDestroyed()) return
    const old = this.views.get(wc)
    if (old) await wc.removeInsertedCSS(old).catch(() => {})
    const css = this.sheet()
    const key = css ? await wc.insertCSS(css).catch(() => null) : null
    if (!wc.isDestroyed()) this.views.set(wc, key)
  }

  /** Reapplies mods everywhere, saves, notifies the Settings section. */
  private changed(): void {
    this.writer.request()
    for (const wc of this.views.keys()) {
      void this.inject(wc)
      if (!wc.isDestroyed()) pushUi(wc, 'mods:changed')
    }
  }

  flush(): void {
    this.writer.flushSync()
  }

  save(id: string | null, name: unknown, css: unknown): string {
    const mod = clean({ id: id ?? randomUUID(), name, css, enabled: true })
    if (!mod) return id ?? ''
    const existing = id ? this.mods.find((m) => m.id === id) : undefined
    if (existing) Object.assign(existing, { name: mod.name, css: mod.css })
    else this.mods.push(mod)
    this.changed()
    return mod.id
  }

  remove(id: string): void {
    this.mods = this.mods.filter((m) => m.id !== id)
    this.changed()
  }

  setEnabled(id: string, enabled: boolean): void {
    const mod = this.mods.find((m) => m.id === id)
    if (!mod) return
    mod.enabled = enabled
    this.changed()
  }

  move(id: string, delta: number): void {
    const i = this.mods.findIndex((m) => m.id === id)
    const j = i + Math.sign(delta)
    if (i < 0 || j < 0 || j >= this.mods.length) return
    ;[this.mods[i], this.mods[j]] = [this.mods[j], this.mods[i]]
    this.changed()
  }

  /** Turns all mods off or back on (Settings, View menu: a way out if a mod gets in the way). */
  setDisabled(disabled: boolean): void {
    if (disabled === this.disabled) return
    this.disabled = disabled
    this.changed()
    buildAppMenu(this.b)
  }

  async importFile(sender: WebContents): Promise<string | null> {
    const win = BrowserWindow.fromWebContents(sender)
    const options: Electron.OpenDialogOptions = { properties: ['openFile'], filters: [{ name: 'CSS', extensions: ['css'] }] }
    const res = win ? await dialog.showOpenDialog(win, options) : await dialog.showOpenDialog(options)
    const path = res.filePaths[0]
    if (res.canceled || !path) return null
    const css = await readFile(path, 'utf8')
    const name = NAME_HEADER.exec(css)?.[1] ?? basename(path).replace(/\.css$/i, '')
    return this.save(null, name, css.replace(NAME_HEADER, '').trimStart())
  }

  async exportFile(id: string, sender: WebContents): Promise<void> {
    const mod = this.mods.find((m) => m.id === id)
    if (!mod) return
    const win = BrowserWindow.fromWebContents(sender)
    const options: Electron.SaveDialogOptions = {
      title: t('mods.exportTitle'),
      defaultPath: join(app.getPath('documents'), `${mod.name.replace(/[/\\?%*:|"<>]/g, '-')}.css`),
      filters: [{ name: 'CSS', extensions: ['css'] }]
    }
    const res = win ? await dialog.showSaveDialog(win, options) : await dialog.showSaveDialog(options)
    if (res.canceled || !res.filePath) return
    await writeFile(res.filePath, `/* name: ${mod.name.replace(/\*\//g, '')} */\n${mod.css}\n`)
  }
}

/** Mod read from disk or received from the UI, validated; null if unusable. */
function clean(value: unknown): InterfaceMod | null {
  if (!value || typeof value !== 'object') return null
  const v = value as Partial<Record<keyof InterfaceMod, unknown>>
  if (typeof v.id !== 'string' || !v.id) return null
  const name = typeof v.name === 'string' && v.name.trim() ? v.name.trim().slice(0, MAX_NAME) : t('mods.untitled')
  const css = typeof v.css === 'string' ? v.css.slice(0, MAX_CSS) : ''
  return { id: v.id, name, css, enabled: v.enabled !== false }
}

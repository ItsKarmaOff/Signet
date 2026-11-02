import { ApiError, type ApiCaller, type ApiHandler } from './types'
import { canAccessUrl, type Access } from './urlmatch'

export type CaptureFormat = 'jpeg' | 'png'

export const DEFAULT_QUALITY = 92

export interface CaptureArgs {
  windowId?: number
  format: CaptureFormat
  quality: number
}

/** `captureVisibleTab([windowId], [options])`: either argument may be missing, options may come first. */
export function parseCaptureArgs(first: unknown, second: unknown): CaptureArgs {
  const optionsFirst = first !== null && typeof first === 'object'
  const options = optionsFirst ? first : second
  const windowId = !optionsFirst && typeof first === 'number' && first >= 0 ? first : undefined
  const o = options !== null && typeof options === 'object' ? (options as Record<string, unknown>) : {}
  if (o.format !== undefined && o.format !== 'jpeg' && o.format !== 'png')
    throw new ApiError(`Error in invocation of tabs.captureVisibleTab: invalid format '${String(o.format)}'.`)
  const q = typeof o.quality === 'number' && Number.isFinite(o.quality) ? o.quality : DEFAULT_QUALITY
  return { windowId, format: o.format === 'png' ? 'png' : 'jpeg', quality: Math.max(0, Math.min(100, Math.round(q))) }
}

/**
 * The primary language of an `<html lang>` value, the way `tabs.detectLanguage` reports it: `en-US` gives `en`, Chinese
 * keeps its script (`zh-CN`, `zh-TW`), anything that is not a language code gives `und`.
 */
export function primaryLanguage(raw: unknown): string {
  const parts = String(raw ?? '')
    .trim()
    .toLowerCase()
    .split(/[-_]/)
  const primary = parts[0]
  if (!primary || !/^[a-z]{2,3}$/.test(primary)) return 'und'
  if (primary === 'zh') return parts.some((p) => ['tw', 'hk', 'mo', 'hant'].includes(p)) ? 'zh-TW' : 'zh-CN'
  return primary
}

export interface CssDetails {
  code?: string
  file?: string
  cssOrigin: 'user' | 'author'
}

/** Normalises the details of `insertCSS` / `removeCSS`; exactly one of `code` and `file` is required, like Chrome. */
export function parseCssDetails(raw: unknown): CssDetails {
  const d = raw !== null && typeof raw === 'object' ? (raw as Record<string, unknown>) : {}
  const hasCode = typeof d.code === 'string'
  const hasFile = typeof d.file === 'string' && d.file !== ''
  if (hasCode === hasFile) throw new ApiError('Exactly one of "code" or "file" must be specified.')
  return { ...(hasCode ? { code: d.code as string } : { file: d.file as string }), cssOrigin: d.cssOrigin === 'user' ? 'user' : 'author' }
}

/** Identity of an injection: what `removeCSS` must be given again to remove it. */
export function cssSource(d: CssDetails): string {
  return `${d.cssOrigin}|${d.file !== undefined ? 'file' : 'code'}|${d.file ?? d.code}`
}

/** Keys returned by `webContents.insertCSS`, per extension, tab and source. */
export class CssRegistry {
  private keys = new Map<string, string[]>()

  private id(extensionId: string, tabId: number, source: string): string {
    return `${extensionId}\0${tabId}\0${source}`
  }

  add(extensionId: string, tabId: number, source: string, key: string): void {
    const id = this.id(extensionId, tabId, source)
    this.keys.set(id, [...(this.keys.get(id) ?? []), key])
  }

  /** Removes and returns every key of this source (a stylesheet inserted twice is removed twice). */
  take(extensionId: string, tabId: number, source: string): string[] {
    const id = this.id(extensionId, tabId, source)
    const found = this.keys.get(id) ?? []
    this.keys.delete(id)
    return found
  }

  forgetTab(tabId: number): void {
    for (const id of [...this.keys.keys()]) if (id.split('\0')[1] === String(tabId)) this.keys.delete(id)
  }

  forgetExtension(extensionId: string): void {
    for (const id of [...this.keys.keys()]) if (id.startsWith(`${extensionId}\0`)) this.keys.delete(id)
  }
}

/** The part of a tab these APIs need (a `WebContents` fits). */
export interface TabLike {
  id: number
  getURL(): string
  isDestroyed(): boolean
  capturePage(): Promise<{ toJPEG(quality: number): Buffer; toDataURL(): string }>
  insertCSS(css: string, options: { cssOrigin: 'user' | 'author' }): Promise<string>
  removeInsertedCSS(key: string): Promise<void>
  executeJavaScript(code: string): Promise<unknown>
}

export interface TabsExtrasDeps {
  contents(tabId: number): TabLike | undefined
  /** Active tab of a window (the current one when `windowId` is undefined). */
  active(windowId: number | undefined): TabLike | undefined
  /** Manifest and granted permissions of the extension. */
  access(caller: ApiCaller): Access
  /** Text of a file of the extension, or throws. */
  readFile(caller: ApiCaller, file: string): Promise<string>
  /** Activates the tab at `index` in the window and returns the window, or undefined if there is no such tab or window. */
  activateAt(index: number, windowId: number | undefined): unknown
}

const NO_TAB = 'No tab found.'

/** The `chrome.tabs` members the library lacks or leaves incomplete. */
export class TabsExtras {
  private css = new CssRegistry()

  constructor(private d: TabsExtrasDeps) {}

  forgetTab(tabId: number): void {
    this.css.forgetTab(tabId)
  }

  forgetExtension(extensionId: string): void {
    this.css.forgetExtension(extensionId)
  }

  private tab(tabId: unknown): TabLike {
    const tab = typeof tabId === 'number' ? this.d.contents(tabId) : this.d.active(undefined)
    if (!tab || tab.isDestroyed()) throw new ApiError(typeof tabId === 'number' ? `No tab with id: ${tabId}.` : NO_TAB)
    return tab
  }

  private checked(caller: ApiCaller, tab: TabLike): TabLike {
    const url = tab.getURL()
    if (!canAccessUrl(this.d.access(caller), caller.extensionId, url))
      throw new ApiError(`Cannot access contents of url "${url}". Extension manifest must request permission to access this host.`)
    return tab
  }

  async capture(caller: ApiCaller, first: unknown, second: unknown): Promise<string> {
    const { windowId, format, quality } = parseCaptureArgs(first, second)
    const tab = this.d.active(windowId)
    if (!tab || tab.isDestroyed()) throw new ApiError(NO_TAB)
    if (!canAccessUrl(this.d.access(caller), caller.extensionId, tab.getURL()))
      throw new ApiError("Either the '<all_urls>' or 'activeTab' permission is required.")
    const image = await tab.capturePage()
    return format === 'png' ? image.toDataURL() : `data:image/jpeg;base64,${image.toJPEG(quality).toString('base64')}`
  }

  highlight(_caller: ApiCaller, info: unknown): unknown {
    const i = info !== null && typeof info === 'object' ? (info as Record<string, unknown>) : {}
    const indexes = (Array.isArray(i.tabs) ? i.tabs : [i.tabs]).filter((n): n is number => Number.isInteger(n) && (n as number) >= 0)
    if (!indexes.length) throw new ApiError('No highlighted tab specified.')
    const windowId = typeof i.windowId === 'number' && i.windowId >= 0 ? i.windowId : undefined
    const win = this.d.activateAt(indexes[0], windowId)
    if (!win) throw new ApiError(`No tab at index: ${indexes[0]}.`)
    return win
  }

  async insertCSS(caller: ApiCaller, first: unknown, second: unknown): Promise<void> {
    const details = parseCssDetails(typeof first === 'number' ? second : first)
    const tab = this.checked(caller, this.tab(first))
    const css = details.code ?? (await this.d.readFile(caller, details.file!))
    const key = await tab.insertCSS(css, { cssOrigin: details.cssOrigin })
    this.css.add(caller.extensionId, tab.id, cssSource(details), key)
  }

  async removeCSS(caller: ApiCaller, first: unknown, second: unknown): Promise<void> {
    const details = parseCssDetails(typeof first === 'number' ? second : first)
    const tab = this.checked(caller, this.tab(first))
    for (const key of this.css.take(caller.extensionId, tab.id, cssSource(details))) await tab.removeInsertedCSS(key).catch(() => {})
  }

  async detectLanguage(_caller: ApiCaller, tabId: unknown): Promise<string> {
    const tab = this.tab(tabId)
    try {
      const raw = await tab.executeJavaScript(
        `(document.documentElement && document.documentElement.lang) || (document.querySelector('meta[http-equiv="content-language" i]') || {}).content || ''`
      )
      return primaryLanguage(raw)
    } catch {
      return 'und'
    }
  }

  methods(): Record<string, ApiHandler> {
    return {
      'tabs.captureVisibleTab': (c, a, b) => this.capture(c, a, b),
      'tabs.highlight': (c, info) => this.highlight(c, info),
      'tabs.insertCSS': (c, a, b) => this.insertCSS(c, a, b),
      'tabs.removeCSS': (c, a, b) => this.removeCSS(c, a, b),
      'tabs.detectLanguage': (c, id) => this.detectLanguage(c, id)
    }
  }
}

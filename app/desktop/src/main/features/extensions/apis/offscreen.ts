import { OFFSCREEN_REASONS } from '@shared/extension-apis'
import { pageContext, type ExtensionContext } from './contexts'
import { ApiError } from './types'

/** What we need from a hidden document: Electron's WebContents in production, a fake in tests. */
export interface OffscreenPage {
  id: number
  load(url: string): Promise<void>
  close(): void
  /** Called once when the page goes away by itself. */
  onClosed(fn: () => void): void
}

interface Doc {
  url: string
  reasons: string[]
  justification: string
  page: OffscreenPage
}

/**
 * `chrome.offscreen`: at most one hidden document per extension, loaded from the extension's own origin. Like Chrome,
 * it stays until the extension closes it (or is unloaded). The page factory creates a webContents that is never
 * attached to a window, so it can neither show nor become a tab.
 */
export class Offscreen {
  private docs = new Map<string, Doc>()

  constructor(
    private open: (ses: unknown, extensionId: string) => OffscreenPage,
    private log: (message: string) => void = () => {}
  ) {}

  async create(ses: unknown, extensionId: string, params: unknown): Promise<void> {
    const p = (params && typeof params === 'object' ? params : {}) as { url?: unknown; reasons?: unknown; justification?: unknown }
    if (typeof p.url !== 'string' || !p.url) throw new ApiError("Invalid value for argument 1. Property 'url': Expected 'string'.")
    if (!Array.isArray(p.reasons) || !p.reasons.length || p.reasons.some((r) => !(OFFSCREEN_REASONS as readonly string[]).includes(String(r)))) {
      throw new ApiError("Invalid value for argument 1. Property 'reasons': Expected a non-empty array of offscreen reasons.")
    }
    if (typeof p.justification !== 'string' || !p.justification)
      throw new ApiError("Invalid value for argument 1. Property 'justification': Expected 'string'.")
    const origin = `chrome-extension://${extensionId}`
    let url: URL
    try {
      url = new URL(p.url, `${origin}/`)
    } catch {
      throw new ApiError('Invalid URL.')
    }
    // `URL.origin` is 'null' for chrome-extension:, so compare the parts.
    if (url.protocol !== 'chrome-extension:' || url.host !== extensionId) throw new ApiError('Offscreen documents must be pages of the extension.')
    if (this.docs.has(extensionId)) throw new ApiError('Only a single offscreen document may be created.')

    const page = this.open(ses, extensionId)
    const doc: Doc = { url: url.href, reasons: p.reasons.map(String), justification: p.justification, page }
    // Reserve the slot before loading: a second call while this one loads must be refused.
    this.docs.set(extensionId, doc)
    page.onClosed(() => {
      if (this.docs.get(extensionId) === doc) this.docs.delete(extensionId)
    })
    try {
      await page.load(doc.url)
    } catch (e) {
      this.docs.delete(extensionId)
      page.close()
      throw new ApiError(`Failed to load the offscreen document: ${e instanceof Error ? e.message : String(e)}`)
    }
    this.log(`[extension-apis] ${extensionId} offscreen document created: ${doc.url} (${doc.reasons.join(', ')})`)
  }

  close(extensionId: string): void {
    const doc = this.docs.get(extensionId)
    if (!doc) throw new ApiError('No current offscreen document.')
    this.docs.delete(extensionId)
    doc.page.close()
    this.log(`[extension-apis] ${extensionId} offscreen document closed`)
  }

  has(extensionId: string): boolean {
    return this.docs.has(extensionId)
  }

  /** Extension unloaded: its document goes (no error if there is none). */
  closeFor(extensionId: string): void {
    if (this.docs.has(extensionId)) this.close(extensionId)
  }

  /** True for the webContents of an offscreen document (so it is not reported as a tab). */
  owns(webContentsId: number): boolean {
    return [...this.docs.values()].some((d) => d.page.id === webContentsId)
  }

  contexts(extensionId: string): ExtensionContext[] {
    const doc = this.docs.get(extensionId)
    return doc ? [pageContext(extensionId, doc.page.id, doc.url, 'OFFSCREEN_DOCUMENT')] : []
  }
}

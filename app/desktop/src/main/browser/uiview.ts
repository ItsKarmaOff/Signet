import { WebContentsView } from 'electron'
import type { UiEventMap } from '@shared/ipc/contract'
import { denyWindowOpen } from '../web/guard'
import { loadRoute, preloadPath } from '../core/urls'
import { logUiError } from '../app/uilog'

/** Transparent UI view (find bar, floating sidebar): early sends are queued. */
export class UiView {
  readonly view: WebContentsView
  private ready = false
  /** Last send per channel, replayed when the page loads. */
  private queued = new Map<keyof UiEventMap, unknown[]>()
  private fallback: NodeJS.Timeout | null = null

  /** `handshake`: the page itself signals that it is listening (`markReady`), instead of the plain did-finish-load. */
  constructor(
    route: string,
    private readonly handshake = false
  ) {
    this.view = new WebContentsView({
      webPreferences: { preload: preloadPath, sandbox: true, contextIsolation: true, nodeIntegration: false }
    })
    this.view.setBackgroundColor('#00000000')
    const wc = this.view.webContents
    // Every (re)load: sends wait for the fresh page, then the last one of each channel is replayed.
    wc.on('did-start-loading', () => {
      this.ready = false
      if (this.fallback) clearTimeout(this.fallback)
      this.fallback = null
    })
    wc.on('did-finish-load', () => {
      if (!this.handshake) return this.markReady()
      // Safety net: a page that never signals still gets its sends.
      this.fallback = setTimeout(() => {
        this.fallback = null
        if (!this.ready) this.markReady()
      }, 3000)
    })
    // View process stopped (crash): we reload rather than leave an empty, hence invisible, view.
    wc.on('render-process-gone', (_e, details) => {
      this.ready = false
      logUiError(route || 'main', `view process gone: ${details.reason}`)
      if (details.reason !== 'clean-exit') setTimeout(() => !wc.isDestroyed() && loadRoute(wc, route), 500)
    })
    denyWindowOpen(wc)
    loadRoute(wc, route)
  }

  /** The page is subscribed to its channels: the last send of each is replayed. */
  markReady(): void {
    if (this.fallback) clearTimeout(this.fallback)
    this.fallback = null
    if (this.view.webContents.isDestroyed()) return
    this.ready = true
    const queued = [...this.queued]
    this.queued.clear()
    for (const [channel, args] of queued) this.sendArgs(channel, args)
  }

  send<C extends keyof UiEventMap>(channel: C, ...args: UiEventMap[C]): void {
    this.sendArgs(channel, args)
  }

  private sendArgs(channel: keyof UiEventMap, args: unknown[]): void {
    const wc = this.view.webContents
    if (wc.isDestroyed()) return
    if (this.ready) wc.send(channel, ...args)
    else this.queued.set(channel, args)
  }

  get webContents() {
    return this.view.webContents
  }

  destroy(): void {
    if (this.fallback) clearTimeout(this.fallback)
    this.fallback = null
    if (!this.view.webContents.isDestroyed()) this.view.webContents.close()
  }
}

import { app, type IpcMainEvent, type IpcMainInvokeEvent, type WebContents } from 'electron'
import { isAppUrl } from '../core/urls'
import { logUiError } from '../app/uilog'

/** Tab views: they may display an app page (notes, Easels) without being an interface view. */
export const tabContents = new WeakSet<WebContents>()

/**
 * Locks down interface views: a page that displays the app's interface may only navigate to the interface
 * (a link dragged onto it must never load a web page there, which would inherit `window.signet`).
 * The check is done on the current URL: tabs (web pages) are not concerned, they have their own guard.
 */
export function guardUiViews(): void {
  app.on('web-contents-created', (_e, wc) => {
    const block = (e: { preventDefault: () => void; url: string }) => {
      if (tabContents.has(wc)) return // a tab (even on a note) has its own guard
      if (isAppUrl(wc.getURL()) && !isAppUrl(e.url)) e.preventDefault()
    }
    wc.on('will-navigate', block)
    wc.on('will-frame-navigate', block)
    wc.on('will-redirect', block)
    healUiView(wc)
  })
}

/** True once the dev server has stopped: no more reloading, the app quits. */
let devEnded = false

function endDevSession(): void {
  if (devEnded) return
  devEnded = true
  console.log('Dev server stopped: Signet is quitting.')
  app.quit()
}

/** Name of the interface view for the log (route after `#`). */
const routeOf = (url: string) => /#([a-z]+)/.exec(url)?.[1] ?? 'main'

/**
 * An interface view that starts without its bridge script (`preload` unreadable while `out/` is rewritten by a
 * build) or without its page (dev server not ready yet) would stay empty and transparent, hence invisible:
 * we reload it, with increasing delays (1 s, 2 s, 4 s… 30 s at most), until it loads.
 */
function healUiView(wc: WebContents): void {
  let tries = 0
  let timer: NodeJS.Timeout | null = null
  const retry = (why: string) => {
    if (timer || wc.isDestroyed() || devEnded) return
    // One line per outage, not one per attempt.
    if (tries === 0) logUiError(routeOf(wc.getURL()), why, 'rechargement')
    timer = setTimeout(
      () => {
        timer = null
        if (!wc.isDestroyed()) wc.reload()
      },
      Math.min(30_000, 1000 * 2 ** Math.min(tries++, 5))
    )
  }
  wc.on('preload-error', (_e, _path, error) => {
    if (isAppUrl(wc.getURL())) retry(`preload : ${error.message}`)
  })
  wc.on('did-fail-load', (_e, code, description, url, isMainFrame) => {
    if (!isMainFrame || !isAppUrl(url)) return
    // Development: the interface server refuses the connection, the dev session is over (Ctrl+C): quit.
    const dev = process.env.ELECTRON_RENDERER_URL
    if (dev && code === -102 && url.startsWith(dev)) return endDevSession()
    // -3: load interrupted by another navigation, not an outage.
    if (code !== -3) retry(`chargement : ${description} (${code})`)
  })
  wc.on('did-finish-load', () => {
    if (isAppUrl(wc.getURL())) tries = 0
  })
  wc.once('destroyed', () => timer && clearTimeout(timer))
}

/** Refuses any window opening from an interface view (to set on those that have no handler of their own). */
export function denyWindowOpen(wc: WebContents): void {
  wc.setWindowOpenHandler(() => ({ action: 'deny' }))
}

/** True if the call really comes from an interface page (frame present, app URL). */
export function fromUi(e: IpcMainInvokeEvent | IpcMainEvent): boolean {
  const frame = e.senderFrame
  return !!frame && isAppUrl(frame.url)
}

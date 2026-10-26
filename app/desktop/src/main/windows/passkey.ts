import { app, BrowserWindow, type WebContents, type WebFrameMain } from 'electron'
import QRCode from 'qrcode'
import { windowChrome } from '../app/platform'
import type { Browser } from '../browser/browser'
import { loadRoute, preloadPath } from '../core/urls'
import { t } from '../app/i18n'
import type { WebAuthnAnswer } from '../web/webauthn'
import type { WebAuthnRequest } from '../web/webauthn-request'
import { pushUi } from '../ipc/push'
import { CABLE_UUIDS } from '../web/hybrid/eid'
import { openTunnel } from '../web/hybrid/tunnel'
import { HybridSessions, type HybridHost, type HybridPrompt, type HybridTarget } from '../web/hybrid/session'

const WIDTH = 400
const HEIGHT = 520

/** Stable key of a frame: its process and routing ids. */
const frameKey = (f: WebFrameMain): string => `${f.processId}:${f.routingId}`

/**
 * Passkey from a phone ("hybrid" transport): the small window showing the QR code, and the session it belongs to.
 * One at a time, child of the window that holds the requesting tab.
 */
export class PasskeyPrompt {
  private win: BrowserWindow | null = null
  private sessions = new HybridSessions(this.host())

  constructor(private b: Browser) {}

  /** Starts a session for a validated, non-conditional request; resolves when it ends. */
  request(request: WebAuthnRequest, sender: WebContents, frame: WebFrameMain): Promise<WebAuthnAnswer> {
    return this.sessions.start(request, { senderId: sender.id, frameKey: frameKey(frame), source: sender })
  }

  /** The page aborted its request. */
  abort(sender: WebContents, frame: WebFrameMain): void {
    this.sessions.abort(sender.id, frameKey(frame))
  }

  /** What the window shows (null: nothing to show, the window closes). */
  prompt(): HybridPrompt | null {
    return this.sessions.prompt()
  }

  /** Cancel button or Escape. */
  cancel(): void {
    this.sessions.cancel()
  }

  /** Window title in the current language. */
  retitle(): void {
    if (this.win && !this.win.isDestroyed()) this.win.setTitle(t('window.passkey'))
  }

  private host(): HybridHost {
    return {
      render: async (text) =>
        `data:image/svg+xml;base64,${Buffer.from(await QRCode.toString(text, { type: 'svg', margin: 2, errorCorrectionLevel: 'L' })).toString('base64')}`,
      watch: (target: HybridTarget, onGone) => {
        const wc = target.source as WebContents
        const navigated = (details: { isMainFrame: boolean; isSameDocument: boolean }): void => {
          if (details.isMainFrame && !details.isSameDocument) onGone()
        }
        wc.on('did-start-navigation', navigated)
        wc.once('destroyed', onGone)
        wc.once('render-process-gone', onGone)
        return () => {
          wc.removeListener('did-start-navigation', navigated)
          wc.removeListener('destroyed', onGone)
          wc.removeListener('render-process-gone', onGone)
        }
      },
      openWindow: (target, onClosed) => this.open(target.source as WebContents, onClosed),
      scan: (onServiceData, onState) => {
        // BlueZ (D-Bus) on Linux only; elsewhere there is nothing to scan with.
        if (process.platform !== 'linux') {
          onState('unsupported')
          return () => {}
        }
        let stop = (): void => {}
        let stopped = false
        void import('../web/hybrid/bluez')
          .then(({ startBleScan }) => {
            if (stopped) return
            stop = startBleScan({
              uuids: CABLE_UUIDS,
              onServiceData: (_uuid, bytes) => onServiceData(bytes),
              onState: (state) => onState(state === 'scanning' ? 'waiting' : state)
            })
          })
          .catch((e) => {
            console.error('Bluetooth: scan unavailable:', e)
            if (!stopped) onState('error')
          })
        return () => {
          stopped = true
          stop()
        }
      },
      status: (status) => {
        if (this.win && !this.win.isDestroyed()) pushUi(this.win.webContents, 'passkey:status', status)
      },
      found: (eid) => {
        // Development only. Neither the nonce nor any key is logged.
        if (!app.isPackaged) console.log(`Passkey: phone found (tunnel ${eid.tunnelDomain}, routing id ${eid.routingId.toString('hex')})`)
      },
      openTunnel,
      // Development only: stages and failure reasons, no keys, no data.
      log: (line) => {
        if (!app.isPackaged) console.log(`Passkey: ${line}`)
      }
    }
  }

  private open(tab: WebContents, onClosed: () => void): () => void {
    const parent = (!tab.isDestroyed() && BrowserWindow.fromWebContents(tab)) || this.b.win || undefined
    const live = parent && !parent.isDestroyed() ? parent : undefined
    const main = live ? live.getBounds() : null
    const win = new BrowserWindow({
      // Child window: stays above the requesting window, even after a click on it.
      parent: live,
      width: WIDTH,
      height: HEIGHT,
      ...(main && { x: Math.round(main.x + (main.width - WIDTH) / 2), y: Math.round(main.y + (main.height - HEIGHT) / 2) }),
      center: !main,
      resizable: false,
      minimizable: false,
      maximizable: false,
      fullscreenable: false,
      title: t('window.passkey'),
      ...windowChrome({ lights: { x: 16, y: 18 }, vibrancy: true }),
      webPreferences: { preload: preloadPath, sandbox: true, contextIsolation: true, nodeIntegration: false }
    })
    this.win = win
    let closing = false
    win.on('closed', () => {
      if (this.win === win) this.win = null
      if (!closing) onClosed()
    })
    loadRoute(win.webContents, '#passkey')
    return () => {
      closing = true
      if (!win.isDestroyed()) win.close()
    }
  }
}

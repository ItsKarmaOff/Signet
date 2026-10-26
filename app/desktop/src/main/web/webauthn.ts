import { app, ipcMain, type WebContents, type WebFrameMain } from 'electron'
import type { Browser } from '../browser/browser'
import { NOT_ALLOWED_MESSAGE, isWebAuthnError, validateWebAuthnRequest, type WebAuthnError, type WebAuthnRequest } from './webauthn-request'

/** Answer sent to the page: an error (the page rejects with it); steps to come add the credential here. */
export type WebAuthnAnswer = { error: WebAuthnError }

/** Only Linux has no passkey support to lose; elsewhere only on demand, in development (Windows Hello keeps working). */
export function webauthnInterceptEnabled(): boolean {
  return process.platform === 'linux' || (!app.isPackaged && process.env.SIGNET_WEBAUTHN_INTERCEPT === '1')
}

/**
 * Answers a validated request: a conditional one stays pending in the page (no IPC), the others open a hybrid session
 * (QR code for a phone) that ends cancelled for now, so that sites fall back to other methods.
 * The next steps (BLE, tunnel) complete the session (see hybrid/session.ts).
 */
export function answerWebAuthn(b: Browser, request: WebAuthnRequest, sender: WebContents, frame: WebFrameMain): Promise<WebAuthnAnswer> {
  if (request.mediation === 'conditional') return Promise.resolve({ error: { name: 'NotAllowedError', message: NOT_ALLOWED_MESSAGE } })
  return b.passkeys.request(request, sender, frame)
}

/** Page channel `webauthn:request` (untrusted): tabs only, origin read from the sending frame. Never throws. */
export function registerWebAuthnIpc(b: Browser): void {
  ipcMain.handle('webauthn:request', async (e, payload: unknown): Promise<WebAuthnAnswer> => {
    try {
      const notAllowed: WebAuthnAnswer = { error: { name: 'NotAllowedError', message: NOT_ALLOWED_MESSAGE } }
      const frame = e.senderFrame
      if (!frame || !b.views.tabIdOfContents(e.sender.id)) return notAllowed
      const topOrigin = (frame.top ?? e.sender.mainFrame).origin
      const request = validateWebAuthnRequest({ origin: frame.origin, topOrigin, isMainFrame: frame.parent === null, payload })
      if (isWebAuthnError(request)) return { error: request }
      return await answerWebAuthn(b, request, e.sender, frame)
    } catch {
      return { error: { name: 'NotAllowedError', message: NOT_ALLOWED_MESSAGE } }
    }
  })
  // The page aborted its pending request: only the frame that owns the session can end it.
  ipcMain.on('webauthn:abort', (e) => {
    if (e.senderFrame && b.views.tabIdOfContents(e.sender.id)) b.passkeys.abort(e.sender, e.senderFrame)
  })
}

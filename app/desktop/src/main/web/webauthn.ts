import { app, ipcMain } from 'electron'
import type { Browser } from '../browser/browser'
import { NOT_ALLOWED_MESSAGE, isWebAuthnError, validateWebAuthnRequest, type WebAuthnError, type WebAuthnRequest } from './webauthn-request'

/** Answer sent to the page: an error (the page rejects with it); steps to come add the credential here. */
export type WebAuthnAnswer = { error: WebAuthnError }

/** Only Linux has no passkey support to lose; elsewhere only on demand, in development (Windows Hello keeps working). */
export function webauthnInterceptEnabled(): boolean {
  return process.platform === 'linux' || (!app.isPackaged && process.env.SIGNET_WEBAUTHN_INTERCEPT === '1')
}

/**
 * Answers a validated request. For now: cancelled, so that sites fall back to other methods.
 * The next steps (QR code, BLE, tunnel) replace this function.
 */
export function answerWebAuthn(request: WebAuthnRequest): Promise<WebAuthnAnswer> {
  void request
  return Promise.resolve({ error: { name: 'NotAllowedError', message: NOT_ALLOWED_MESSAGE } })
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
      return await answerWebAuthn(request)
    } catch {
      return { error: { name: 'NotAllowedError', message: NOT_ALLOWED_MESSAGE } }
    }
  })
}

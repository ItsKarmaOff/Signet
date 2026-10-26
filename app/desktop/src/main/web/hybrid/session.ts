import type { PasskeyPromptState } from '@shared/types'
import { NOT_ALLOWED_MESSAGE, type WebAuthnRequest } from '../webauthn-request'
import type { WebAuthnAnswer } from '../webauthn'
import { generateQrKeys, qrText, type QrKeys } from './qr'

// "Hybrid" (caBLE v2) sessions: one at a time, owned by the main process. Pure module (no Electron): the window and the
// tab watching are injected by `HybridHost`. Tested.

/** `publicKey.timeout` is clamped like Chrome does (10 s to 10 min); 5 min without one. */
const MIN_TIMEOUT = 10_000
const MAX_TIMEOUT = 600_000
const DEFAULT_TIMEOUT = 300_000

export function hybridTimeout(publicKey: Record<string, unknown>): number {
  const t = publicKey.timeout
  return typeof t === 'number' && Number.isFinite(t) ? Math.min(MAX_TIMEOUT, Math.max(MIN_TIMEOUT, t)) : DEFAULT_TIMEOUT
}

/** The requesting frame, as main knows it (never from the page). */
export interface HybridTarget {
  senderId: number
  /** Stable key of the frame (process and routing ids). */
  frameKey: string
  /** The sender's webContents (opaque here, read by the host). */
  source: unknown
}

/** What the session needs from Electron. */
export interface HybridHost {
  /** QR code as an image data URL. */
  render(text: string): Promise<string>
  /** Shows the code window for the tab `target`; `onClosed` fires when the user closes it. Returns the function that closes it. */
  openWindow(target: HybridTarget, onClosed: () => void): () => void
  /** Calls `onGone` when the tab navigates away (main frame, not same-document) or is destroyed. Returns the unwatch function. */
  watch(target: HybridTarget, onGone: () => void): () => void
}

/** What the code window shows. */
export type HybridPrompt = PasskeyPromptState

export const NOT_ALLOWED: WebAuthnAnswer = { error: { name: 'NotAllowedError', message: NOT_ALLOWED_MESSAGE } }

/**
 * One request waiting for a phone. Holds the secrets of the session (identity private key, QR secret): they stay here, in
 * main memory. `end` resolves the answer once and cleans everything.
 *
 * Steps to come plug in here: the BLE scan reads the phone's advert with `keys.secret`, the tunnel handshake uses
 * `keys.ecdh`; success calls `end({ credential })`, a failure `end()`.
 */
export class HybridSession {
  readonly keys: QrKeys
  readonly timeout: number
  qr = ''
  private ended = false
  private cleanups: (() => void)[] = []
  private resolve!: (a: WebAuthnAnswer) => void
  readonly answer = new Promise<WebAuthnAnswer>((r) => (this.resolve = r))

  constructor(
    readonly request: WebAuthnRequest,
    readonly target: HybridTarget,
    private onEnd: (s: HybridSession) => void
  ) {
    this.keys = generateQrKeys()
    this.timeout = hybridTimeout(request.publicKey)
  }

  get isEnded(): boolean {
    return this.ended
  }

  text(now?: number): string {
    return qrText(this.keys, this.request.type, now)
  }

  /** Registers something to undo when the session ends (timer, listener, window). */
  onCleanup(fn: () => void): void {
    if (this.ended) fn()
    else this.cleanups.push(fn)
  }

  prompt(): HybridPrompt {
    return { type: this.request.type, rpId: this.request.rpId, qr: this.qr }
  }

  /** Ends the session: the first call decides the answer (cancelled by default), the next ones do nothing. */
  end(answer: WebAuthnAnswer = NOT_ALLOWED): void {
    if (this.ended) return
    this.ended = true
    for (const fn of this.cleanups.splice(0)) {
      try {
        fn()
      } catch {
        // cleaning must go on
      }
    }
    this.onEnd(this)
    this.resolve(answer)
  }
}

/** The current session, if any: a new request ends the previous one. */
export class HybridSessions {
  private current: HybridSession | null = null

  constructor(private host: HybridHost) {}

  /** Starts a session for a validated, non-conditional request; resolves when it ends. */
  async start(request: WebAuthnRequest, target: HybridTarget): Promise<WebAuthnAnswer> {
    this.current?.end()
    const session = new HybridSession(request, target, (s) => {
      if (this.current === s) this.current = null
    })
    this.current = session
    try {
      const timer = setTimeout(() => session.end(), session.timeout)
      session.onCleanup(() => clearTimeout(timer))
      session.onCleanup(this.host.watch(target, () => session.end()))
      session.qr = await this.host.render(session.text())
      // Replaced, cancelled or gone while the code was drawn.
      if (!session.isEnded) session.onCleanup(this.host.openWindow(target, () => session.end()))
    } catch {
      session.end()
    }
    return session.answer
  }

  /** What the window shows; null without a session. */
  prompt(): HybridPrompt | null {
    return this.current && !this.current.isEnded ? this.current.prompt() : null
  }

  /** The user cancelled (button, Escape). */
  cancel(): void {
    this.current?.end()
  }

  /** The page aborted its request (AbortSignal): only the frame that owns the session may end it. */
  abort(senderId: number, frameKey: string): void {
    const { current } = this
    if (current && current.target.senderId === senderId && current.target.frameKey === frameKey) current.end()
  }
}

import type { PasskeyPromptState, PasskeyStatus } from '@shared/types'
import { NOT_ALLOWED_MESSAGE, type WebAuthnRequest } from '../webauthn-request'
import type { WebAuthnAnswer } from '../webauthn'
import { CtapFailure, parseCtapResponse, parseGetInfo, planCtap } from './ctap'
import { decryptAdvert, type Eid } from './eid'
import { generateQrKeys, qrText, type QrKeys } from './qr'
import { TunnelError, type Tunnel, type TunnelOptions } from './tunnel'

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
  /** Scans for the phone's BLE advert: `onServiceData` gets the service data of each candidate advert, `onState` the state of the scan. Returns the stop function. */
  scan(onServiceData: (bytes: Uint8Array) => void, onState: (state: ScanState) => void): () => void
  /** The window must show a new status. */
  status(status: PasskeyStatus): void
  /** The phone's advert was found and decrypted (development log). */
  found?(eid: Eid): void
  /** Connects to the phone through the tunnel server (see tunnel.ts). */
  openTunnel(options: TunnelOptions): Promise<Tunnel>
  /** Development log of the stages and of why a request failed (never data or keys). */
  log?(line: string): void
}

/** State of the Bluetooth scan, as the window shows it (`unsupported`: no scan on this system). */
export type ScanState = Extract<PasskeyStatus, 'waiting' | 'off' | 'no-adapter' | 'denied' | 'error' | 'unsupported'>

/** How long the "failed" status stays before the page gets its error. */
export const FAILED_DELAY = 3000

/** What the code window shows. */
export type HybridPrompt = PasskeyPromptState

export const NOT_ALLOWED: WebAuthnAnswer = { error: { name: 'NotAllowedError', message: NOT_ALLOWED_MESSAGE } }

/**
 * One request waiting for a phone. Holds the secrets of the session (identity private key, QR secret): they stay here, in
 * main memory. `end` resolves the answer once and cleans everything.
 *
 * The BLE scan reads the phone's advert with `keys.secret` (-> `eid`); the tunnel handshake uses `eid` and `keys.ecdh`;
 * success calls `end({ credential })`, a failure shows "failed" for a moment, then `end({ error })`.
 */
export class HybridSession {
  readonly keys: QrKeys
  readonly timeout: number
  qr = ''
  status: PasskeyStatus = 'waiting'
  /** The phone's decrypted advert, once found: routing id and nonce for the tunnel. Main memory only. */
  eid: Eid | null = null
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
    return { type: this.request.type, rpId: this.request.rpId, qr: this.qr, status: this.status }
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
      if (!session.isEnded) {
        session.onCleanup(this.host.openWindow(target, () => session.end()))
        this.scan(session)
      }
    } catch {
      session.end()
    }
    return session.answer
  }

  /** Looks for the phone's advert while the session is open; the first one that decrypts with the QR secret is the phone. */
  private scan(session: HybridSession): void {
    let stop = (): void => {}
    const found = (bytes: Uint8Array): void => {
      if (session.isEnded || session.eid) return
      const eid = decryptAdvert(session.keys.secret, bytes)
      if (!eid) return
      session.eid = eid
      stop()
      this.host.found?.(eid)
      void this.connect(session, eid)
    }
    stop = this.host.scan(found, (state) => {
      if (!session.eid) this.setStatus(session, state)
    })
    session.onCleanup(stop)
  }

  /** Tunnel, handshake, then the CTAP2 request; the session stays open until the answer, a failure, cancel or timeout. */
  private async connect(session: HybridSession, eid: Eid): Promise<void> {
    const log = (line: string): void => this.host.log?.(line)
    const abort = new AbortController()
    session.onCleanup(() => abort.abort())
    this.setStatus(session, 'connecting')
    try {
      const tunnel = await this.host.openTunnel({
        eid,
        secret: session.keys.secret,
        identity: session.keys.ecdh,
        signal: abort.signal,
        log
      })
      session.onCleanup(() => tunnel.close())
      if (session.isEnded) return
      this.setStatus(session, 'confirm')
      const plan = planCtap(session.request, parseGetInfo(tunnel.getInfo))
      if (plan.skipped.length) log(`ignored extensions: ${plan.skipped.join(', ')}`)
      log(`sending ${plan.type === 'get' ? 'getAssertion' : 'makeCredential'}`)
      const response = await tunnel.transact(plan.bytes, abort.signal)
      const credential = parseCtapResponse(plan, response)
      log('credential received')
      session.end({ credential })
    } catch (e) {
      if (session.isEnded) return
      this.fail(session, e)
    }
  }

  /** Shows "failed" for a few seconds, then answers the page with the mapped error. */
  private fail(session: HybridSession, e: unknown): void {
    // Only our own errors carry a message that is safe to log (fixed texts, no data).
    this.host.log?.(
      e instanceof TunnelError ? `failed (${e.stage}): ${e.message}` : e instanceof CtapFailure ? `failed: ${e.reason}` : 'failed: unexpected error'
    )
    const answer: WebAuthnAnswer = e instanceof CtapFailure ? { error: e.error } : NOT_ALLOWED
    this.setStatus(session, 'failed')
    const timer = setTimeout(() => session.end(answer), FAILED_DELAY)
    session.onCleanup(() => clearTimeout(timer))
  }

  private setStatus(session: HybridSession, status: PasskeyStatus): void {
    if (session.isEnded || session.status === status) return
    session.status = status
    this.host.status(status)
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

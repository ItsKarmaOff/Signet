import type { Session } from 'electron'

/** Who is calling: always derived from the sender (worker scope or page URL), never from the arguments. */
export interface ApiCaller {
  ses: Session
  extensionId: string
  /** The exact context that called (a worker run, or one page): several can exist for one extension. */
  context: ApiContext
}

export interface ApiContext {
  kind: 'worker' | 'page'
  /** Unique while the context lives (`worker-<version>`, `page-<webContents id>`). */
  id: string
  url: string
  /** Runs `fn` once when this context goes away (worker stopped, page closed). */
  onGone(fn: () => void): void
}

/** Context used when a caller does not give one (tests, internal calls). */
export const NO_CONTEXT: ApiContext = { kind: 'worker', id: 'worker-unknown', url: '', onGone: () => {} }

export type ApiHandler = (caller: ApiCaller, ...args: unknown[]) => unknown

/** What a feature needs from the router to reach extensions. */
export interface ExtensionEmitter {
  /**
   * Delivers an event to an extension's worker and pages in every session where it is loaded.
   * `wake` starts a stopped service worker (alarms, a setting taken over); otherwise only running contexts receive it.
   * When the worker cannot start, the event is kept and retried unless `queue` is false (time-sensitive events).
   */
  emit(extensionId: string, name: string, args: unknown[], wake: boolean, queue?: boolean): void
  /** Extensions loaded in at least one attached session that declare the permission. */
  extensionsWith(permission: string): string[]
}

/** Error shown to the extension as `runtime.lastError` / a rejected promise. */
export class ApiError extends Error {}

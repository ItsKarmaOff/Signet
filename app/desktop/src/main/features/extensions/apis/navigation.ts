import type { ExtensionEmitter } from './types'

export const ERROR_EVENT = 'webNavigation.onErrorOccurred'
export const FRAGMENT_EVENT = 'webNavigation.onReferenceFragmentUpdated'

/** Chrome's text for a network error: `net::ERR_NAME_NOT_RESOLVED` (Electron drops the `net::`). */
export function errorString(code: number, description: string): string {
  if (/^net::/.test(description)) return description
  return description.startsWith('ERR_') ? `net::${description}` : `net::ERR_FAILED (${code})`
}

/** A navigation inside a document that only changed the `#fragment`. */
export function isFragmentChange(previous: string | undefined, next: string): boolean {
  if (!previous) return false
  try {
    const a = new URL(previous)
    const b = new URL(next)
    const [ha, hb] = [a.hash, b.hash]
    a.hash = ''
    b.hash = ''
    return a.href === b.href && ha !== hb
  } catch {
    return false
  }
}

export interface FrameInfo {
  frameId: number
  parentFrameId: number
  frameType: 'outermost_frame' | 'sub_frame'
}

/** The events of a tab we listen to (a `WebContents` fits). */
export interface NavigationSource {
  id: number
  getURL(): string
  on(event: string, listener: (...args: any[]) => void): unknown // eslint-disable-line @typescript-eslint/no-explicit-any
}

export interface NavigationDeps {
  emitter: Pick<ExtensionEmitter, 'emit' | 'extensionsWith'>
  frame(processId: number, routingId: number): FrameInfo | undefined
  now?: () => number
}

/** `webNavigation` events the library does not send: errors and fragment changes. */
export class NavigationExtras {
  private watched = new WeakSet<object>()

  constructor(private d: NavigationDeps) {}

  watch(tab: NavigationSource): void {
    if (this.watched.has(tab)) return
    this.watched.add(tab)
    const last = new Map<string, string>()
    const key = (pid: number, rid: number): string => `${pid}:${rid}`
    // The main frame is addressed by whichever process currently renders it; its first URL is the tab's.
    let mainUrl = tab.getURL()
    const urlOf = (pid: number, rid: number, main: boolean): string | undefined => (main ? (last.get('main') ?? mainUrl) : last.get(key(pid, rid)))
    const remember = (pid: number, rid: number, main: boolean, url: string): void => {
      last.set(key(pid, rid), url)
      if (main) {
        mainUrl = url
        last.set('main', url)
      }
    }

    tab.on('did-frame-navigate', (_e: unknown, url: string, _code: number, _text: string, main: boolean, pid: number, rid: number) =>
      remember(pid, rid, main, url)
    )

    tab.on('did-navigate-in-page', (_e: unknown, url: string, main: boolean, pid: number, rid: number) => {
      const previous = urlOf(pid, rid, main)
      remember(pid, rid, main, url)
      if (isFragmentChange(previous, url)) this.send(tab, FRAGMENT_EVENT, pid, rid, url, { transitionType: 'link', transitionQualifiers: [] })
    })

    tab.on('did-fail-load', (_e: unknown, code: number, description: string, url: string, _main: boolean, pid: number, rid: number) => {
      // ERR_ABORTED (-3) too: Chrome reports cancelled navigations, downloads and stops with net::ERR_ABORTED.
      this.send(tab, ERROR_EVENT, pid, rid, url, { error: errorString(code, description) })
    })
  }

  private send(tab: NavigationSource, event: string, pid: number, rid: number, url: string, extra: Record<string, unknown>): void {
    const frame = this.d.frame(pid, rid)
    if (!frame) return
    const details = { ...frame, documentLifecycle: 'active', processId: pid, tabId: tab.id, timeStamp: (this.d.now ?? Date.now)(), url, ...extra }
    // Running contexts only: waking a stopped worker for every page load would start all extensions' workers each time.
    for (const id of this.d.emitter.extensionsWith('webNavigation')) this.d.emitter.emit(id, event, [details], false)
  }
}

/// <reference lib="dom" />
// Page script (registered on the tabs' sessions, all frames): fills `window.chrome` like Chrome.
// Electron leaves it empty, without `loadTimes`, `csi` or `app`, and Google then refuses the sign-in ("This browser or
// app may not be secure"). Runs in the page's world, before its scripts.
import { contextBridge } from 'electron'

try {
  contextBridge.executeInMainWorld({
    func: () => {
      // A bound function displays as `function () { [native code] }`, like Chrome's native functions.
      const native = <T extends (...args: never[]) => unknown>(name: string, fn: T): T => {
        const bound = fn.bind(null) as T
        Object.defineProperty(bound, 'name', { value: name })
        return bound
      }
      const nav = (): Partial<PerformanceNavigationTiming> => (performance.getEntriesByType('navigation')[0] as PerformanceNavigationTiming | undefined) ?? {}
      const t0 = performance.timeOrigin
      const seconds = (ms: number | undefined) => (ms ? (t0 + ms) / 1000 : 0)
      const w = window as unknown as { chrome?: Record<string, unknown> }
      const chrome = w.chrome ?? {}

      chrome.loadTimes ??= native('loadTimes', () => {
        const n = nav()
        const protocol = n.nextHopProtocol || 'http/1.1'
        return {
          requestTime: seconds(n.requestStart),
          startLoadTime: t0 / 1000,
          commitLoadTime: seconds(n.responseStart),
          finishDocumentLoadTime: seconds(n.domContentLoadedEventEnd),
          finishLoadTime: seconds(n.loadEventEnd),
          firstPaintTime: seconds(performance.getEntriesByName('first-paint')[0]?.startTime),
          firstPaintAfterLoadTime: 0,
          navigationType: 'Other',
          wasFetchedViaSpdy: protocol === 'h2',
          wasNpnNegotiated: protocol === 'h2',
          npnNegotiatedProtocol: protocol === 'h2' ? 'h2' : 'unknown',
          wasAlternateProtocolAvailable: false,
          connectionInfo: protocol
        }
      })
      chrome.csi ??= native('csi', () => ({
        startE: Math.round(t0),
        onloadT: Math.round(t0 + (nav().loadEventEnd ?? 0)),
        pageT: performance.now(),
        tran: 15
      }))
      chrome.app ??= {
        isInstalled: false,
        getDetails: native('getDetails', () => null),
        getIsInstalled: native('getIsInstalled', () => false),
        installState: native('installState', (cb?: (state: string) => void) => {
          if (cb) setTimeout(() => cb('not_installed'))
        }),
        runningState: native('runningState', () => 'cannot_run'),
        InstallState: { DISABLED: 'disabled', INSTALLED: 'installed', NOT_INSTALLED: 'not_installed' },
        RunningState: { CANNOT_RUN: 'cannot_run', READY_TO_RUN: 'ready_to_run', RUNNING: 'running' }
      }
      w.chrome = chrome
    }
  })
} catch {
  // frame without context isolation: nothing to do, the page keeps Electron's object
}

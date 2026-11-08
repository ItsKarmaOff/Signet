// Port of preload/identity.ts. Fills `window.chrome` like Chrome (Google refuses the sign-in otherwise).
// All frames, main world, document start. Extension pages keep their real `chrome`.
(function identity(bridge, config, g) {
  'use strict'
  g = g || globalThis
  if (g.location.protocol === 'chrome-extension:') return
  // A bound function displays as `function () { [native code] }`, like Chrome's native functions.
  const native = (name, fn) => {
    const bound = fn.bind(null)
    Object.defineProperty(bound, 'name', { value: name })
    return bound
  }
  const perf = g.performance
  const nav = () => (perf.getEntriesByType('navigation')[0]) || {}
  const t0 = perf.timeOrigin
  const seconds = (ms) => (ms ? (t0 + ms) / 1000 : 0)
  const chrome = g.chrome || {}

  if (chrome.loadTimes === undefined) {
    chrome.loadTimes = native('loadTimes', () => {
      const n = nav()
      const protocol = n.nextHopProtocol || 'http/1.1'
      const paint = perf.getEntriesByName('first-paint')[0]
      return {
        requestTime: seconds(n.requestStart),
        startLoadTime: t0 / 1000,
        commitLoadTime: seconds(n.responseStart),
        finishDocumentLoadTime: seconds(n.domContentLoadedEventEnd),
        finishLoadTime: seconds(n.loadEventEnd),
        firstPaintTime: seconds(paint && paint.startTime),
        firstPaintAfterLoadTime: 0,
        navigationType: 'Other',
        wasFetchedViaSpdy: protocol === 'h2',
        wasNpnNegotiated: protocol === 'h2',
        npnNegotiatedProtocol: protocol === 'h2' ? 'h2' : 'unknown',
        wasAlternateProtocolAvailable: false,
        connectionInfo: protocol
      }
    })
  }
  if (chrome.csi === undefined) {
    chrome.csi = native('csi', () => ({
      startE: Math.round(t0),
      onloadT: Math.round(t0 + (nav().loadEventEnd || 0)),
      pageT: perf.now(),
      tran: 15
    }))
  }
  if (chrome.app === undefined) {
    chrome.app = {
      isInstalled: false,
      getDetails: native('getDetails', () => null),
      getIsInstalled: native('getIsInstalled', () => false),
      installState: native('installState', (cb) => {
        if (cb) g.setTimeout(() => cb('not_installed'))
      }),
      runningState: native('runningState', () => 'cannot_run'),
      InstallState: { DISABLED: 'disabled', INSTALLED: 'installed', NOT_INSTALLED: 'not_installed' },
      RunningState: { CANNOT_RUN: 'cannot_run', READY_TO_RUN: 'ready_to_run', RUNNING: 'running' }
    }
  }
  g.chrome = chrome
})

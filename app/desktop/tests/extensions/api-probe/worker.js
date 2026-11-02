// Service worker of the API probe. Runs the probe at startup and answers { probe: 'report' } messages.
/* global chrome */
importScripts('probe-core.js')

const errors = []
self.addEventListener('error', (e) => errors.push({ kind: 'uncaught', message: String(e.message || e) }))
self.addEventListener('unhandledrejection', (e) => errors.push({ kind: 'unhandledrejection', message: String((e.reason && e.reason.message) || e.reason) }))

const saved = {}
let workerReport = null
const ready = self.__probe.runProbe('worker').then((r) => {
  workerReport = r
  return r
})

// Registered synchronously at the top level, as MV3 requires.
chrome.runtime.onMessage.addListener((msg, _sender, sendResponse) => {
  if (!msg || typeof msg !== 'object') return false
  if (msg.probe === 'save') {
    saved[msg.context] = msg.report
    sendResponse(true)
    return false
  }
  if (msg.probe === 'report') {
    ready.then(() => sendResponse({ worker: { ...workerReport, errors: [...workerReport.errors, ...errors] }, saved, startupErrors: errors }))
    return true
  }
  return false
})

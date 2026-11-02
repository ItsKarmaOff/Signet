// Extension page embedded in an iframe of the options page (subframe): reports to its parent and to the worker.
/* global chrome */
window.__probeDone = false
window.__probe.runProbe('iframe').then(async (report) => {
  window.__probeReport = report
  try {
    await chrome.runtime.sendMessage({ probe: 'save', context: 'iframe', report })
  } catch (e) {
    report.errors.push({ kind: 'save', message: String(e && e.message) })
  }
  window.parent.postMessage({ probeFrameReport: report }, '*')
  window.__probeDone = true
})

// Popup page of the API probe: reports to the worker and keeps the report on window.__probeReport for the test.
/* global chrome */
window.__probeDone = false
window.__probe.runProbe('popup').then(async (report) => {
  window.__probeReport = report
  try {
    await chrome.runtime.sendMessage({ probe: 'save', context: 'popup', report })
  } catch (e) {
    report.errors.push({ kind: 'save', message: String(e && e.message) })
  }
  window.__probeDone = true
  document.getElementById('status').textContent = 'done'
})

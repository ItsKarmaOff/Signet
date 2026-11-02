// Options page of the API probe. Collects its own report, the iframe's, and (via runtime.sendMessage) the worker's.
/* global chrome */
window.__probeDone = false
window.__probeReports = {}

const frameReport = new Promise((resolve) => {
  window.addEventListener('message', (e) => {
    if (e.data && e.data.probeFrameReport) resolve(e.data.probeFrameReport)
  })
  setTimeout(() => resolve(null), 20000)
})

async function main() {
  const report = await window.__probe.runProbe('options')
  window.__probeReport = report
  window.__probeReports.options = report
  try {
    await chrome.runtime.sendMessage({ probe: 'save', context: 'options', report })
  } catch (e) {
    report.errors.push({ kind: 'save', message: String(e && e.message) })
  }
  window.__probeReports.iframe = await frameReport
  try {
    window.__probeReports.workerAnswer = await Promise.race([
      chrome.runtime.sendMessage({ probe: 'report' }),
      new Promise((_, rej) => setTimeout(() => rej(new Error('worker did not answer in 15 s')), 15000))
    ])
  } catch (e) {
    window.__probeReports.workerError = String(e && e.message)
  }
  window.__probeDone = true
  document.getElementById('status').textContent = 'done'
}
main()

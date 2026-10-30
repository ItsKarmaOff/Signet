// Extension API probe: loads tests/extensions/api-probe as an unpacked extension, runs it in the service worker, the popup page,
// the options page and an extension iframe, and writes test-results/extension-api-probe.{json,md}.
// Assertions stay light (this spec reports more than it checks): the worker starts cleanly, Signet's own namespaces are there,
// and `browser === chrome` wherever `browser` exists.
import { cpSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { EXTENSION_API_CATALOG } from '../../src/shared/extension-api-catalog'
import { test, expect, type App } from './fixtures'
import { buildMarkdown, type ProbeReport, type Reports } from './extension-api-report'

const PROBE_SRC = resolve(__dirname, '../extensions/api-probe')
const RESULTS = resolve('test-results')

test.setTimeout(240_000)

/** Opens `url` in a new active tab and returns the tab id. */
const openUrl = (a: App, url: string) => a.signet((b, u) => b.tabs.open(u, { activate: true }), url)

/** Waits for `window.__probeDone`, then returns `JSON.stringify(expression)` evaluated in the tab page. */
async function collect<T>(a: App, tabId: string, expression: string): Promise<T> {
  const js = (code: string) => a.signet((b, [id, c]) => b.views.viewOf(id)!.webContents.executeJavaScript(c), [tabId, code] as [string, string])
  await expect.poll(() => js('window.__probeDone === true').catch(() => false), { timeout: 90_000, intervals: [500, 1000] }).toBe(true)
  return JSON.parse((await js(`JSON.stringify(${expression})`)) as string) as T
}

test('extension API probe: report what each context exposes', async ({ signetApp }) => {
  // A temporary copy of the probe, with the catalog generated next to it (the committed folder holds no generated file).
  const dir = mkdtempSync(join(tmpdir(), 'signet-api-probe-'))
  const probeDir = join(dir, 'api-probe')
  cpSync(PROBE_SRC, probeDir, { recursive: true })
  writeFileSync(join(probeDir, 'catalog.json'), JSON.stringify(EXTENSION_API_CATALOG))
  // SIGNET_PROBE_UNSAFE=system.cpu.getInfo,event:system.storage.onAttached lets the probe touch APIs known to crash Signet.
  writeFileSync(join(probeDir, 'probe-config.json'), JSON.stringify({ unsafe: (process.env.SIGNET_PROBE_UNSAFE ?? '').split(',').filter(Boolean) }))

  const logs: string[] = []
  // Progress lines of the probe (stderr): if a native API ever kills the Electron process, the last lines say where.
  const progress: string[] = []
  signetApp.app.process().stderr?.on('data', (d: Buffer) =>
    progress.push(
      ...String(d)
        .split('\n')
        .filter((l) => l.includes('[probe]'))
    )
  )
  signetApp.app.process().stdout?.on('data', (d: Buffer) =>
    logs.push(
      ...String(d)
        .split('\n')
        .filter((l) => l.includes('[extension-apis]'))
    )
  )

  try {
    // Loaded straight into the session tabs use, not through Signet's unpacked mechanism (Settings > Extensions): that one watches the
    // folder and reloads on change, and a reload while the probe runs kills the Electron process (also seen on macOS FSEvents replays).
    const loaded = await signetApp.app.evaluate(
      ({ session }, p) => session.defaultSession.extensions.loadExtension(p, { allowFileAccess: true }).then((e) => ({ id: e.id, name: e.name })),
      probeDir
    )
    expect(loaded.name).toBe('Signet API probe')
    const id = loaded.id

    const popupTab = await openUrl(signetApp, `chrome-extension://${id}/popup.html`)
    const optionsTab = await openUrl(signetApp, `chrome-extension://${id}/options.html`)

    const popup = await collect<ProbeReport>(signetApp, popupTab, 'window.__probeReport')
    const options = await collect<{
      options: ProbeReport
      iframe: ProbeReport | null
      workerAnswer?: { worker: ProbeReport; saved: Record<string, ProbeReport> }
      workerError?: string
    }>(signetApp, optionsTab, 'window.__probeReports')

    const reports: Reports = { worker: options.workerAnswer?.worker ?? null, popup, options: options.options, iframe: options.iframe }
    const electron = await signetApp.app.evaluate(() => process.versions.electron)
    mkdirSync(RESULTS, { recursive: true })
    writeFileSync(
      join(RESULTS, 'extension-api-probe.json'),
      JSON.stringify({ extensionId: id, platform: process.platform, electron, reports, workerError: options.workerError, logs }, null, 2)
    )
    writeFileSync(
      join(RESULTS, 'extension-api-probe.md'),
      buildMarkdown(reports, { extensionId: id!, logs, electron, platform: `${process.platform} ${process.arch}` })
    )

    // The worker started and answered, without an uncaught error.
    expect(options.workerError, 'worker answered runtime.onMessage').toBeUndefined()
    const worker = reports.worker!
    expect(worker.errors.filter((e) => e.kind !== 'save')).toEqual([])
    expect(worker.hasChrome).toBe(true)

    // Signet's own namespaces, in the worker.
    const present = (c: ProbeReport, path: string) => !!c.namespaces?.[path]?.chrome
    for (const path of ['proxy', 'privacy', 'alarms', 'offscreen', 'declarativeNetRequest']) expect(present(worker, path), `worker chrome.${path}`).toBe(true)
    expect(worker.namespaces?.runtime?.members.getContexts?.chrome, 'worker chrome.runtime.getContexts').toBe('function')

    // `browser` is an alias of `chrome` wherever Signet's preload runs. Extension subframes (iframe) get no preload today: reported, not asserted.
    for (const name of ['worker', 'popup', 'options'] as const) {
      const r = reports[name]
      if (r?.hasBrowser) expect(r.browserIsChrome, `${name}: browser === chrome`).toBe(true)
    }
  } finally {
    mkdirSync(RESULTS, { recursive: true })
    writeFileSync(join(RESULTS, 'extension-api-probe.progress.log'), progress.join('\n'))
    rmSync(dir, { recursive: true, force: true })
  }
})

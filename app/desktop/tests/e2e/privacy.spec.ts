import { createServer } from 'node:http'
import { test, expect, openTab, type App } from './fixtures'

/** The page title shows the `Sec-GPC` header the server received. */
async function gpcServer(): Promise<{ url: string; close: () => void }> {
  const server = createServer((req, res) => {
    res.writeHead(200, { 'content-type': 'text/html; charset=utf-8' })
    res.end(`<!doctype html><title>gpc-${req.headers['sec-gpc'] ?? 'none'}</title><body>gpc</body>`)
  })
  await new Promise<void>((r) => server.listen(0, '127.0.0.1', () => r()))
  const port = (server.address() as { port: number }).port
  return { url: `http://127.0.0.1:${port}/`, close: () => server.close() }
}

const title = (a: App, id: string) => a.signet((b, i) => b.views.viewOf(i)?.webContents.getTitle(), id)
const reload = (a: App, id: string) => a.signet((b, i) => b.views.viewOf(i)!.webContents.reload(), id)
const navigatorGpc = (a: App, id: string) => a.signet((b, i) => b.views.viewOf(i)!.webContents.executeJavaScript('String(navigator.globalPrivacyControl)'), id)

test('Global Privacy Control: header and navigator follow the setting without restart', async ({ signetApp }) => {
  const site = await gpcServer()
  try {
    const id = await openTab(signetApp, site.url, 'gpc-none')
    expect(await navigatorGpc(signetApp, id)).toBe('undefined')

    await signetApp.signet((b) => b.settings.update({ globalPrivacyControl: true }))
    await reload(signetApp, id)
    await expect.poll(() => title(signetApp, id)).toBe('gpc-1')
    expect(await navigatorGpc(signetApp, id)).toBe('true')

    await signetApp.signet((b) => b.settings.update({ globalPrivacyControl: false }))
    await reload(signetApp, id)
    await expect.poll(() => title(signetApp, id)).toBe('gpc-none')
    expect(await navigatorGpc(signetApp, id)).toBe('undefined')
  } finally {
    site.close()
  }
})

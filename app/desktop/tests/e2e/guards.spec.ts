import { createServer } from 'node:http'
import { test, expect, openTab, uiPage } from './fixtures'

test('signet:// addresses: a regular web page cannot go there; the command bar opens Settings without a tab', async ({ signetApp }) => {
  const html = '<!doctype html><title>plain</title><a id="l" href="signet://version">link</a>'
  const server = createServer((_q, res) => res.end(html)).listen(0, '127.0.0.1')
  await new Promise((r) => server.once('listening', r))
  try {
    const origin = `http://127.0.0.1:${(server.address() as { port: number }).port}/`
    const id = await openTab(signetApp, origin, 'plain')
    await expect.poll(() => signetApp.signet((b, i) => b.views.viewOf(i)?.webContents.isLoading() === false, id)).toBe(true)
    const attempts = [
      "document.getElementById('l').click()",
      "location.href = 'signet://version'",
      "window.open('signet://version')",
      "document.body.insertAdjacentHTML('beforeend', '<iframe id=f src=signet://version></iframe>')"
    ]
    const tabs = async () => Object.keys((await signetApp.snapshot()).tabs).length
    const before = await tabs()
    for (const code of attempts) {
      await signetApp.signet(
        (b, a) =>
          b.views
            .viewOf(a.id)!
            .webContents.executeJavaScript(a.code)
            .catch(() => {}),
        { id, code }
      )
      // Leaves time for a possible navigation to complete before concluding that none happened.
      await signetApp.signet((b, i) => b.views.viewOf(i)!.webContents.executeJavaScript('new Promise((r) => setTimeout(r, 400))'), id)
      const state = await signetApp.signet((b, i) => {
        const wc = b.views.viewOf(i)!.webContents
        return { url: wc.getURL(), tabUrl: b.store.state.tabs[i].url, frames: wc.mainFrame.frames.map((f) => f.url) }
      }, id)
      expect(state.url).toBe(origin)
      expect(state.tabUrl).toBe(origin)
      expect(state.frames.filter((u) => u.startsWith('signet:') || u.startsWith('chrome:'))).toEqual([])
      expect(await tabs()).toBe(before)
    }
  } finally {
    server.close()
  }

  // From the command bar: signet://settings/privacy opens Settings on that section, without creating a tab.
  const n = Object.keys((await signetApp.snapshot()).tabs).length
  await signetApp.signet((b) => b.tabs.openUrl('signet://settings/privacy', 'new-tab'))
  await expect.poll(() => !!uiPage(signetApp, '#settings')).toBe(true)
  await expect(uiPage(signetApp, '#settings')!.locator('.st-nav-item.on')).toHaveAttribute('data-section', 'privacy')
  expect(Object.keys((await signetApp.snapshot()).tabs).length).toBe(n)
})

test('private window: in-memory, separate session, cleared on close', async ({ signetApp }) => {
  const server = createServer((req, res) => {
    res.writeHead(200, { 'content-type': 'text/html; charset=utf-8' })
    res.end(
      req.url === '/a'
        ? `<!doctype html><title>A</title><script>document.cookie = 'signetpriv=1; max-age=3600'</script>`
        : `<!doctype html><title>B</title><p id=c></p><script>c.textContent = document.cookie</script>`
    )
  }).listen(0, '127.0.0.1')
  await new Promise((r) => server.once('listening', r))
  try {
    const base = `http://127.0.0.1:${(server.address() as { port: number }).port}`
    const res = await signetApp.signet(async (b, url) => {
      const defaultSession = b.win!.webContents.session
      const win = b.privates.open(url)
      const wc = () => b.views.viewOf(b.privates.activeTabOf(win)!)?.webContents
      for (let i = 0; i < 100 && wc()?.getTitle() !== 'A'; i++) await new Promise((r) => setTimeout(r, 100))
      const ses = wc()!.session
      const own = (await ses.cookies.get({ url })).length
      const shared = (await defaultSession.cookies.get({ url })).length
      // Second private window: another session, without the first one's cookie.
      const win2 = b.privates.open(`${url}/b`)
      for (let i = 0; i < 100 && b.views.viewOf(b.privates.activeTabOf(win2)!)?.webContents.getTitle() !== 'B'; i++)
        await new Promise((r) => setTimeout(r, 100))
      const wc2 = b.views.viewOf(b.privates.activeTabOf(win2)!)!.webContents
      const cookie2 = await wc2.executeJavaScript('document.getElementById("c").textContent')
      const out = { persistent: ses.isPersistent(), isDefault: ses === defaultSession, own, shared, otherSession: wc2.session !== ses, cookie2 }
      win2.close()
      b.privates.closeActive(win)
      for (let i = 0; i < 100 && !win.isDestroyed(); i++) await new Promise((r) => setTimeout(r, 100))
      return { ...out, closed: win.isDestroyed(), after: (await ses.cookies.get({ url })).length }
    }, `${base}/a`)
    expect(res).toEqual({ persistent: false, isDefault: false, own: 1, shared: 0, otherSession: true, cookie2: '', closed: true, after: 0 })
  } finally {
    server.close()
  }
})

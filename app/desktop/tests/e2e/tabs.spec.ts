import { test, expect, openTab, uiPage } from './fixtures'

test('an open tab shows its title and URL in the state and in the sidebar', async ({ signetApp, site }) => {
  const id = await openTab(signetApp, site.url('alpha'), 'alpha')
  const tab = (await signetApp.snapshot()).tabs[id]
  expect(tab.url).toBe(site.url('alpha'))
  expect(tab.loaded).toBe(true)
  expect(await signetApp.signet((b) => b.space.activeTabId)).toBe(id)
  await expect(uiPage(signetApp, '')!.locator(`[data-flip-id="${id}"]`)).toContainText('alpha')
})

test('naviguer, reculer, avancer', async ({ signetApp, site }) => {
  const id = await openTab(signetApp, site.url('one'), 'one')
  await signetApp.signet((b, a) => b.tabs.navigate(a.id, a.url), { id, url: site.url('two') })
  await expect.poll(async () => (await signetApp.snapshot()).tabs[id].title).toBe('two')
  await expect.poll(async () => (await signetApp.snapshot()).tabs[id].canGoBack).toBe(true)
  await signetApp.signet((b, i) => b.tabs.goBack(i), id)
  await expect.poll(async () => (await signetApp.snapshot()).tabs[id].title).toBe('one')
  expect((await signetApp.snapshot()).tabs[id].canGoForward).toBe(true)
  await signetApp.signet((b, i) => b.tabs.goForward(i), id)
  await expect.poll(async () => (await signetApp.snapshot()).tabs[id].url).toBe(site.url('two'))
})

test('close a tab then reopen it', async ({ signetApp, site }) => {
  await openTab(signetApp, site.url('keep'), 'keep')
  const id = await openTab(signetApp, site.url('doomed'), 'doomed')
  await signetApp.signet((b, i) => b.tabs.close(i), id)
  await expect.poll(async () => Object.values((await signetApp.snapshot()).tabs).map((t) => t.title)).not.toContain('doomed')
  await expect(uiPage(signetApp, '')!.locator('.row', { hasText: 'doomed' })).toHaveCount(0)
  await signetApp.signet((b) => b.tabs.reopenClosed())
  await expect.poll(async () => Object.values((await signetApp.snapshot()).tabs).map((t) => t.url)).toContain(site.url('doomed'))
})

test('pin, unpin and favorite', async ({ signetApp, site }) => {
  const id = await openTab(signetApp, site.url('pinme'), 'pinme')
  const zone = () => signetApp.signet((b, i) => b.model.locate(i)?.zone, id)
  expect(await zone()).toBe('today')
  await signetApp.signet((b, i) => b.arrange.togglePin(i), id)
  expect(await zone()).toBe('pinned')
  expect((await signetApp.snapshot()).tabs[id].pinnedUrl).toBe(site.url('pinme'))
  await signetApp.signet((b, i) => b.arrange.togglePin(i), id)
  expect(await zone()).toBe('today')
  await signetApp.signet((b, i) => b.arrange.toggleFavorite(i), id)
  expect(await zone()).toBe('favorites')
  expect((await signetApp.snapshot()).favorites).toContain(id)
  await signetApp.signet((b, i) => b.arrange.toggleFavorite(i), id)
  expect((await signetApp.snapshot()).favorites).not.toContain(id)
})

test('a link opened from a page creates a tab group', async ({ signetApp, site }) => {
  const parent = await openTab(
    signetApp,
    `data:text/html;charset=utf-8,${encodeURIComponent(
      `<!doctype html><title>parent-page</title><body style="margin:0"><a target="_blank" href="${site.url('child-page')}" style="display:block;height:80px;background:#eef">link</a>`
    )}`,
    'parent-page'
  )
  // Real click in the page (like the user), once the view is in place.
  await expect.poll(() => signetApp.signet((b, id) => b.views.viewOf(id)?.getBounds().height ?? 0, parent)).toBeGreaterThan(100)
  await signetApp.signet((b, id) => {
    const wc = b.views.viewOf(id)!.webContents
    wc.sendInputEvent({ type: 'mouseMove', x: 40, y: 30 })
    wc.sendInputEvent({ type: 'mouseDown', x: 40, y: 30, button: 'left', clickCount: 1 })
    wc.sendInputEvent({ type: 'mouseUp', x: 40, y: 30, button: 'left', clickCount: 1 })
  }, parent)
  await expect.poll(async () => Object.values((await signetApp.snapshot()).tabs).some((t) => t.title === 'child-page')).toBe(true)
  // The parent and the child form a group.
  const gid = await signetApp.signet((b) => b.space.today.flatMap((i) => (i.kind === 'tab' && i.group ? [i.group] : []))[0])
  expect(gid).toBeTruthy()
  expect(await signetApp.signet((b, g) => b.space.today.filter((i) => i.kind === 'tab' && i.group === g).length, gid)).toBe(2)
  await signetApp.signet((b, g) => b.groups.rename(g, 'Mon groupe'), gid)
  expect(await signetApp.signet((b, g) => b.space.groups?.[g]?.name, gid)).toBe('Mon groupe')
})

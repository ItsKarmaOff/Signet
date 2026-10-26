import { test, expect, openTab, uiPage } from './fixtures'

test('folder: create, file a tab in it, rename, delete', async ({ signetApp, site }) => {
  const id = await openTab(signetApp, site.url('inside'), 'inside')
  // The sidebar must be listening before the rename request (sent only once, no queue).
  await expect(uiPage(signetApp, '')!.locator(`[data-flip-id="${id}"]`)).toBeVisible()
  const folderId = await signetApp.signet((b) => b.folders.create().id)
  await signetApp.signet((b, a) => b.folders.addTo(a.id, a.folderId), { id, folderId })
  // Renaming in the UI: creating a folder opens the name field.
  const row = uiPage(signetApp, '')!.locator(`[data-flip-id="${folderId}"] .folder-row`)
  await row.locator('input.folder-input').fill('Lectures')
  await row.locator('input.folder-input').press('Enter')

  const folder = () =>
    signetApp.signet((b, f) => {
      const loc = b.model.findFolder(f)
      return loc && { name: loc.item.name, tabs: loc.item.items.flatMap((i) => (i.kind === 'tab' ? [i.tabId] : [])) }
    }, folderId)
  expect(await folder()).toEqual({ name: 'Lectures', tabs: [id] })
  // A tab filed in a folder is pinned.
  expect(await signetApp.signet((b, i) => b.model.locate(i)?.zone, id)).toBe('pinned')
  await expect(row).toContainText('Lectures')

  // Deleting without the tabs: the folder disappears, the tab stays.
  await signetApp.signet((b, f) => b.folders.delete(f, false), folderId)
  expect(await folder()).toBeNull()
  expect(await signetApp.signet((b, i) => b.model.locate(i)?.zone, id)).toBe('pinned')
  expect((await signetApp.snapshot()).tabs[id]).toBeDefined()
})

test('deleting a folder with its tabs: undoable via "reopen"', async ({ signetApp, site }) => {
  const id = await openTab(signetApp, site.url('in-folder'), 'in-folder')
  const folderId = await signetApp.signet((b) => b.folders.create().id)
  await signetApp.signet((b, a) => b.folders.addTo(a.id, a.folderId), { id, folderId })
  await signetApp.signet((b, f) => b.folders.delete(f, true), folderId)
  expect((await signetApp.snapshot()).tabs[id]).toBeUndefined()
  await signetApp.signet((b) => b.tabs.reopenClosed())
  await expect.poll(async () => Object.values((await signetApp.snapshot()).tabs).map((t) => t.url)).toContain(site.url('in-folder'))
})

test('spaces: create, switch, tabs isolated per space', async ({ signetApp, site }) => {
  const first = (await signetApp.snapshot()).activeSpaceId
  const tabA = await openTab(signetApp, site.url('in-first'), 'in-first')

  await signetApp.signet((b) => b.spaces.create({ name: 'Travail', icon: '💼', hue: 200 }))
  let snap = await signetApp.snapshot()
  expect(snap.spaces.map((s) => s.name)).toContain('Travail')
  const second = snap.activeSpaceId
  expect(second).not.toBe(first)
  await expect(uiPage(signetApp, '')!.locator('.sb-head .name')).toHaveText('Travail')

  const tabB = await openTab(signetApp, site.url('in-second'), 'in-second')
  const tabsOf = (spaceId: string) =>
    signetApp.signet((b, sid) => {
      const s = b.store.state.spaces.find((x) => x.id === sid)!
      return [...s.pinned, ...s.today].flatMap((i) => (i.kind === 'tab' ? [i.tabId] : []))
    }, spaceId)
  expect(await tabsOf(second)).toEqual([tabB])
  expect(await tabsOf(first)).toContain(tabA)
  expect(await tabsOf(first)).not.toContain(tabB)

  await signetApp.signet((b, id) => b.spaces.switchTo(id), first)
  await expect.poll(async () => (await signetApp.snapshot()).activeSpaceId).toBe(first)
  const sidebar = uiPage(signetApp, '')!
  await expect(sidebar.locator('.sb-head .name')).not.toHaveText('Travail')
  await expect(sidebar.locator(`.space-page.cur [data-flip-id="${tabA}"]`)).toBeVisible()
  await expect(sidebar.locator(`[data-flip-id="${tabB}"]`)).toHaveCount(0)
  snap = await signetApp.snapshot()
  expect(snap.spaces).toHaveLength(2)
})

test('split view: two tabs side by side, then separated', async ({ signetApp, site }) => {
  const a = await openTab(signetApp, site.url('left'), 'left')
  const b2 = await openTab(signetApp, site.url('right'), 'right')
  await signetApp.signet((b, id) => b.tabs.activate(id), a)
  await signetApp.signet((b, id) => b.splitView.splitActiveWith(id), b2)

  const split = () =>
    signetApp.signet((b) => {
      const s = b.space.today.find((i) => i.kind === 'split')
      return s && s.kind === 'split' ? { id: s.id, tabIds: s.tabIds } : null
    })
  const item = await split()
  expect(item?.tabIds.sort()).toEqual([a, b2].sort())
  // Both pages are displayed at the same time.
  await expect.poll(async () => (await signetApp.snapshot()).panes.map((p) => p.tabId).sort()).toEqual([a, b2].sort())

  await signetApp.signet((b, id) => b.splitView.unsplit(id), item!.id)
  expect(await split()).toBeNull()
  expect(await signetApp.signet((b) => b.space.today.filter((i) => i.kind === 'tab').length)).toBeGreaterThanOrEqual(2)
  await expect.poll(async () => (await signetApp.snapshot()).panes).toHaveLength(1)
})

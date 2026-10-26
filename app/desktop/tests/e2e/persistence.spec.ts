import { test, expect, launch, quit, openTab } from './fixtures'

test('restart: spaces, tabs, pinned tabs, favorites, folders and history come back', async ({ profile, site }) => {
  // Launch 1: build a varied state.
  const a = await launch(profile)
  const home = (await a.snapshot()).activeSpaceId
  const today = await openTab(a, site.url('today-tab'), 'today-tab')
  const pinned = await openTab(a, site.url('pinned-tab'), 'pinned-tab')
  const fav = await openTab(a, site.url('fav-tab'), 'fav-tab')
  const inFolder = await openTab(a, site.url('folder-tab'), 'folder-tab')
  const walker = await openTab(a, site.url('walk-1'), 'walk-1')
  await a.signet((b, x) => b.tabs.navigate(x.id, x.url), { id: walker, url: site.url('walk-2') })
  await expect.poll(async () => (await a.snapshot()).tabs[walker].title).toBe('walk-2')
  await a.signet((b, x) => b.tabs.navigate(x.id, x.url), { id: walker, url: site.url('walk-3') })
  await expect.poll(async () => (await a.snapshot()).tabs[walker].title).toBe('walk-3')
  await a.signet((b, id) => b.tabs.goBack(id), walker)
  await expect.poll(async () => (await a.snapshot()).tabs[walker].title).toBe('walk-2')

  await a.signet((b, id) => b.arrange.togglePin(id), pinned)
  await a.signet((b, id) => b.arrange.toggleFavorite(id), fav)
  const folderId = await a.signet((b) => b.folders.create().id)
  await a.signet((b, x) => (b.folders.rename(x.folderId, 'Persistant'), b.folders.addTo(x.id, x.folderId)), { folderId, id: inFolder })
  await a.signet((b) => b.spaces.create({ name: 'Second', icon: '🚀', hue: 120 }))
  const second = (await a.snapshot()).activeSpaceId
  const secondTab = await openTab(a, site.url('second-space-tab'), 'second-space-tab')
  await a.signet((b, id) => b.spaces.switchTo(id), home)
  // Lets the page history fill up (deferred write) before exit.
  await expect.poll(() => a.signet((b) => b.history.size)).toBeGreaterThan(0)
  expect(await quit(a)).toBe(true)

  // Launch 2: same profile.
  const r = await launch(profile)
  try {
    const snap = await r.snapshot()
    expect(snap.spaces.map((s) => s.name)).toEqual([expect.any(String), 'Second'])
    expect(snap.activeSpaceId).toBe(home)
    const zone = (id: string) => r.signet((b, i) => b.model.locate(i)?.zone, id)
    expect(snap.tabs[today].url).toBe(site.url('today-tab'))
    expect(await zone(today)).toBe('today')
    expect(await zone(pinned)).toBe('pinned')
    expect(snap.tabs[pinned].pinnedUrl).toBe(site.url('pinned-tab'))
    expect(await zone(fav)).toBe('favorites')
    expect(snap.favorites).toContain(fav)
    expect(await r.signet((b, f) => b.model.findFolder(f)?.item.name, folderId)).toBe('Persistant')
    expect(await r.signet((b, f) => b.model.findFolder(f)?.item.items.length, folderId)).toBe(1)
    expect(await zone(inFolder)).toBe('pinned')
    expect(await r.signet((b, i) => b.model.locate(i)?.space?.id, secondTab)).toBe(second)
    // Restored tabs stay unloaded until they are opened.
    expect(snap.tabs[secondTab].loaded).toBe(false)

    // A tab's navigation history: entries and position preserved.
    const nav = await r.signet(async (b, id) => {
      const wc = b.views.ensureView(id).webContents
      await new Promise((res) => setTimeout(res, 2500))
      const h = wc.navigationHistory
      return { entries: h.getAllEntries().length, index: h.getActiveIndex(), url: wc.getURL(), canGoBack: h.canGoBack(), canGoForward: h.canGoForward() }
    }, walker)
    expect(nav.entries).toBe(3)
    expect(nav.index).toBe(1)
    expect(nav.url).toBe(site.url('walk-2'))
    expect(nav.canGoBack && nav.canGoForward).toBe(true)

    // Page history (palette): readable after restart.
    expect(await r.signet((b) => b.history.size)).toBeGreaterThan(0)
  } finally {
    expect(await quit(r)).toBe(true)
  }
})

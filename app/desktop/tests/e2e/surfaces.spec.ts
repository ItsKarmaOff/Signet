import { test, expect, openTab, uiPage } from './fixtures'

// Overlays and floating views of the main window: Peek, undo announcements, popovers, drag, floating bars.

test('Peek: opens as a detached tab, closes, or becomes a tab', async ({ signetApp, site }) => {
  await openTab(signetApp, site.url('base'), 'base')
  await signetApp.signet((b, url) => b.peek.open(url), site.url('apercu'))
  const peeked = await signetApp.signet((b) => ({
    id: b.peek.tabId,
    detached: b.peek.tabId ? b.store.detached.has(b.peek.tabId) : false,
    kind: b.overlay.kind
  }))
  expect(peeked).toMatchObject({ detached: true, kind: 'peek' })
  await expect.poll(async () => (await signetApp.snapshot()).overlayOpen).toBe(true)
  // The Peek web view is placed in the window.
  expect(await signetApp.signet((b) => b.win!.contentView.children.includes(b.views.viewOf(b.peek.tabId!)!))).toBe(true)

  // Closing (with exit animation): the detached tab disappears from the state.
  await signetApp.signet((b) => b.overlay.close())
  await expect.poll(() => signetApp.signet((b) => b.peek.tabId)).toBeNull()
  await expect.poll(async () => (await signetApp.snapshot()).overlayOpen).toBe(false)
  await expect.poll(() => signetApp.signet((b, id) => id in b.store.state.tabs, peeked.id!)).toBe(false)

  // Becoming a tab: a slot among today's tabs, and the tab is active.
  await signetApp.signet((b, url) => b.peek.open(url), site.url('promu'))
  const id = await signetApp.signet((b) => b.peek.tabId!)
  await signetApp.signet((b) => b.peek.expand())
  await expect.poll(() => signetApp.signet((b, i) => b.model.locate(i)?.zone, id)).toBe('today')
  expect(await signetApp.signet((b) => b.space.activeTabId)).toBe(id)
  expect(await signetApp.signet((b, i) => b.store.detached.has(i), id)).toBe(false)
  await expect.poll(async () => (await signetApp.snapshot()).overlayOpen).toBe(false)
  await expect.poll(() => signetApp.signet((b, i) => b.win!.contentView.children.includes(b.views.viewOf(i)!), id)).toBe(true)
})

test('Peek: a click outside the card closes it without animation', async ({ signetApp, site }) => {
  await signetApp.signet((b, url) => b.peek.open(url), site.url('apercu'))
  await expect.poll(async () => (await signetApp.snapshot()).overlayOpen).toBe(true)
  await signetApp.signet((b) => b.overlay.close(true))
  await expect.poll(async () => (await signetApp.snapshot()).overlayOpen).toBe(false)
  expect(await signetApp.signet((b) => b.peek.tabId)).toBeNull()
})

test("archive today's tabs: announcement in the sidebar, then undo", async ({ signetApp, site }) => {
  const a = await openTab(signetApp, site.url('un'), 'un')
  const b2 = await openTab(signetApp, site.url('deux'), 'deux')
  await signetApp.signet((b) => b.tabs.clearToday())
  await expect.poll(async () => Object.keys((await signetApp.snapshot()).tabs).length).toBe(0)
  expect(await signetApp.signet((b) => b.store.state.archive.length)).toBe(2)
  // Sidebar visible: no separate announcement view.
  expect(await signetApp.signet((b) => b.undo.view)).toBeNull()
  const undoId = await signetApp.signet((b) => b.undo['last']!.id)
  await signetApp.signet((b, id) => b.undo.undo(id), undoId)
  await expect.poll(async () => Object.keys((await signetApp.snapshot()).tabs).sort()).toEqual([a, b2].sort())
  expect(await signetApp.signet((b) => b.store.state.archive.length)).toBe(0)
  // A second undo with the same number has no effect.
  await signetApp.signet((b, id) => b.undo.undo(id), undoId)
  expect(Object.keys((await signetApp.snapshot()).tabs)).toHaveLength(2)
})

test('tabs on top: the undo announcement lives in its own view, removed at the end', async ({ signetApp, site }) => {
  await openTab(signetApp, site.url('un'), 'un')
  await signetApp.signet((b) => b.shell.setTabLayout('horizontal'))
  await expect.poll(async () => (await signetApp.snapshot()).sidebarVisible).toBe(false)
  await signetApp.signet((b) => b.tabs.clearToday())
  expect(await signetApp.signet((b) => b.undo.view !== null)).toBe(true)
  await expect.poll(() => signetApp.signet((b) => b.win!.contentView.children.includes(b.undo.view!))).toBe(true)
  await signetApp.signet((b) => b.undo.toastDone())
  expect(await signetApp.signet((b) => b.undo.view)).toBeNull()
})

test('tab row popover: opens the overlay, then closes it', async ({ signetApp }) => {
  await signetApp.signet((b) => b.shell.setTabLayout('horizontal'))
  await signetApp.signet((b) => b.overlay.openPopover({ kind: 'space-form', spaceId: null }, { x: 80, y: 6, width: 120, height: 28 }))
  expect(await signetApp.signet((b) => b.overlay.popoverOpen)).toBe(true)
  await expect.poll(async () => (await signetApp.snapshot()).overlayOpen).toBe(true)
  await signetApp.signet((b) => b.overlay.close(true))
  expect(await signetApp.signet((b) => b.overlay.popoverOpen)).toBe(false)
  await expect.poll(async () => (await signetApp.snapshot()).overlayOpen).toBe(false)
})

test('dragging a tab onto the page: drop zones, then split view', async ({ signetApp, site }) => {
  const a = await openTab(signetApp, site.url('left'), 'left')
  const b2 = await openTab(signetApp, site.url('right'), 'right')
  await signetApp.signet((b, ids) => (b.tabs.activate(ids.a), b.tabs.activate(ids.b2)), { a, b2 })
  // Pressing `b2` just activated it: at the start of the drag, the previous page (`a`) comes back so it can be shared.
  await signetApp.signet((b, id) => b.drag.begin(id), b2)
  expect(await signetApp.signet((b) => b.overlay.kind)).toBe('dropzones')
  expect(await signetApp.signet((b) => b.space.activeTabId)).toBe(a)
  // Without a drop on the page, the pressed tab becomes active again.
  await signetApp.signet((b) => b.drag.end())
  expect(await signetApp.signet((b) => b.overlay.kind)).toBeUndefined()
  await expect.poll(() => signetApp.signet((b) => b.space.activeTabId)).toBe(b2)

  await signetApp.signet((b, id) => b.drag.begin(id), b2)
  await signetApp.signet((b, id) => b.drag.dropOnContent(id, 'right'), b2)
  expect(await signetApp.signet((b) => b.overlay.kind)).toBeUndefined()
  await expect.poll(async () => (await signetApp.snapshot()).panes.length).toBe(2)
})

test('hidden sidebar: hovering the edge reveals it, then it retracts', async ({ signetApp }) => {
  await signetApp.signet((b) => b.shell.toggleSidebar())
  await expect.poll(async () => (await signetApp.snapshot()).sidebarVisible).toBe(false)
  await signetApp.signet((b) => b.floating.setHover(true))
  await expect.poll(async () => (await signetApp.snapshot()).sidebarFloating).toBe(true)
  await expect.poll(() => signetApp.signet((b) => b.win!.contentView.children.includes(b.floating.sidebar!.view))).toBe(true)
  await signetApp.signet((b) => b.floating.setHover(false))
  await expect.poll(async () => (await signetApp.snapshot()).sidebarFloating).toBe(false)
  await expect.poll(() => signetApp.signet((b) => b.floating.leaving)).toBe(false)
  await expect.poll(() => signetApp.signet((b) => b.win!.contentView.children.includes(b.floating.sidebar!.view))).toBe(false)
})

test('compact mode: the floating toolbar appears when hovering the top edge', async ({ signetApp }) => {
  await signetApp.signet((b) => b.shell.setCompact(true))
  await expect.poll(async () => (await signetApp.snapshot()).settings.compactMode).toBe(true)
  await signetApp.signet((b) => b.compact.hover(true))
  await expect.poll(async () => (await signetApp.snapshot()).toolbarFloating).toBe(true)
  await signetApp.signet((b) => b.compact.hover(false))
  await expect.poll(async () => (await signetApp.snapshot()).toolbarFloating).toBe(false)
})

test('closing then reopening the main window: the pages come back', async ({ signetApp, site }) => {
  // Outside macOS, closing the last window quits the app (see index.ts): nothing to reopen.
  test.skip(process.platform !== 'darwin', 'macOS seulement')
  const id = await openTab(signetApp, site.url('persiste'), 'persiste')
  await signetApp.signet((b) => b.win!.close())
  await expect.poll(() => signetApp.signet((b) => b.win)).toBeNull()
  expect(await signetApp.signet((b, i) => b.views.has(i), id)).toBe(false)
  expect(await signetApp.signet((b, i) => b.store.state.tabs[i]?.loaded, id)).toBe(false)
  await signetApp.signet((b) => b.window.open())
  await expect.poll(() => signetApp.signet((b) => b.win?.isVisible())).toBe(true)
  await expect.poll(() => signetApp.signet((b, i) => b.win!.contentView.children.includes(b.views.viewOf(i)!), id)).toBe(true)
  await expect.poll(async () => (await signetApp.snapshot()).tabs[id]?.title).toBe('persiste')
  await expect.poll(() => !!uiPage(signetApp, '')).toBe(true)
})

test('sleep: a hidden tab is unloaded, then comes back when displayed', async ({ signetApp, site }) => {
  const a = await openTab(signetApp, site.url('endormi'), 'endormi')
  await openTab(signetApp, site.url('devant'), 'devant')
  await signetApp.signet((b) => {
    b.store.state.settings.tabSleepMinutes = 0.0001 // quelques millisecondes
  })
  await signetApp.signet(() => new Promise((r) => setTimeout(r, 100)))
  await signetApp.signet((b) => b.sleep['discardIdle']())
  await expect.poll(() => signetApp.signet((b, id) => b.views.has(id), a)).toBe(false)
  expect(await signetApp.signet((b, id) => b.store.state.tabs[id].loaded, a)).toBe(false)
  // The tab stays in the bar; activating it reloads it.
  await signetApp.signet((b, id) => b.tabs.activate(id), a)
  await expect.poll(() => signetApp.signet((b, id) => b.views.has(id), a)).toBe(true)
  await expect.poll(async () => (await signetApp.snapshot()).tabs[a].title).toBe('endormi')
})

test('sleep: the destroyed page does not close the tab (callback from the extensions library)', async ({ signetApp, site }) => {
  const a = await openTab(signetApp, site.url('dort'), 'dort')
  await openTab(signetApp, site.url('devant'), 'devant')
  await signetApp.signet((b) => {
    b.store.state.settings.tabSleepMinutes = 0.0001
  })
  await signetApp.signet(() => new Promise((r) => setTimeout(r, 100)))
  await signetApp.signet((b) => b.sleep['discardIdle']())
  await expect.poll(() => signetApp.signet((b, id) => b.views.has(id), a)).toBe(false)
  expect(await signetApp.signet((b, id) => b.store.state.tabs[id]?.loaded, a)).toBe(false)
  expect(await signetApp.signet((b) => b.store.closed.length)).toBe(0)
})

test('closing a sleeping tab, or one whose page is destroyed, does not crash', async ({ signetApp, site }) => {
  const asleep = await openTab(signetApp, site.url('dort'), 'dort')
  const front = await openTab(signetApp, site.url('devant'), 'devant')
  // Tab without a view (unloaded): closing touches no view.
  await signetApp.signet((b, id) => b.views.destroyView(id), asleep)
  expect(await signetApp.signet((b, id) => b.views.has(id), asleep)).toBe(false)
  await signetApp.signet((b, id) => b.tabs.close(id), asleep)
  expect(await signetApp.signet((b, id) => id in b.store.state.tabs, asleep)).toBe(false)
  // Page destroyed from outside: the extensions library calls back to close the tab (reentrancy).
  await signetApp.signet((b, id) => b.views.viewOf(id)!.webContents.close(), front)
  await expect.poll(() => signetApp.signet((b, id) => id in b.store.state.tabs, front)).toBe(false)
  await expect.poll(() => signetApp.signet((b) => b.win?.isVisible())).toBe(true)
})

test('horizontal swipe: a long gesture goes back, a short or vertical one does not navigate', async ({ signetApp, site }) => {
  const id = await openTab(signetApp, site.url('page-a'), 'page-a')
  await expect.poll(() => signetApp.signet((b, i) => b.views.viewOf(i)?.getBounds().width ?? 0, id)).toBeGreaterThan(100)
  await signetApp.signet((b, i) => b.views.viewOf(i)!.webContents.loadURL(b.store.state.tabs[i].url.replace('page-a', 'page-b')), id)
  await expect.poll(async () => (await signetApp.snapshot()).tabs[id]?.title).toBe('page-b')
  // Real wheel events (Electron's sign is the opposite of the DOM's: we send -dx).
  const wheel = (steps: number, dx: number, dy = 0) =>
    signetApp.signet(
      async (b, a) => {
        const wc = b.views.viewOf(a.id)!.webContents
        const { width, height } = b.views.viewOf(a.id)!.getBounds()
        for (let i = 0; i < a.steps; i++) {
          wc.sendInputEvent({
            type: 'mouseWheel',
            x: Math.round(width / 2),
            y: Math.round(height / 2),
            deltaX: -a.dx,
            deltaY: -a.dy,
            wheelTicksX: -a.dx / 40,
            wheelTicksY: -a.dy / 40,
            hasPreciseScrollingDeltas: true,
            canScroll: true
          })
          await new Promise((r) => setTimeout(r, 8))
        }
      },
      { id, steps, dx, dy }
    )
  // Short gesture (50 px): no navigation.
  await wheel(5, -10)
  await signetApp.signet((b, i) => b.views.viewOf(i)!.webContents.executeJavaScript('new Promise((r) => setTimeout(r, 900))'), id)
  expect((await signetApp.snapshot()).tabs[id]?.title).toBe('page-b')
  // Vertical scroll with a little horizontal drift: no navigation.
  await wheel(30, -2, 20)
  await signetApp.signet((b, i) => b.views.viewOf(i)!.webContents.executeJavaScript('new Promise((r) => setTimeout(r, 900))'), id)
  expect((await signetApp.snapshot()).tabs[id]?.title).toBe('page-b')
  // Long gesture (≈ 270 px) to the right: back to the previous page.
  await wheel(27, -10)
  await expect.poll(async () => (await signetApp.snapshot()).tabs[id]?.title, { timeout: 5000 }).toBe('page-a')
})

import { test, expect, launch, quit, uiPage, openTab } from './fixtures'
import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'

test('starts with a fresh profile: window, default space, sidebar', async ({ signetApp }) => {
  const snap = await signetApp.snapshot()
  expect(snap.spaces).toHaveLength(1)
  expect(snap.activeSpaceId).toBe(snap.spaces[0].id)
  expect(snap.sidebarVisible).toBe(true)
  expect(await signetApp.signet((b) => b.win?.isVisible())).toBe(true)
  // The sidebar is a real UI page, with the preload library.
  await expect.poll(() => uiPage(signetApp, '')?.evaluate(() => typeof (window as unknown as { signet?: unknown }).signet)).toBe('object')
})

test('the palette opens as an overlay and answers a search', async ({ signetApp, site }) => {
  await openTab(signetApp, site.url('zebra-page'), 'zebra-page')
  await signetApp.signet((b) => b.overlay.openPalette('new-tab'))
  await expect.poll(async () => (await signetApp.snapshot()).overlayOpen).toBe(true)
  await expect.poll(() => !!uiPage(signetApp, '#overlay')).toBe(true)
  const overlay = uiPage(signetApp, '#overlay')!
  const input = overlay.getByRole('combobox')
  await input.fill('zebra')
  await expect(overlay.getByRole('option').filter({ hasText: 'zebra-page' }).first()).toBeVisible()
  await input.press('Escape')
  await expect.poll(async () => (await signetApp.snapshot()).overlayOpen).toBe(false)
})

test('the Settings window opens', async ({ signetApp }) => {
  await signetApp.signet((b) => b.settings.open())
  await expect.poll(() => !!uiPage(signetApp, '#settings')).toBe(true)
  await expect(uiPage(signetApp, '#settings')!.locator('[data-section]').first()).toBeAttached()
})

test('private window: no history and no saved tabs', async ({ profile, site }) => {
  const a = await launch(profile)
  await openTab(a, site.url('visible-page'), 'visible-page')
  const info = await a.signet(async (b, url) => {
    const win = b.privates.open(url)
    await new Promise((r) => setTimeout(r, 1500))
    return { has: b.privates.has(win), tabs: b.privates.tabsOf(win).length }
  }, site.url('secret-private-page'))
  expect(info).toEqual({ has: true, tabs: 1 })
  await expect.poll(() => !!uiPage(a, '#private')).toBe(true)
  // Private tabs are "detached": absent from the state sent to the UI.
  const titles = Object.values((await a.snapshot()).tabs).map((t) => t.title)
  expect(titles).not.toContain('secret-private-page')
  await a.signet((b) => b.history.flush())
  expect(await quit(a)).toBe(true)
  for (const f of ['state.json', 'history.json', 'tab-history.json']) {
    const file = join(profile, f)
    if (existsSync(file)) expect(readFileSync(file, 'utf8')).not.toContain('secret-private-page')
  }
  // Control: the normal page, by contrast, is saved.
  expect(readFileSync(join(profile, 'state.json'), 'utf8')).toContain('visible-page')
})

test('quits cleanly: the process exits on its own', async ({ profile }) => {
  const a = await launch(profile)
  const proc = a.app.process() // taken before exit: once the app is closed, Playwright releases the object
  expect(await quit(a)).toBe(true)
  expect(proc.exitCode).toBe(0)
})

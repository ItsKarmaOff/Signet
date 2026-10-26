import { mkdirSync } from 'node:fs'
import { join } from 'node:path'
import { test, expect, launch, quit, uiPage } from './fixtures'

// Screenshots of each step (to review the design): folder to provide via SIGNET_WELCOME_SHOTS.
const SHOTS = process.env.SIGNET_WELCOME_SHOTS

test('first launch: the introduction opens, saves the chosen engine, then does not come back', async ({ profile }) => {
  // SIGNET_NO_WELCOME empty: the fresh profile opens the introduction.
  const a = await launch(profile, [], { SIGNET_NO_WELCOME: '' })
  const intro = () => uiPage(a, '#welcome')
  await expect.poll(() => !!intro(), { timeout: 15_000 }).toBe(true)
  const page = intro()!
  const shot = async (name: string) => {
    if (!SHOTS) return
    mkdirSync(SHOTS, { recursive: true })
    await page.screenshot({ path: join(SHOTS, `${name}.png`) })
  }
  expect(await a.signet((b) => b.store.state.settings.onboarded)).toBe(false)

  // 1. Language: switching to French re-renders the introduction.
  await expect(page.getByRole('heading', { name: 'Welcome to Signet' })).toBeVisible()
  await shot('1-language-en')
  await page.locator('select.st-select').selectOption('fr')
  await expect(page.getByRole('heading', { name: 'Bienvenue dans Signet' })).toBeVisible()
  await shot('1-language-fr')
  await page.locator('select.st-select').selectOption('en')
  await expect(page.getByRole('heading', { name: 'Welcome to Signet' })).toBeVisible()
  await page.getByRole('button', { name: 'Continue' }).click()

  // 2. Search engine.
  await page.getByRole('radio', { name: 'DuckDuckGo' }).click()
  await expect(page.getByRole('radio', { name: 'DuckDuckGo' })).toHaveAttribute('aria-checked', 'true')
  await expect.poll(() => a.signet((b) => b.store.state.settings.searchEngine)).toBe('duckduckgo')
  await shot('2-engine')
  await page.getByRole('button', { name: 'Continue' }).click()

  // 3. Appearance: applied live.
  await page.getByRole('radio', { name: 'Dark' }).click()
  await page.getByRole('radio', { name: 'On top' }).click()
  await expect.poll(() => a.signet((b) => [b.store.state.settings.theme, b.store.state.settings.tabLayout])).toEqual(['dark', 'horizontal'])
  await shot('3-look')
  await page.getByRole('button', { name: 'Continue' }).click()

  // 4. Import: the test machine may or may not have another browser; the step can always be passed.
  await expect(page.getByRole('heading', { name: 'Bring your things along' })).toBeVisible()
  await shot('4-import')
  await page.keyboard.press('Enter') // Enter continues

  // 5. Ad blocker (turned off by SIGNET_E2E at start; the default browser only works when packaged: button greyed out).
  await page.getByRole('switch', { name: 'Block ads and trackers' }).click()
  await expect.poll(() => a.signet((b) => b.store.state.settings.adblock)).toBe(true)
  await shot('5-privacy')
  await page.getByRole('button', { name: 'Continue' }).click()

  // 6. Fin.
  await expect(page.getByRole('heading', { name: 'You are all set' })).toBeVisible()
  await shot('6-done')
  await page.getByRole('button', { name: 'Start browsing' }).click()
  await expect.poll(() => !!intro()).toBe(false)
  expect(await a.signet((b) => b.store.state.settings.onboarded)).toBe(true)
  expect(await quit(a)).toBe(true)

  // Relaunch on the same profile: settings saved, no more introduction.
  const r = await launch(profile, [], { SIGNET_NO_WELCOME: '' })
  try {
    const s = await r.signet((b) => b.store.state.settings)
    expect(s).toMatchObject({ onboarded: true, searchEngine: 'duckduckgo', theme: 'dark', tabLayout: 'horizontal' })
    await new Promise((res) => setTimeout(res, 1500))
    expect(uiPage(r, '#welcome')).toBeUndefined()
  } finally {
    await quit(r)
  }
})

test('first launch: Escape skips the introduction', async ({ profile }) => {
  const a = await launch(profile, [], { SIGNET_NO_WELCOME: '' })
  try {
    await expect.poll(() => !!uiPage(a, '#welcome'), { timeout: 15_000 }).toBe(true)
    await expect(uiPage(a, '#welcome')!.getByRole('heading', { name: 'Welcome to Signet' })).toBeVisible()
    // The window closes during the call: its reply never arrives.
    await uiPage(a, '#welcome')!
      .keyboard.press('Escape')
      .catch(() => {})
    await expect.poll(() => a.signet((b) => b.store.state.settings.onboarded)).toBe(true)
    await expect.poll(() => !!uiPage(a, '#welcome')).toBe(false)
  } finally {
    await quit(a)
  }
})

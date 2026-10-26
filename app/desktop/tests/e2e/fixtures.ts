import { test as base, _electron as electron, expect, type ElectronApplication, type Page } from '@playwright/test'
import { createServer, type Server } from 'node:http'
import { mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import type { Browser } from '../../src/main/browser/browser'
import type { Snapshot } from '../../src/shared/types'

const MAIN = resolve(process.env.SIGNET_E2E_MAIN ?? 'out-e2e/main/index.js')

/** Page locale : titre `title`, corps `body` (HTML libre). */
export const dataPage = (title: string, body = ''): string =>
  `data:text/html;charset=utf-8,${encodeURIComponent(`<!doctype html><title>${title}</title><body><h1>${title}</h1>${body}</body>`)}`

export interface App {
  app: ElectronApplication
  profile: string
  /** Runs `fn` in the main process, with the `Browser` as first argument. */
  signet: <R, A = undefined>(fn: (b: Browser, arg: A) => R | Promise<R>, arg?: A) => Promise<R>
  snapshot: () => Promise<Snapshot>
}

/** Launches the app with a given profile (created if missing). `SIGNET_E2E=1` exposes `globalThis.__signet` (see src/main/index.ts). */
export async function launch(profile: string, extraArgs: string[] = [], extraEnv: Record<string, string> = {}): Promise<App> {
  // ELECTRON_RUN_AS_NODE (inherited from some terminals) would start Electron as a plain Node.
  // SIGNET_NO_WELCOME: without it, a fresh profile would open the first-launch introduction (see welcome.spec.ts).
  const env = { ...process.env, SIGNET_E2E: '1', SIGNET_NO_WELCOME: '1', ELECTRON_ENABLE_LOGGING: '0', ...extraEnv } as Record<string, string>
  delete env.ELECTRON_RUN_AS_NODE
  const app = await electron.launch({
    args: [MAIN, `--user-data-dir=${profile}`, ...(process.platform === 'linux' ? ['--no-sandbox'] : []), ...extraArgs],
    env,
    timeout: 30_000
  })
  const signet = ((fn: (b: Browser, arg: unknown) => unknown, arg?: unknown) =>
    app.evaluate(
      (_e, [src, a]) => {
        const b = (globalThis as { __signet?: unknown }).__signet
        if (!b) throw new Error('globalThis.__signet absent (SIGNET_E2E=1 ?)')
        return (0, eval)(`(${src})`)(b, a)
      },
      [fn.toString(), arg] as [string, unknown]
    )) as App['signet']
  const ready = async () => {
    await expect.poll(() => app.evaluate(() => !!(globalThis as { __signet?: { win?: unknown } }).__signet?.win), { timeout: 20_000 }).toBe(true)
  }
  await ready()
  return { app, profile, signet, snapshot: () => signet((b) => b.broadcast.snapshot()) }
}

/** Page (UI view) whose URL contains `fragment` (`#overlay`, `#settings`, `#private`...); `''` = main window sidebar. */
export function uiPage(a: App, fragment: string): Page | undefined {
  return a.app.windows().find((p) => p.url().startsWith('signetui://') && (fragment ? p.url().includes(fragment) : !p.url().includes('#')))
}

/** Opens an active tab on `url` and waits until its title is known. Returns its id. */
export async function openTab(a: App, url: string, title: string): Promise<string> {
  const id = await a.signet((b, u) => b.tabs.open(u, { activate: true }), url)
  await expect.poll(async () => (await a.snapshot()).tabs[id]?.title).toBe(title)
  return id
}

/** Closes cleanly (app.quit) and waits for the process to end; force-kills if the app does not respond. Returns true if it exited on its own. */
export async function quit(a: App): Promise<boolean> {
  const proc = a.app.process()
  const exited = new Promise<void>((r) => (proc.exitCode !== null ? r() : proc.once('exit', () => r())))
  await a.app.evaluate(({ app }) => app.quit()).catch(() => {})
  let forced = false
  const timer = setTimeout(() => ((forced = true), proc.kill('SIGKILL')), 15_000)
  await exited
  clearTimeout(timer)
  return !forced
}

export const test = base.extend<{ profile: string; signetApp: App; site: { url: (path?: string) => string } }>({
  // eslint-disable-next-line no-empty-pattern
  profile: async ({}, use) => {
    const dir = mkdtempSync(join(tmpdir(), 'signet-e2e-'))
    await use(dir)
    rmSync(dir, { recursive: true, force: true })
  },
  signetApp: async ({ profile }, use, info) => {
    const a = await launch(profile)
    await a.app.context().tracing.start({ screenshots: false, snapshots: false })
    await use(a)
    if (info.status !== info.expectedStatus)
      await a.app
        .context()
        .tracing.stop({ path: info.outputPath('trace.zip') })
        .catch(() => {})
    await quit(a)
  },
  // Small local server: /<name> returns a page whose title is <name>.
  // eslint-disable-next-line no-empty-pattern
  site: async ({}, use) => {
    const server: Server = createServer((req, res) => {
      const name = decodeURIComponent((req.url ?? '/').slice(1).split('?')[0]) || 'index'
      res.writeHead(200, { 'content-type': 'text/html; charset=utf-8' })
      res.end(`<!doctype html><title>${name}</title><body><h1>${name}</h1></body>`)
    })
    await new Promise<void>((r) => server.listen(0, '127.0.0.1', () => r()))
    const port = (server.address() as { port: number }).port
    await use({ url: (path = 'index') => `http://127.0.0.1:${port}/${path}` })
    server.close()
  }
})

export { expect }

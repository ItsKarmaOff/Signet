// End-to-end tests: builds the app into `out-e2e/` (never into `out/`, which `npm run dev` rewrites) then runs Playwright.
// Usage: npm run test:e2e [-- <Playwright options>]   (SIGNET_E2E_SKIP_BUILD=1 reuses the last build)
import { spawnSync } from 'node:child_process'
import { existsSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = join(dirname(fileURLToPath(import.meta.url)), '..')
// npm workspace: executables are hoisted to the repository root (node_modules/.bin), unless there is a version conflict.
const bin = (name) => {
  const file = process.platform === 'win32' ? `${name}.cmd` : name
  return [join(root, 'node_modules', '.bin', file), join(root, '..', '..', 'node_modules', '.bin', file)].find(existsSync) ?? file
}
const run = (cmd, args) => spawnSync(cmd, args, { cwd: root, stdio: 'inherit', shell: process.platform === 'win32' }).status ?? 1

if (process.env.SIGNET_E2E_SKIP_BUILD !== '1' || !existsSync(join(root, 'out-e2e', 'main', 'index.js'))) {
  const status = run(bin('electron-vite'), ['build', '--outDir', 'out-e2e'])
  if (status !== 0) process.exit(status)
}
process.exit(run(bin('playwright'), ['test', ...process.argv.slice(2)]))

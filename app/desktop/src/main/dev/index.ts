// Development tools: dynamically imported by `main/index.ts` when the app is not packaged (`dev-*.js` chunk,
// excluded from the package by electron-builder.mjs). Each tool is driven by `SIGNET_*` environment variables (docs/DEVELOPMENT.md).
import type { Browser } from '../browser/browser'
import type { Store } from '../core/store'
import { devCapture } from './capture'
import { devSetup } from './seed'

/** Before the window opens: forced theme (SIGNET_THEME) and sample state (SIGNET_SEED). */
export function setup(store: Store): void {
  devSetup(store)
}

/** After the window opens: screenshot of the interface (SIGNET_CAPTURE). */
export function start(b: Browser): void {
  devCapture(b)
}

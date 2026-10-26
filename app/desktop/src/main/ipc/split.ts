import type { Browser } from '../browser/browser'
import type { Registrar } from './registrar'

/** Split view. */
export function registerSplitIpc(b: Browser, { handle }: Registrar): void {
  handle('focusPane', (_c, id) => b.splitView.focusPane(id))
  handle('unsplit', (_c, id) => b.splitView.unsplit(id))
  handle('setSplitLayout', (_c, id, layout) => b.splitView.setLayout(id, layout))
  handle('setSplitSizes', (_c, id, sizes, live) => b.splitView.setSizes(id, sizes, live === true))
}

import type { Browser } from '../browser/browser'
import { paletteExecute, paletteQuery, paletteSuggest } from '../browser/palette'
import type { Registrar } from './registrar'

/** Command bar, overlay (popovers, announcements), Peek, in-page search. */
export function registerOverlayIpc(b: Browser, { handle }: Registrar): void {
  handle('openPalette', (_c, mode, anchor) => b.overlay.openPalette(mode, anchor))
  handle('openPopover', (_c, popover, anchor) => b.overlay.openPopover(popover, anchor))
  handle('closeOverlay', (_c, instant) => b.overlay.close(instant === true))
  // The same signal serves the small announcement view, the floating sidebar and the overlay.
  handle('overlayReady', ({ sender }) => {
    if (b.undo.markToastReady(sender) || b.floating.markReady(sender)) return
    b.overlay.hello(sender)
  })
  handle('toastDone', () => b.undo.toastDone())
  handle('paletteQuery', (_c, q, mode) => paletteQuery(b, q, mode))
  handle('paletteSuggest', (_c, q) => paletteSuggest(q))
  handle('paletteExecute', (_c, result, mode) => paletteExecute(b, result, mode))
  handle('expandPeek', () => b.peek.expand())
  handle('find', (_c, q, opts) => b.find.search(q, opts))
  handle('closeFind', () => b.find.dismiss())
}

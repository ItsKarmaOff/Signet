import type { Browser } from '../browser/browser'
import { renameTab, setIcon } from '../windows/library'
import { showTabMenu } from '../app/menu'
import type { Registrar } from './registrar'

/** Tabs: activation, navigation, capture, preview, drag and drop, title and icon, media. */
export function registerTabsIpc(b: Browser, { handle }: Registrar): void {
  handle('activateTab', (_c, id) => b.tabs.activate(id))
  handle('closeTab', (_c, id) => b.tabs.close(id))
  handle('navigate', (_c, id, input) => b.tabs.navigate(id, input))
  handle('goBack', ({ sender }, id) => b.tabs.goBack(id ?? b.tabs.idForSender(sender)))
  handle('goForward', ({ sender }, id) => b.tabs.goForward(id ?? b.tabs.idForSender(sender)))
  handle('reload', ({ sender }, id) => b.tabs.reloadOrStop(id ?? b.tabs.idForSender(sender)))
  handle('toggleMute', (_c, id) => b.tabs.toggleMute(id))
  handle('resetPinned', (_c, id) => b.tabs.resetPinned(id))
  handle('toggleReader', (_c, id) => b.reader.toggle(id))
  handle('toggleTranslation', (_c, id) => b.translator.toggle(id))
  // Malformed capture: `{ ok: false }` rather than an error (the capture tool expects a reply).
  handle('takeCapture', (_c, id, target, action) => b.capture.take(id, target, action), { onInvalid: () => ({ ok: false }) })
  handle('showTabMenu', (_c, id) => showTabMenu(b, id))
  handle('moveTabStep', (_c, id, dir) => b.arrange.moveTabStep(id, dir))
  // "New" menu (tab, note, Easel) of the sidebar.
  handle('showNewMenu', () => b.notes.showNewMenu())
  handle('moveItem', (_c, source, target) => b.arrange.moveItem(source, target))
  handle('renameTab', (_c, id, title) => renameTab(b, id, title))
  handle('setIcon', (_c, target, emoji) => setIcon(b, target, emoji))

  // Tab preview on hover (sidebar, tab row): the card sits next to the hovered element.
  handle('showTabPreview', ({ sender }, tabId, rect, below) => b.previews.show(sender, tabId, rect, below))
  handle('hideTabPreview', () => b.previews.hide())

  // Drag a sidebar tab onto the page
  handle('beginTabDrag', (_c, tabId) => b.drag.begin(tabId))
  handle('endTabDrag', () => b.drag.end())
  handle('dropOnContent', (_c, tabId, zone) => b.drag.dropOnContent(tabId, zone))

  handle('mediaToggle', (_c, id) => b.media.toggle(id))
  handle('mediaSkip', (_c, id, direction) => b.media.skip(id, direction))
}

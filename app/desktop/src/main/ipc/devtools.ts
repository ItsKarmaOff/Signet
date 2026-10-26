import type { Browser } from '../browser/browser'
import type { Registrar } from './registrar'

/** Built-in developer tools, Develop menu, responsive mode. */
export function registerDevtoolsIpc(b: Browser, { handle }: Registrar): void {
  handle('setDevToolsDock', (_c, id, dock) => b.devtools.setDock(id, dock))
  handle('closeDevTools', (_c, id) => b.devtools.close(id))
  handle('setDevToolsSize', (_c, id, size, done) => b.devtools.setSize(id, size, done))
  handle('showDevToolsMenu', (_c, id) => b.devtools.showMenu(id))
  handle('showConsole', (_c, id) => b.devtools.showConsole(id))
  handle('showDevelopMenu', (_c, id) => b.develop.showMenu(id))
  handle('setResponsive', (_c, id, r) => b.develop.setResponsive(id, r))
  handle('setResponsiveDragging', (_c, id, dragging) => b.develop.setDragging(id, dragging))
  handle('devToolsWindowReady', ({ sender }) => b.devtools.windowState(sender))
}

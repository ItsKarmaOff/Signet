import { BrowserWindow, Menu } from 'electron'
import type { Browser } from '../browser/browser'
import { guardMenu } from '../app/menuguard'
import { currentLocale } from '../app/i18n'
import { appearanceOf } from '../app/appearance'
import { logUiError } from '../app/uilog'
import { fallbackFavicon } from '../web/favicons'
import type { Registrar } from './registrar'

/** State, language, interface appearance, error log, app menu. */
export function registerAppIpc(b: Browser, { handle }: Registrar): void {
  handle('getState', () => b.broadcast.snapshot())
  handle('getLocale', () => currentLocale())
  handle('getAppearance', () => appearanceOf(b.store.state.settings))
  handle('fallbackFavicon', (_c, url) => fallbackFavicon(b, url))
  // The three fields are permissive in the contract: `logUiError` always receives strings.
  handle('reportUiError', (_c, route, message, stack) =>
    logUiError(typeof route === 'string' ? route : '?', typeof message === 'string' ? message : '?', typeof stack === 'string' ? stack : '')
  )
  handle('showAppMenu', ({ sender }) => {
    const win = BrowserWindow.fromWebContents(sender) ?? b.win
    const menu = Menu.getApplicationMenu()
    if (menu) guardMenu(menu).popup(win && !win.isDestroyed() ? { window: win } : undefined)
  })
}

import type { Browser } from '../browser/browser'
import { registerAppIpc } from './app'
import { registerCustomizeIpc } from './customize'
import { registerDevtoolsIpc } from './devtools'
import { registerExtensionsIpc } from './extensions'
import { registerLibraryIpc } from './library'
import { registerLinkIpc } from './link'
import { registerOverlayIpc } from './overlay'
import { registerPageIpc } from './page'
import { registerPasswordsIpc } from './passwords'
import { createRegistrar, type Registrar } from './registrar'
import { registerSettingsIpc } from './settings'
import { registerSidebarIpc } from './sidebar'
import { registerSpacesIpc } from './spaces'
import { registerSplitIpc } from './split'
import { registerTabsIpc } from './tabs'
import { registerWindowsIpc } from './windows'

/**
 * Registers all IPC channels. Interface: one file per domain, via the registrar (guard and contract schemas).
 * Web pages: `registerPageIpc`, separately. Returns the registrar (its channel list is used by tests).
 */
export function registerIpc(b: Browser, dev: boolean): Registrar {
  const reg = createRegistrar(b, dev)
  registerAppIpc(b, reg)
  registerTabsIpc(b, reg)
  registerSplitIpc(b, reg)
  registerSpacesIpc(b, reg)
  registerSidebarIpc(b, reg)
  registerOverlayIpc(b, reg)
  registerDevtoolsIpc(b, reg)
  registerLibraryIpc(b, reg)
  registerSettingsIpc(b, reg)
  registerExtensionsIpc(b, reg)
  registerPasswordsIpc(b, reg)
  registerLinkIpc(b, reg)
  registerCustomizeIpc(b, reg)
  registerWindowsIpc(b, reg)
  registerPageIpc(b)
  return reg
}

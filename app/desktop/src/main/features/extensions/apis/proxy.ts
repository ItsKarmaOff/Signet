import type { Session } from 'electron'
import { parseProxyConfig, SYSTEM_PROXY, toElectronProxy, type ChromeProxyConfig } from './proxyconfig'
import type { SettingDef } from './settings'

/** `chrome.proxy.settings`: applied with `session.setProxy`; the default is the system proxy. */
export const proxySetting: SettingDef = {
  key: 'proxy.settings',
  permission: 'proxy',
  defaultValue: SYSTEM_PROXY,
  parse: parseProxyConfig,
  apply: (ses: Session, value: unknown) => ses.setProxy(toElectronProxy(value as ChromeProxyConfig))
}

// `chrome.proxy.onProxyError` is declared in the preload and never fired for now: Chromium reports PAC errors through
// a channel Electron does not surface.

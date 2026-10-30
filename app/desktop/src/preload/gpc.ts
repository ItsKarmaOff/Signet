/// <reference lib="dom" />
// Page script (registered on a session only while Global Privacy Control is on, all frames): defines
// `navigator.globalPrivacyControl` in the page's world, before its scripts. No IPC: the script exists only while the
// setting is on, so there is nothing to read. When the setting is off the property is absent, like in Chrome.
import { contextBridge } from 'electron'

try {
  contextBridge.executeInMainWorld({
    func: () => {
      const get = {
        get globalPrivacyControl(): boolean {
          return true
        }
      }
      const getter = Object.getOwnPropertyDescriptor(get, 'globalPrivacyControl')?.get
      if (!getter) return
      Object.defineProperty(Navigator.prototype, 'globalPrivacyControl', { get: getter, enumerable: true, configurable: true })
    }
  })
} catch (e) {
  console.error('Global Privacy Control: page script failed', e)
}

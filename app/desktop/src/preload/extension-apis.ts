/// <reference lib="dom" />
// Preload for extension contexts (an extension's service worker, or a chrome-extension:// page): adds the chrome.* APIs
// Electron lacks (proxy, privacy, alarms) and safe stubs for the ones a later phase implements (offscreen,
// declarativeNetRequest, runtime.getContexts). Calls go to main (src/main/features/extensions/apis/), which derives the
// extension from the sender.
//
// ORDER DECISION: electron-chrome-extensions defines its own stubs (privacy, downloads...) with defineProperty, then
// freezes `chrome`. If we ran first the library would overwrite our namespaces, so Extensions.attach registers this
// preload AFTER the library and we rely on the "frozen" path: copy the frozen `chrome` property descriptors (getters and
// lazy native bindings keep working, values are never read), override our namespaces, freeze the copy like the library
// does, and swap `globalThis.chrome`. If the swap is impossible we report `failed`. If we happen to run before the
// library (`first`), we define our namespaces in place; the library may then override some, which the terminal log shows.
import { contextBridge, ipcRenderer } from 'electron'
import {
  ALARM_EVENT,
  CRX_EVENT,
  CRX_INVOKE,
  INSTALLED_METHOD,
  isExtensionContext,
  NAMESPACE_PERMISSIONS,
  OFFSCREEN_REASONS,
  AUTH_EVENT,
  AUTH_RESPONSE_METHOD,
  AUTH_LISTENERS_METHOD,
  DNR_CONSTANTS,
  DNR_ENUMS,
  PRIVACY_SETTINGS,
  PROXY_ERROR_EVENT,
  SETTING_METHODS,
  type CrxResult,
  type InstallReport
} from '@shared/extension-apis'
import { EXTENSION_API_CATALOG } from '@shared/extension-api-catalog'
import { installApis, type Plan } from './extension-apis-install'

const isWorker = process.type === 'service-worker'

/**
 * Address of the current document or worker script. In a service worker preload `location` does not exist (the isolated
 * context has no WorkerGlobalScope), so it is read in the main world.
 */
function contextUrl(): string | null {
  try {
    if (isWorker) return contextBridge.executeInMainWorld({ func: () => String(self.location && self.location.href) }) as string
    return location.href
  } catch {
    return null
  }
}

/**
 * A sandboxed extension page (manifest `sandbox`) has an opaque origin and no extension APIs in Chrome: it must not get
 * them here either, or untrusted content running in it could call our APIs.
 */
function isSandboxed(): boolean {
  try {
    return !isWorker && location.origin === 'null'
  } catch {
    return true
  }
}

/** The worker's IPC handler can be registered a moment after its script starts: retry a few times. */
async function invoke(method: string, args: unknown[]): Promise<CrxResult> {
  for (let attempt = 0; ; attempt++) {
    try {
      return await ipcRenderer.invoke(CRX_INVOKE, method, args)
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e)
      if (attempt < 20 && /no handler registered/i.test(message)) {
        await new Promise((r) => setTimeout(r, 50))
        continue
      }
      return { ok: false, error: message }
    }
  }
}

if (isExtensionContext({ processType: process.type, url: contextUrl(), sandboxed: isSandboxed() })) {
  const context = isWorker ? 'worker' : 'page'
  const report = (r: Omit<InstallReport, 'context'>) => void invoke(INSTALLED_METHOD, [{ context, ...r }])
  try {
    const bridge = {
      invoke,
      subscribe: (callback: (name: string, args: unknown[]) => void): void => {
        ipcRenderer.on(CRX_EVENT, (_event, name: string, args: unknown[]) => callback(name, args))
      }
    }
    const plan: Plan = {
      settings: Object.fromEntries(Object.entries(PRIVACY_SETTINGS).map(([ns, list]) => [ns, Object.keys(list)])),
      settingMethods: SETTING_METHODS,
      alarmEvent: ALARM_EVENT,
      proxyErrorEvent: PROXY_ERROR_EVENT,
      namespacePermissions: NAMESPACE_PERMISSIONS,
      offscreenReasons: OFFSCREEN_REASONS,
      dnrEnums: DNR_ENUMS,
      dnrConstants: DNR_CONSTANTS,
      authEvent: AUTH_EVENT,
      authResponseMethod: AUTH_RESPONSE_METHOD,
      authListenersMethod: AUTH_LISTENERS_METHOD,
      catalog: EXTENSION_API_CATALOG
    }
    let result: Omit<InstallReport, 'context'>
    if (!isWorker && process.contextIsolated === false) {
      // No context isolation (some extension pages): preload and page share one world, so no bridge is needed (or possible).
      ;(globalThis as unknown as Record<string, unknown>).__signetCrx = bridge
      result = installApis(plan)
    } else {
      // The main world gets the bridge then deletes it from the global scope (see `installApis`).
      contextBridge.exposeInMainWorld('__signetCrx', bridge)
      result = contextBridge.executeInMainWorld({ func: installApis, args: [plan] }) as Omit<InstallReport, 'context'>
    }
    report(result)
  } catch (e) {
    console.error('[signet] extension APIs: injection failed', e)
    report({ namespaces: [], mode: 'failed', error: e instanceof Error ? e.message : String(e) })
  }
}

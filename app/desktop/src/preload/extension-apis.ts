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

function isTopFrame(): boolean {
  try {
    return isWorker || window.top === window
  } catch {
    return false
  }
}

/** What the main-world script needs to know: plain data only (it crosses the world boundary). */
interface Plan {
  settings: Record<string, string[]>
  settingMethods: readonly string[]
  alarmEvent: string
  proxyErrorEvent: string
  namespacePermissions: Record<string, readonly string[]>
  offscreenReasons: readonly string[]
  dnrEnums: Record<string, Record<string, string>>
  dnrConstants: Record<string, number | string>
  authEvent: string
  authResponseMethod: string
  authListenersMethod: string
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

if (isExtensionContext({ processType: process.type, url: contextUrl(), isTopFrame: isTopFrame() })) {
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
      authListenersMethod: AUTH_LISTENERS_METHOD
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

/**
 * Runs in the page's main world, so it must be self-contained (it is serialised with `toString`): no import, no helper
 * from outside.
 */
function installApis(plan: Plan): Omit<InstallReport, 'context'> {
  /* eslint-disable @typescript-eslint/no-explicit-any */
  const g = globalThis as any
  const bridge = g.__signetCrx
  try {
    delete g.__signetCrx
  } catch {
    // not deletable: harmless, the functions only reach main which checks the sender
  }
  if (!bridge) return { namespaces: [], mode: 'failed', error: 'bridge missing' }

  // ── chrome object ──
  if (!g.chrome) g.chrome = {}
  const original = g.chrome
  const frozen = !Object.isExtensible(original)
  let target = original
  if (frozen) {
    // Descriptors are copied, never read: native getters stay lazy and keep working.
    target = {}
    for (const key of Reflect.ownKeys(original)) {
      Object.defineProperty(target, key, { ...Object.getOwnPropertyDescriptor(original, key)!, configurable: true })
    }
  }

  // ── events ──
  class ExtensionEvent {
    private listeners = new Set<(...args: unknown[]) => unknown>()
    addListener(fn: (...args: unknown[]) => unknown): void {
      if (typeof fn === 'function') this.listeners.add(fn)
    }
    removeListener(fn: (...args: unknown[]) => unknown): void {
      this.listeners.delete(fn)
    }
    hasListener(fn: (...args: unknown[]) => unknown): boolean {
      return this.listeners.has(fn)
    }
    hasListeners(): boolean {
      return this.listeners.size > 0
    }
    dispatch(args: unknown[]): void {
      for (const fn of [...this.listeners]) {
        try {
          fn(...args)
        } catch (e) {
          console.error('Error in event handler:', e)
        }
      }
    }
  }
  const events = new Map<string, ExtensionEvent>()
  const event = (name: string): ExtensionEvent => {
    let e = events.get(name)
    if (!e) events.set(name, (e = new ExtensionEvent()))
    return e
  }

  // ── proxy authentication: main asks, our listeners answer ──
  // `webRequest` is native, but we cannot count on Electron delivering onAuthRequired for PROXY challenges, so main
  // handles the `login` event and sends the request here. Listeners registered through the wrapper below get it with
  // Chrome's signature `(details, callback)`; a returned value or promise (blocking style) works too.
  const authListeners = new Set<(...args: unknown[]) => unknown>()
  // Main must know which contexts have listeners (worker, popup, offscreen page...) to wait for the right answers.
  const announceAuthListeners = (): void => void bridge.invoke(plan.authListenersMethod, [authListeners.size])
  const isDecision = (r: any): boolean => !!r && typeof r === 'object' && (r.cancel === true || !!r.authCredentials)
  const runAuth = (key: string, details: unknown): void => {
    // A context without listeners stays silent: main does not wait for it.
    if (!authListeners.size) return
    let sent = false
    let remaining = authListeners.size
    let last: unknown = {}
    const send = (response: unknown): void => {
      if (sent) return
      sent = true
      bridge.invoke(plan.authResponseMethod, [key, response ?? {}])
    }
    // Several listeners in one context: credentials win at once, otherwise the answer is sent when all have spoken.
    const answer = (response: unknown): void => {
      if (sent) return
      if (isDecision(response) && (response as any).authCredentials) return send(response)
      if (isDecision(response)) last = response
      if (--remaining <= 0) send(last)
    }
    for (const fn of [...authListeners]) {
      let called = false
      const once = (r: unknown): void => {
        if (called) return
        called = true
        answer(r)
      }
      try {
        const returned = fn(details, once)
        // With a callback parameter (asyncBlocking) only the callback or a returned decision counts: an async function
        // returns a promise that resolves before the callback is called. Without one, the return value is the answer.
        const usesCallback = fn.length >= 2
        if (returned && typeof returned === 'object') {
          Promise.resolve(returned).then(
            (r) => (usesCallback ? isDecision(r) && once(r) : once(r)),
            () => !usesCallback && once({})
          )
        } else if (!usesCallback) once({})
      } catch (e) {
        console.error('Error in event handler:', e)
        once({})
      }
    }
  }
  bridge.subscribe((name: string, args: unknown[]) => (name === plan.authEvent ? runAuth(args[0] as string, args[1]) : events.get(name)?.dispatch(args)))

  // ── calls (callback or promise style, like MV3) ──
  const withLastError = (message: string, callback: (...args: unknown[]) => void): void => {
    const runtime = target.runtime
    let set = false
    try {
      Object.defineProperty(runtime, 'lastError', { get: () => ({ message }), configurable: true })
      set = true
    } catch {
      // runtime not extensible: the callback runs without lastError
    }
    try {
      callback()
    } finally {
      if (set) delete runtime.lastError
    }
    if (!set) console.error('Unchecked runtime.lastError:', message)
  }
  const call = (method: string, args: unknown[]): Promise<unknown> | undefined => {
    const rest = [...args]
    const callback = typeof rest[rest.length - 1] === 'function' ? (rest.pop() as (...a: unknown[]) => void) : undefined
    while (rest.length && rest[rest.length - 1] === undefined) rest.pop()
    const result: Promise<unknown> = bridge.invoke(method, rest).then((r: CrxResult) => {
      if (!r.ok) throw new Error(r.error)
      return r.value
    })
    if (!callback) return result
    result.then(
      (value) => callback(value),
      (e: Error) => withLastError(e.message, callback)
    )
    return undefined
  }
  const method =
    (name: string) =>
    (...args: unknown[]) =>
      call(name, args)
  const setting = (key: string) => ({
    ...Object.fromEntries(plan.settingMethods.map((m) => [m, method(`${key}.${m}`)])),
    onChange: event(`${key}.onChange`)
  })

  // ── install ──
  const assign = (dst: any, src: any, path: string): void => {
    for (const key of Object.keys(src)) {
      const value = src[key]
      try {
        Object.defineProperty(dst, key, { value, enumerable: true, configurable: true, writable: true })
      } catch {
        // Not configurable (native binding): merge into the existing object when we can.
        const current = dst[key]
        if (current && typeof current === 'object' && value && typeof value === 'object' && !(value instanceof ExtensionEvent))
          assign(current, value, `${path}.${key}`)
        else console.warn(`[signet] extension APIs: cannot replace chrome.${path}.${key}`)
      }
    }
  }
  const apis: Record<string, any> = {
    proxy: { settings: setting('proxy.settings'), onProxyError: event(plan.proxyErrorEvent) },
    privacy: Object.fromEntries(
      Object.entries(plan.settings).map(([ns, keys]) => [ns, Object.fromEntries(keys.map((k) => [k, setting(`privacy.${ns}.${k}`)]))])
    ),
    alarms: {
      create: method('alarms.create'),
      get: method('alarms.get'),
      getAll: method('alarms.getAll'),
      clear: method('alarms.clear'),
      clearAll: method('alarms.clearAll'),
      onAlarm: event(plan.alarmEvent)
    },
    // Real implementation in main (offscreen.ts): one hidden document per extension.
    offscreen: {
      Reason: Object.fromEntries(plan.offscreenReasons.map((r) => [r, r])),
      createDocument: method('offscreen.createDocument'),
      closeDocument: method('offscreen.closeDocument'),
      hasDocument: method('offscreen.hasDocument')
    },
    // Rules are stored (dynamic ones persist) but not enforced on the network yet, see the main side.
    declarativeNetRequest: {
      ...plan.dnrConstants,
      ...plan.dnrEnums,
      ...Object.fromEntries(
        [
          'updateDynamicRules',
          'getDynamicRules',
          'updateSessionRules',
          'getSessionRules',
          'getEnabledRulesets',
          'updateEnabledRulesets',
          'getAvailableStaticRuleCount',
          'isRegexSupported',
          'getMatchedRules'
        ].map((m) => [m, method(`declarativeNetRequest.${m}`)])
      )
    }
  }

  // Only the namespaces the manifest asks for, like Chrome. If the manifest cannot be read, all of them.
  let granted: string[] | null = null
  try {
    const manifest = target.runtime?.getManifest?.()
    if (manifest) granted = [...(manifest.permissions ?? []), ...(manifest.optional_permissions ?? [])].map(String)
  } catch {
    // keep null
  }
  const installed: string[] = []
  // webRequest stays native; only onAuthRequired is wrapped (copy of the namespace, like runtime below).
  const nativeWebRequest = target.webRequest
  if (nativeWebRequest && (!granted || plan.namespacePermissions.webRequest.some((p) => granted!.includes(p)))) {
    try {
      const nativeEvent = nativeWebRequest.onAuthRequired
      const overrides: Record<string, unknown> = {
        addListener: (fn: (...a: unknown[]) => unknown, ...rest: unknown[]) => {
          if (typeof fn !== 'function') return
          authListeners.add(fn)
          announceAuthListeners()
          try {
            nativeEvent?.addListener(fn, ...rest)
          } catch (e) {
            console.warn('[signet] extension APIs: native onAuthRequired.addListener failed', e)
          }
        },
        removeListener: (fn: (...a: unknown[]) => unknown) => {
          authListeners.delete(fn)
          announceAuthListeners()
          try {
            nativeEvent?.removeListener(fn)
          } catch {
            // already gone natively
          }
        },
        hasListener: (fn: (...a: unknown[]) => unknown) => authListeners.has(fn),
        hasListeners: () => authListeners.size > 0 || !!nativeEvent?.hasListeners?.()
      }
      const wrapped = new Proxy(nativeEvent ?? {}, {
        get: (t, k) => {
          if (typeof k === 'string' && k in overrides) return overrides[k]
          const v = Reflect.get(t, k)
          return typeof v === 'function' ? v.bind(t) : v
        }
      })
      const copy: any = {}
      for (const key of Reflect.ownKeys(nativeWebRequest)) {
        Object.defineProperty(copy, key, { ...Object.getOwnPropertyDescriptor(nativeWebRequest, key)!, configurable: true })
      }
      Object.defineProperty(copy, 'onAuthRequired', { value: wrapped, enumerable: true, configurable: true, writable: true })
      Object.defineProperty(target, 'webRequest', { value: copy, enumerable: true, configurable: true, writable: true })
      installed.push('webRequest.onAuthRequired')
    } catch (e) {
      console.warn('[signet] extension APIs: cannot wrap webRequest.onAuthRequired', e)
    }
  }
  for (const [name, api] of Object.entries(apis)) {
    const needs: readonly string[] = plan.namespacePermissions[name] ?? [name]
    if (granted && !needs.some((p) => granted!.includes(p))) continue
    assign(target, { [name]: api }, '')
    installed.push(name)
  }
  // Electron's native runtime.getContexts breaks in Chromium's runtime API ("Unexpected view type"): ours answers from main.
  // `chrome.runtime` is native (and spread into the library's own object), so on a copy we wrap it in a new object that
  // keeps every descriptor and only replaces getContexts.
  try {
    const runtime = target.runtime
    if (runtime) {
      const getContexts = method('runtime.getContexts')
      let next = runtime
      if (frozen || !Object.isExtensible(runtime)) {
        next = {}
        for (const key of Reflect.ownKeys(runtime)) {
          Object.defineProperty(next, key, { ...Object.getOwnPropertyDescriptor(runtime, key)!, configurable: true })
        }
        Object.defineProperty(target, 'runtime', { value: next, enumerable: true, configurable: true, writable: true })
      }
      Object.defineProperty(next, 'getContexts', { value: getContexts, enumerable: true, configurable: true, writable: true })
      installed.push('runtime.getContexts')
    }
  } catch (e) {
    console.warn('[signet] extension APIs: cannot override runtime.getContexts', e)
  }

  if (!frozen) return { namespaces: installed, mode: 'first' }

  // Same as the library: nobody changes `chrome` afterwards. Then swap the global for our copy.
  Object.freeze(target)
  try {
    Object.defineProperty(g, 'chrome', { value: target, writable: true, configurable: true, enumerable: true })
  } catch {
    // fall through to a plain assignment
  }
  if (g.chrome !== target) {
    try {
      g.chrome = target
    } catch {
      // checked below
    }
  }
  if (g.chrome !== target) {
    console.warn('[signet] extension APIs: chrome is frozen and the global cannot be replaced, APIs unavailable')
    return { namespaces: [], mode: 'failed', error: 'globalThis.chrome cannot be replaced' }
  }
  return { namespaces: installed, mode: 'replaced-frozen' }
  /* eslint-enable @typescript-eslint/no-explicit-any */
}

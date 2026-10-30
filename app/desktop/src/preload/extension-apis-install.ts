/// <reference lib="dom" />
// The code injected into an extension's main world (see extension-apis.ts for the why). Kept in its own module, with no
// runtime import, so that the unit tests can run it against a fake `chrome` object.
import type { CrxResult, InstallReport } from '@shared/extension-apis'
import type { NamespaceSpec } from '@shared/extension-api-catalog'

/** What the main-world script needs to know: plain data only (it crosses the world boundary). */
export interface Plan {
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
  /** Safety net: every namespace Signet knows from Chrome's reference (src/shared/extension-api-catalog.ts). */
  catalog?: Record<string, NamespaceSpec>
}

/**
 * Runs in the page's main world, so it must be self-contained (it is serialised with `toString`): no import, no helper
 * from outside.
 */
export function installApis(plan: Plan): Omit<InstallReport, 'context'> {
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
    // Real implementation in main (dnr.ts, dnr-engine.ts): rules are stored and enforced through the network hub.
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
          'getMatchedRules',
          'testMatchOutcome',
          'setExtensionActionOptions'
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
  // chrome.commands: Electron has none, and some workers read `commands.onCommand` at startup (uBlock Origin Lite crashes
  // without it). Extension shortcuts are not wired: getAll lists the manifest's commands without a shortcut, onCommand
  // never fires.
  if (!target.commands?.onCommand) {
    let declared: Record<string, { description?: unknown } | undefined> = {}
    try {
      declared = target.runtime?.getManifest?.()?.commands ?? {}
    } catch {
      // keep none
    }
    const list = () => Object.entries(declared).map(([name, c]) => ({ name, description: String(c?.description ?? ''), shortcut: '' }))
    assign(
      target,
      {
        commands: {
          getAll: (callback?: (r: unknown) => void) => {
            if (typeof callback !== 'function') return Promise.resolve(list())
            callback(list())
          },
          onCommand: event('commands.onCommand')
        }
      },
      ''
    )
    installed.push('commands')
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

  // ── safety net ──
  // Whatever Signet does not provide must not crash an extension: missing members are filled with placeholders that report
  // "not supported" (methods, events) or hold their constants; members that kill the process natively are replaced by
  // inert ones; `storage.managed` is an empty read-only area. Driven by the catalog (plan.catalog); installed on `target`
  // before it is frozen and before `browser` is aliased.
  const stubbed: string[] = []
  const NOT_SUPPORTED = (path: string): string => `chrome.${path} is not supported in Signet`
  const isObj = (v: unknown): boolean => v !== null && (typeof v === 'object' || typeof v === 'function')
  const clone = (v: any): any => (v !== null && typeof v === 'object' ? JSON.parse(JSON.stringify(v)) : v)
  const lastFn = (args: unknown[]): ((...a: unknown[]) => void) | undefined =>
    typeof args[args.length - 1] === 'function' ? (args[args.length - 1] as (...a: unknown[]) => void) : undefined
  /** Resolves `value` in callback or promise style. */
  const settle = (args: unknown[], value: unknown): Promise<unknown> | undefined => {
    const cb = lastFn(args)
    if (cb) {
      cb(clone(value))
      return undefined
    }
    return Promise.resolve(clone(value))
  }
  /** Fails in callback (runtime.lastError) or promise style. */
  const failWith = (args: unknown[], message: string): Promise<never> | undefined => {
    const cb = lastFn(args)
    if (cb) {
      withLastError(message, cb)
      return undefined
    }
    return Promise.reject(new Error(message))
  }
  const define = (obj: any, key: string, value: unknown): void => {
    Object.defineProperty(obj, key, { value, enumerable: true, configurable: true, writable: true })
  }
  const copyOf = (src: any): any => {
    // Descriptors are copied, never read: native getters stay lazy.
    const next = Object.create(Object.getPrototypeOf(src))
    for (const key of Reflect.ownKeys(src)) Object.defineProperty(next, key, { ...Object.getOwnPropertyDescriptor(src, key)!, configurable: true })
    return next
  }
  const read = (obj: any, key: string): any => {
    try {
      return obj[key]
    } catch {
      return undefined
    }
  }
  /** Existing object at a path, or undefined (never creates). */
  const existing = (segs: string[]): any => {
    let cur: any = target
    for (const seg of segs) {
      if (!isObj(cur) || !(seg in cur)) return undefined
      cur = read(cur, seg)
    }
    return isObj(cur) ? cur : undefined
  }
  /** Object at a path that can be written to: created when absent, copied when native or non-extensible (`force`: always). */
  const writable = (segs: string[], force = false): any => {
    let parent: any = target
    segs.forEach((seg, i) => {
      const cur = read(parent, seg)
      const last = i === segs.length - 1
      if (isObj(cur) && Object.isExtensible(cur) && !(force && last)) {
        parent = cur
        return
      }
      const next = isObj(cur) ? copyOf(cur) : {}
      define(parent, seg, next)
      parent = next
    })
    return parent
  }
  const stubEvent = (declarative: boolean): any => {
    const e: any = new ExtensionEvent()
    if (declarative) {
      e.addRules = (...a: unknown[]) => settle(a, [])
      e.getRules = (...a: unknown[]) => settle(a, [])
      e.removeRules = (...a: unknown[]) => settle(a, undefined)
    }
    return e
  }
  const stubClass = (name: string): any => {
    const ctor = function (this: any, options?: unknown): void {
      if (options && typeof options === 'object') Object.assign(this, options)
    }
    Object.defineProperty(ctor, 'name', { value: name })
    return ctor
  }

  // storage.managed: Chrome's enterprise policy area. The library aliases it to storage.local, which makes extensions read their own
  // data as if it were policy. Replaced by an empty read-only area (the one case, with crashers, where an existing member is replaced).
  const READ_ONLY = 'This is a read-only store.'
  const stubManaged = (): void => {
    const area: Record<string, unknown> = {
      get: (...a: unknown[]) => {
        const first = a[0]
        // Defaults object: Chrome returns the defaults for keys the area does not hold.
        return settle(a, first && typeof first === 'object' && !Array.isArray(first) && typeof first !== 'function' ? first : {})
      },
      getBytesInUse: (...a: unknown[]) => settle(a, 0),
      getKeys: (...a: unknown[]) => settle(a, []),
      set: (...a: unknown[]) => failWith(a, READ_ONLY),
      remove: (...a: unknown[]) => failWith(a, READ_ONLY),
      clear: (...a: unknown[]) => failWith(a, READ_ONLY),
      setAccessLevel: (...a: unknown[]) => failWith(a, READ_ONLY),
      onChanged: new ExtensionEvent()
    }
    try {
      define(writable(['storage']), 'managed', area)
    } catch {
      try {
        define(writable(['storage'], true), 'managed', area)
      } catch (e) {
        console.warn('[signet] extension APIs: cannot replace chrome.storage.managed', e)
      }
    }
  }

  // Which namespaces this extension may see: permissions and optional permissions, or a manifest key.
  let manifest: any = null
  try {
    manifest = target.runtime?.getManifest?.() ?? null
  } catch {
    // keep null: no stubs without a manifest
  }
  const unlocked = (spec: any): boolean => {
    if (!manifest || !granted || spec.contentScriptOnly) return false
    if (!spec.permissions.length && !spec.manifestKey) return true
    return spec.permissions.some((p: string) => granted!.includes(p)) || (!!spec.manifestKey && manifest[spec.manifestKey] !== undefined)
  }

  const fillSpec = (segs: string[], spec: any): void => {
    const path = segs.join('.')
    const crash = new Set<string>(spec.crashes ?? [])
    const soft: Record<string, unknown> = spec.soft ?? {}
    const declarative = new Set<string>(spec.declarativeEvents ?? [])
    const present = existing(segs)
    const wanted: { name: string; make: () => unknown; list: boolean }[] = []
    for (const name of spec.methods as string[]) {
      wanted.push({
        name,
        list: true,
        make: () => (name in soft ? (...a: unknown[]) => settle(a, soft[name]) : (...a: unknown[]) => failWith(a, NOT_SUPPORTED(`${path}.${name}`)))
      })
    }
    for (const name of spec.events as string[]) wanted.push({ name, list: true, make: () => stubEvent(declarative.has(name)) })
    for (const [name, value] of Object.entries((spec.constants ?? {}) as Record<string, unknown>)) wanted.push({ name, list: false, make: () => clone(value) })
    for (const name of (spec.classes ?? []) as string[]) wanted.push({ name, list: false, make: () => stubClass(name) })
    const todo = wanted.filter((w) => !(present && w.name in present) || crash.has(w.name))
    if (!todo.length) return
    let obj: any
    try {
      obj = writable(segs)
    } catch (e) {
      console.warn(`[signet] extension APIs: cannot fill chrome.${path}`, e)
      return
    }
    if (!present && todo.some((w) => w.list)) stubbed.push(path)
    for (const w of todo) {
      const replace = !!present && w.name in present
      try {
        define(obj, w.name, w.make())
      } catch {
        if (!replace) continue
        // Native member that cannot be redefined in place: copy the namespace object and replace it on its parent.
        try {
          define(writable(segs, true), w.name, w.make())
        } catch (e) {
          console.warn(`[signet] extension APIs: cannot replace chrome.${path}.${w.name}`, e)
          continue
        }
      }
      if (w.list && present) stubbed.push(`${path}.${w.name}`)
    }
  }
  const fillAll = (catalog: Record<string, any>, prefix: string[]): void => {
    for (const [name, spec] of Object.entries(catalog)) {
      const segs = [...prefix, name]
      try {
        if (unlocked(spec)) fillSpec(segs, spec)
        if (spec.children) fillAll(spec.children, segs)
      } catch (e) {
        console.warn(`[signet] extension APIs: safety net failed for chrome.${segs.join('.')}`, e)
      }
    }
  }
  if (granted && unlocked({ permissions: ['storage'] })) stubManaged()
  if (plan.catalog) fillAll(plan.catalog, [])

  // Library events: the electron-chrome-extensions stubs throw "Method not implemented" from hasListener / hasListeners (and
  // the rules methods). An extension calling them at startup dies. Such events get listener tracking instead.
  const fixedEvents: string[] = []
  const repairEvents = (): void => {
    const seen = new WeakSet<object>()
    const noop = (): void => {}
    const isEvent = (v: any): boolean => !!v && typeof v === 'object' && typeof v.addListener === 'function' && typeof v.removeListener === 'function'
    const throws = (f: () => unknown): boolean => {
      try {
        Promise.resolve(f()).catch(() => {})
        return false
      } catch {
        return true
      }
    }
    const repair = (ev: any, path: string): void => {
      const brokenListeners = throws(() => ev.hasListener(noop)) || (typeof ev.hasListeners === 'function' && throws(() => ev.hasListeners()))
      const brokenRules = typeof ev.getRules === 'function' && throws(() => ev.getRules(noop))
      if (!brokenListeners && !brokenRules) return
      try {
        if (brokenListeners) {
          const tracked = new Set<unknown>()
          const add = ev.addListener
          const remove = ev.removeListener
          define(ev, 'addListener', (fn: unknown, ...rest: unknown[]) => {
            if (typeof fn !== 'function') return
            tracked.add(fn)
            try {
              add.call(ev, fn, ...rest)
            } catch {
              // the library's own stub may throw: tracking is enough
            }
          })
          define(ev, 'removeListener', (fn: unknown) => {
            tracked.delete(fn)
            try {
              remove.call(ev, fn)
            } catch {
              // same
            }
          })
          define(ev, 'hasListener', (fn: unknown) => tracked.has(fn))
          define(ev, 'hasListeners', () => tracked.size > 0)
        }
        if (brokenRules) {
          define(ev, 'addRules', (...a: unknown[]) => settle(a, []))
          define(ev, 'getRules', (...a: unknown[]) => settle(a, []))
          define(ev, 'removeRules', (...a: unknown[]) => settle(a, undefined))
        }
        fixedEvents.push(path)
      } catch (e) {
        console.warn(`[signet] extension APIs: cannot repair chrome.${path}`, e)
      }
    }
    const walk = (obj: any, path: string, depth: number): void => {
      if (depth > 4 || seen.has(obj)) return
      seen.add(obj)
      let keys: (string | symbol)[]
      try {
        keys = Reflect.ownKeys(obj)
      } catch {
        return
      }
      for (const key of keys) {
        if (typeof key !== 'string') continue
        const v = read(obj, key)
        if (!v || typeof v !== 'object') continue
        if (isEvent(v)) repair(v, `${path}${key}`)
        else walk(v, `${path}${key}.`, depth + 1)
      }
    }
    walk(target, '', 0)
  }
  repairEvents()

  // Chromium also exposes a native `browser` namespace, a separate object with only the native APIs. Extensions that read
  // `self.browser || self.chrome` (uBlock Origin Lite) would miss everything added here: point it at the same object.
  const aliasBrowser = (): void => {
    if (g.browser === undefined || g.browser === target) return
    try {
      Object.defineProperty(g, 'browser', { value: target, writable: true, configurable: true, enumerable: true })
    } catch {
      // checked below
    }
    if (g.browser === target) installed.push('browser')
    else console.warn('[signet] extension APIs: globalThis.browser cannot be replaced, extensions using it miss these APIs')
  }

  if (!frozen) {
    aliasBrowser()
    return { namespaces: installed, mode: 'first', stubbed, fixedEvents }
  }

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
  aliasBrowser()
  return { namespaces: installed, mode: 'replaced-frozen', stubbed, fixedEvents }
  /* eslint-enable @typescript-eslint/no-explicit-any */
}

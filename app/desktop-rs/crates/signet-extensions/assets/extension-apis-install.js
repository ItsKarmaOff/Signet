// Builds the `chrome` object of an extension context. Port of app/desktop/src/preload/extension-apis-install.ts.
//
// BRIDGE ASSUMPTION: bridge.js is loaded first and provides globalThis.__signetCrx (see its header):
//   window.__signetPost(channel, json) and window.__signetDispatch(channel, json) are provided by signet-web.
//
// Difference with the Electron app: there, Chromium gave every extension context a native `chrome` and this script only
// added what was missing. With CEF in Alloy style there is no native extension system, so this script builds the WHOLE
// `chrome` object: every method the host implements (config.implemented, from ExtensionHost::implemented_methods) becomes a
// call over the bridge, the special cases below (runtime messaging, i18n, contextMenus onclick, action.setIcon imageData,
// onAuthRequired) are written by hand, and everything else in the catalog (api-catalog.json) gets the safety net:
// placeholders that report "not supported in Signet" instead of crashing the extension.
//
//   installApis(config, bridge, target) -> report { namespaces, mode, stubbed, fixedEvents }
//     config: { id, manifest, messages, implemented, catalog, plan, context: { kind: 'worker'|'page'|'content', tabId?, frameId? } }
//     target: the object that receives `chrome` and `browser` (globalThis in extension pages, a fresh object for content scripts)
//
// Pure JS with no browser API besides console, so it can be tested in Node with a fake bridge.
(function (root) {
  'use strict'

  function installApis(config, bridge, target) {
    var manifest = config.manifest || {}
    var plan = config.plan || {}
    var catalog = config.catalog || {}
    var kind = (config.context && config.context.kind) || 'page'
    var contentScript = kind === 'content'
    var implemented = new Set(config.implemented || [])
    var installed = []
    var stubbed = []

    // ---- events ----
    function ExtensionEvent() {
      this.listeners = []
    }
    ExtensionEvent.prototype.addListener = function (fn) {
      if (typeof fn === 'function' && this.listeners.indexOf(fn) < 0) this.listeners.push(fn)
    }
    ExtensionEvent.prototype.removeListener = function (fn) {
      var i = this.listeners.indexOf(fn)
      if (i >= 0) this.listeners.splice(i, 1)
    }
    ExtensionEvent.prototype.hasListener = function (fn) {
      return this.listeners.indexOf(fn) >= 0
    }
    ExtensionEvent.prototype.hasListeners = function () {
      return this.listeners.length > 0
    }
    ExtensionEvent.prototype.dispatch = function (args) {
      var list = this.listeners.slice()
      var results = []
      for (var i = 0; i < list.length; i++) {
        try {
          results.push(list[i].apply(null, args))
        } catch (e) {
          console.error('Error in event handler:', e)
        }
      }
      return results
    }
    var events = new Map()
    function event(name) {
      var e = events.get(name)
      if (!e) events.set(name, (e = new ExtensionEvent()))
      return e
    }

    // ---- calls (callback or promise style, like MV3) ----
    var runtimeObject = {}
    function withLastError(message, callback) {
      var set = false
      try {
        Object.defineProperty(runtimeObject, 'lastError', { get: function () { return { message: message } }, configurable: true })
        set = true
      } catch (e) {
        // not extensible
      }
      try {
        callback()
      } finally {
        if (set) delete runtimeObject.lastError
      }
      if (!set) console.error('Unchecked runtime.lastError:', message)
    }
    function call(method, args) {
      var rest = Array.prototype.slice.call(args)
      var callback = typeof rest[rest.length - 1] === 'function' ? rest.pop() : undefined
      while (rest.length && rest[rest.length - 1] === undefined) rest.pop()
      var result = bridge.call(method, rest).then(function (r) {
        if (!r.ok) throw new Error(r.error)
        return r.value
      })
      if (!callback) return result
      result.then(
        function (value) { callback(value) },
        function (e) { withLastError(e.message, callback) }
      )
      return undefined
    }
    function method(name) {
      return function () { return call(name, arguments) }
    }
    function settle(args, value) {
      var cb = typeof args[args.length - 1] === 'function' ? args[args.length - 1] : null
      var copy = value !== null && typeof value === 'object' ? JSON.parse(JSON.stringify(value)) : value
      if (cb) {
        cb(copy)
        return undefined
      }
      return Promise.resolve(copy)
    }
    function failWith(args, message) {
      var cb = typeof args[args.length - 1] === 'function' ? args[args.length - 1] : null
      if (cb) {
        withLastError(message, cb)
        return undefined
      }
      return Promise.reject(new Error(message))
    }

    // ---- the chrome object ----
    var chrome = {}
    function ns(path) {
      var cur = chrome
      var segs = path.split('.')
      for (var i = 0; i < segs.length; i++) {
        if (!Object.prototype.hasOwnProperty.call(cur, segs[i]) || cur[segs[i]] === null || typeof cur[segs[i]] !== 'object') cur[segs[i]] = {}
        cur = cur[segs[i]]
      }
      return cur
    }
    function define(obj, key, value) {
      Object.defineProperty(obj, key, { value: value, enumerable: true, configurable: true, writable: true })
    }

    // Which namespaces this extension may see: permissions and optional permissions, or a manifest key.
    var granted = [].concat(manifest.permissions || [], manifest.optional_permissions || []).map(String)
    function unlocked(spec) {
      if (spec.contentScriptOnly && !contentScript) return false
      if (contentScript && !spec.contentScriptOnly && !{ runtime: 1, storage: 1, i18n: 1, extension: 1 }[spec._name]) return false
      if (!spec.permissions.length && !spec.manifestKey) return true
      return spec.permissions.some(function (p) { return granted.indexOf(p) >= 0 }) || (!!spec.manifestKey && manifest[spec.manifestKey] !== undefined)
    }

    // 1. Everything the host implements becomes a call.
    implemented.forEach(function (name) {
      if (name.indexOf('__') >= 0) return
      var parts = name.split('.')
      var fn = parts.pop()
      var top = parts[0]
      if (contentScript && ['storage', 'i18n'].indexOf(top) < 0 && name.indexOf('runtime.') !== 0) return
      var spec = catalog[top]
      if (spec && !unlocked(Object.assign({ _name: top }, spec)) && top !== 'permissions') return
      define(ns(parts.join('.')), fn, method(name))
    })
    // browserAction mirrors action in MV2.
    if (manifest.browser_action && chrome.action) chrome.browserAction = chrome.action

    // 2. Settings (proxy, privacy): the objects have get/set/clear and onChange.
    function setting(key) {
      var s = {}
      ;(plan.settingMethods || ['get', 'set', 'clear']).forEach(function (m) { define(s, m, method(key + '.' + m)) })
      define(s, 'onChange', event(key + '.onChange'))
      return s
    }
    if (!contentScript) {
      if (granted.indexOf('proxy') >= 0) {
        ns('proxy').settings = setting('proxy.settings')
        define(chrome.proxy, 'onProxyError', event(plan.proxyErrorEvent || 'proxy.onProxyError'))
        installed.push('proxy')
      }
      if (granted.indexOf('privacy') >= 0) {
        Object.keys(plan.settings || {}).forEach(function (n) {
          plan.settings[n].forEach(function (k) { define(ns('privacy.' + n), k, setting('privacy.' + n + '.' + k)) })
        })
        installed.push('privacy')
      }
    }

    // 3. Constants Signet defines itself.
    if (granted.indexOf('offscreen') >= 0 && !contentScript) {
      var reasons = {}
      ;(plan.offscreenReasons || []).forEach(function (r) { reasons[r] = r })
      define(ns('offscreen'), 'Reason', reasons)
      installed.push('offscreen')
    }
    if ((granted.indexOf('declarativeNetRequest') >= 0 || granted.indexOf('declarativeNetRequestWithHostAccess') >= 0) && !contentScript) {
      var dnr = ns('declarativeNetRequest')
      Object.keys(plan.dnrConstants || {}).forEach(function (k) { define(dnr, k, plan.dnrConstants[k]) })
      Object.keys(plan.dnrEnums || {}).forEach(function (k) { define(dnr, k, plan.dnrEnums[k]) })
      installed.push('declarativeNetRequest')
    }

    // 4. runtime: id, URLs, manifest, messaging.
    var id = config.id
    var rt = ns('runtime')
    Object.keys(rt).forEach(function (k) { runtimeObject[k] = rt[k] })
    chrome.runtime = Object.assign(runtimeObject, rt)
    define(runtimeObject, 'id', id)
    define(runtimeObject, 'getManifest', function () { return JSON.parse(JSON.stringify(manifest)) })
    define(runtimeObject, 'getURL', function (path) { return 'chrome-extension://' + id + '/' + String(path || '').replace(/^\/+/, '') })
    define(runtimeObject, 'getBackgroundPage', function (cb) { return settle([cb], null) })
    define(runtimeObject, 'onInstalled', event('runtime.onInstalled'))
    define(runtimeObject, 'onStartup', event('runtime.onStartup'))
    define(runtimeObject, 'onSuspend', event('runtime.onSuspend'))
    define(runtimeObject, 'onUpdateAvailable', event('runtime.onUpdateAvailable'))
    define(runtimeObject, 'requestUpdateCheck', function () { return failWith(arguments, 'Autoupdate is not enabled.') })
    define(runtimeObject, 'PlatformOs', { MAC: 'mac', WIN: 'win', ANDROID: 'android', CROS: 'cros', LINUX: 'linux', OPENBSD: 'openbsd' })
    var ctx = config.context || {}
    // Messages: the host delivers `runtime.onMessage` with [message, sender, token]; a listener answers through sendResponse
    // or by returning true (async) / a promise.
    var onMessage = event('runtime.onMessage')
    define(runtimeObject, 'onMessage', onMessage)
    define(runtimeObject, 'onMessageExternal', event('runtime.onMessageExternal'))
    define(runtimeObject, 'onConnectExternal', event('runtime.onConnectExternal'))
    function reply(token, handled, hasListener, response) {
      bridge.call('runtime.__reply', [token, { handled: handled, hasListener: hasListener, response: response }])
    }
    bridge.subscribe(function (name, args) {
      if (name === 'runtime.onMessage') {
        var message = args[0], sender = args[1], token = args[2]
        var list = onMessage.listeners.slice()
        if (!list.length) return reply(token, false, false, null)
        var answered = false
        var async = false
        var sendResponse = function (response) {
          if (answered) return
          answered = true
          reply(token, true, true, response)
        }
        list.forEach(function (fn) {
          try {
            var r = fn(message, sender, sendResponse)
            if (r === true) async = true
            else if (r && typeof r.then === 'function') {
              async = true
              r.then(sendResponse, function () { sendResponse(undefined) })
            }
          } catch (e) {
            console.error('Error in event handler:', e)
          }
        })
        if (!async && !answered) reply(token, false, true, null)
        return
      }
      var e = events.get(name)
      if (e) e.dispatch(args)
    })
    define(runtimeObject, 'sendMessage', function () {
      var a = Array.prototype.slice.call(arguments)
      return call('runtime.sendMessage', a)
    })
    // Ports.
    var ports = new Map()
    function makePort(portId, name, sender) {
      var port = { name: name, sender: sender, onMessage: new ExtensionEvent(), onDisconnect: new ExtensionEvent() }
      port.postMessage = function (m) { bridge.call('runtime.__portPost', [portId, m]) }
      port.disconnect = function () {
        ports.delete(portId)
        bridge.call('runtime.__portClose', [portId])
      }
      ports.set(portId, port)
      return port
    }
    define(runtimeObject, 'onConnect', event('runtime.onConnect'))
    define(runtimeObject, 'connect', function () {
      var a = Array.prototype.slice.call(arguments)
      var info = typeof a[0] === 'object' && a[0] !== null ? a[0] : a[1] || {}
      var holder = {}
      var port = makePort(-1, info.name || '', undefined)
      bridge.call('runtime.connect', [id, { name: info.name || '' }]).then(function (r) {
        if (!r.ok) {
          port.onDisconnect.dispatch([port])
          return
        }
        ports.delete(-1)
        holder.id = r.value
        ports.set(r.value, port)
        port.postMessage = function (m) { bridge.call('runtime.__portPost', [r.value, m]) }
        port.disconnect = function () { ports.delete(r.value); bridge.call('runtime.__portClose', [r.value]) }
      })
      return port
    })
    bridge.subscribe(function (name, args) {
      if (name === 'runtime.onConnect') {
        var info = args[0]
        var list = runtimeObject.onConnect.listeners
        if (!list.length) return void bridge.call('runtime.__decline', [info.portId])
        var port = makePort(info.portId, info.name, info.sender)
        bridge.call('runtime.__accept', [info.portId])
        runtimeObject.onConnect.dispatch([port])
        return
      }
      var m = /^port\.(onMessage|onDisconnect|accepted):(\d+)$/.exec(name)
      if (!m) return
      var p = ports.get(Number(m[2]))
      if (!p) return
      if (m[1] === 'onMessage') p.onMessage.dispatch([args[0], p])
      else if (m[1] === 'onDisconnect') {
        ports.delete(Number(m[2]))
        p.onDisconnect.dispatch([p])
      }
    })
    // Tell the host this context exists (messaging targets, getContexts).
    bridge.call('runtime.__openContext', [{ contentScript: contentScript, tabId: ctx.tabId, frameId: ctx.frameId || 0 }])
    installed.push('runtime')

    // 5. i18n: messages are preloaded by the host.
    var messages = (config.messages && config.messages.messages) || {}
    var uiLanguage = ((config.messages && config.messages.locale) || 'en').replace('_', '-')
    var i18n = ns('i18n')
    define(i18n, 'getMessage', function (name, subs) {
      var entry = messages[String(name).toLowerCase()]
      if (!entry || typeof entry.message !== 'string') return ''
      var list = subs === undefined ? [] : [].concat(subs)
      var placeholders = {}
      Object.keys(entry.placeholders || {}).forEach(function (k) { placeholders[k.toLowerCase()] = entry.placeholders[k].content || '' })
      return entry.message.replace(/\$([A-Za-z0-9_@]+)\$|\$\$|\$([1-9])/g, function (all, n, digit) {
        if (n) {
          var c = placeholders[n.toLowerCase()]
          return c === undefined ? all : c.replace(/\$([1-9])/g, function (_a, d) { return list[Number(d) - 1] === undefined ? '' : String(list[Number(d) - 1]) })
        }
        if (digit) return list[Number(digit) - 1] === undefined ? '' : String(list[Number(digit) - 1])
        return '$'
      })
    })
    define(i18n, 'getUILanguage', function () { return uiLanguage })
    define(i18n, 'getAcceptLanguages', function () { return settle(arguments, [uiLanguage, uiLanguage.split('-')[0]]) })
    define(i18n, 'detectLanguage', function () { return settle(arguments, { isReliable: false, languages: [] }) })
    ns('extension')
    define(chrome.extension, 'getURL', runtimeObject.getURL)
    define(chrome.extension, 'getBackgroundPage', function () { return null })
    define(chrome.extension, 'getViews', function () { return [] })
    define(chrome.extension, 'isAllowedIncognitoAccess', function () { return settle(arguments, false) })
    define(chrome.extension, 'isAllowedFileSchemeAccess', function () { return settle(arguments, true) })
    if (contentScript) {
      finish()
      return { namespaces: ['runtime', 'storage', 'i18n', 'extension'], mode: 'first', stubbed: [], fixedEvents: [] }
    }

    // 6. storage events: area objects get their onChanged events.
    ;['local', 'session', 'sync', 'managed'].forEach(function (area) {
      if (chrome.storage && chrome.storage[area]) define(chrome.storage[area], 'onChanged', event('storage.' + area + '.onChanged'))
    })
    if (chrome.storage) {
      define(chrome.storage, 'onChanged', event('storage.onChanged'))
      define(chrome.storage.session || {}, 'QUOTA_BYTES', 10485760)
      define(chrome.storage.local || {}, 'QUOTA_BYTES', 10485760)
      define(chrome.storage.sync || {}, 'QUOTA_BYTES', 102400)
      define(chrome.storage.sync || {}, 'QUOTA_BYTES_PER_ITEM', 8192)
      define(chrome.storage.sync || {}, 'MAX_ITEMS', 512)
      installed.push('storage')
    }

    // 7. Hand written members: contextMenus onclick, action.setIcon imageData, onAuthRequired, events of the host.
    var clicks = new Map()
    var counter = 0
    var listening = false
    if (chrome.contextMenus && implemented.has('contextMenus.create')) {
      var onClicked = event('contextMenus.onClicked')
      define(chrome.contextMenus, 'onClicked', onClicked)
      define(chrome.contextMenus, 'create', function (props, callback) {
        var p = Object.assign({}, props)
        if (p.id === undefined) p.id = 'signet-menu-' + Date.now().toString(36) + '-' + ++counter
        if (typeof p.onclick === 'function') {
          clicks.set(String(p.id), p.onclick)
          if (!listening) {
            listening = true
            onClicked.addListener(function (info, tab) {
              var fn = clicks.get(String(info.menuItemId))
              if (fn) fn(info, tab)
            })
          }
        }
        delete p.onclick
        var promise = call('contextMenus.create', typeof callback === 'function' ? [p, callback] : [p])
        if (promise) promise.catch(function (e) { console.error('Unchecked runtime.lastError:', e.message) })
        return p.id
      })
      installed.push('contextMenus')
    }
    ;['action', 'browserAction'].forEach(function (name) {
      var obj = chrome[name]
      if (!obj) return
      define(obj, 'onClicked', event(name + '.onClicked'))
      var original = obj.setIcon
      define(obj, 'setIcon', function (details) {
        var data = details && details.imageData
        if (!data || typeof data !== 'object') return original ? original.apply(obj, arguments) : settle(arguments, undefined)
        var plain = function (img) { return { width: img.width, height: img.height, data: Array.from(img.data) } }
        var imageData = 'width' in data && 'data' in data ? plain(data) : Object.keys(data).reduce(function (o, k) { o[k] = plain(data[k]); return o }, {})
        return call('action.setIcon', [{ tabId: details.tabId, imageData: imageData }].concat(Array.prototype.slice.call(arguments, 1)))
      })
    })
    // webRequest.onAuthRequired: the host sends the request, listeners answer (details, callback) or by returning.
    if (granted.indexOf('webRequest') >= 0 || granted.indexOf('webRequestAuthProvider') >= 0) {
      var authListeners = []
      var announce = function () { bridge.call(plan.authListenersMethod || 'webRequest.authListeners', [authListeners.length]) }
      var authEvent = {
        addListener: function (fn) {
          if (typeof fn !== 'function') return
          authListeners.push(fn)
          announce()
        },
        removeListener: function (fn) {
          authListeners = authListeners.filter(function (f) { return f !== fn })
          announce()
        },
        hasListener: function (fn) { return authListeners.indexOf(fn) >= 0 },
        hasListeners: function () { return authListeners.length > 0 }
      }
      ns('webRequest')
      define(chrome.webRequest, 'onAuthRequired', authEvent)
      var isDecision = function (r) { return !!r && typeof r === 'object' && (r.cancel === true || !!r.authCredentials) }
      bridge.subscribe(function (name, args) {
        if (name !== (plan.authEvent || 'webRequest.onAuthRequired') || !authListeners.length) return
        var key = args[0], details = args[1]
        var sent = false
        var remaining = authListeners.length
        var last = {}
        var send = function (response) {
          if (sent) return
          sent = true
          bridge.call(plan.authResponseMethod || 'webRequest.authResponse', [key, response || {}])
        }
        var answer = function (response) {
          if (sent) return
          if (isDecision(response) && response.authCredentials) return send(response)
          if (isDecision(response)) last = response
          if (--remaining <= 0) send(last)
        }
        authListeners.slice().forEach(function (fn) {
          var called = false
          var once = function (r) { if (called) return; called = true; answer(r) }
          try {
            var returned = fn(details, once)
            var usesCallback = fn.length >= 2
            if (returned && typeof returned === 'object') {
              Promise.resolve(returned).then(
                function (r) { if (usesCallback) { if (isDecision(r)) once(r) } else once(r) },
                function () { if (!usesCallback) once({}) }
              )
            } else if (!usesCallback) once({})
          } catch (e) {
            console.error('Error in event handler:', e)
            once({})
          }
        })
      })
      installed.push('webRequest.onAuthRequired')
    }

    // 8. Safety net: whatever Signet does not provide must not crash an extension.
    var NOT_SUPPORTED = function (path) { return 'chrome.' + path + ' is not supported in Signet' }
    function stubClass(name) {
      var ctor = function (options) { if (options && typeof options === 'object') Object.assign(this, options) }
      Object.defineProperty(ctor, 'name', { value: name })
      return ctor
    }
    function stubEvent(declarative) {
      var e = new ExtensionEvent()
      if (declarative) {
        e.addRules = function () { return settle(arguments, []) }
        e.getRules = function () { return settle(arguments, []) }
        e.removeRules = function () { return settle(arguments, undefined) }
      }
      return e
    }
    function fillSpec(path, spec) {
      var obj = ns(path)
      var soft = spec.soft || {}
      var declarative = spec.declarativeEvents || []
      var crashes = spec.crashes || []
      var listed = false
      ;(spec.methods || []).forEach(function (name) {
        if (name in obj && crashes.indexOf(name) < 0) return
        define(obj, name, Object.prototype.hasOwnProperty.call(soft, name) ? function () { return settle(arguments, soft[name]) } : function () { return failWith(arguments, NOT_SUPPORTED(path + '.' + name)) })
        stubbed.push(path + '.' + name)
        listed = true
      })
      ;(spec.events || []).forEach(function (name) {
        if (name in obj && crashes.indexOf(name) < 0) return
        define(obj, name, stubEvent(declarative.indexOf(name) >= 0))
      })
      Object.keys(spec.constants || {}).forEach(function (k) {
        if (!(k in obj)) define(obj, k, JSON.parse(JSON.stringify(spec.constants[k])))
      })
      ;(spec.classes || []).forEach(function (k) {
        if (!(k in obj)) define(obj, k, stubClass(k))
      })
      return listed
    }
    function fillAll(cat, prefix) {
      Object.keys(cat).forEach(function (name) {
        var spec = cat[name]
        var path = prefix ? prefix + '.' + name : name
        try {
          var top = prefix ? prefix.split('.')[0] : name
          var visible = unlocked(Object.assign({ _name: top }, spec)) || (prefix && chrome[top])
          if (visible) fillSpec(path, spec)
          if (spec.children) fillAll(spec.children, path)
        } catch (e) {
          console.warn('[signet] extension APIs: safety net failed for chrome.' + path, e)
        }
      })
    }
    // storage.managed is an empty read-only area (the host answers), nothing to stub there.
    fillAll(catalog, '')

    // `browser === chrome`, frozen like the library did.
    finish()
    return { namespaces: installed, mode: 'first', stubbed: stubbed, fixedEvents: [] }

    function finish() {
      Object.freeze(chrome)
      target.chrome = chrome
      target.browser = chrome
    }
  }

  root.__signetInstallApis = installApis
  if (typeof module !== 'undefined' && module.exports) module.exports = { installApis: installApis }
})(typeof globalThis !== 'undefined' ? globalThis : this)

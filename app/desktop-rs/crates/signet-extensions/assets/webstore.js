// Install bridge for the Chrome Web Store pages (chromewebstore.google.com and chrome.google.com/webstore).
// Role of electron-chrome-web-store's `chrome.webstorePrivate` in the Electron app.
//
// BRIDGE ASSUMPTION: bridge.js ran first; signet-web provides window.__signetPost / window.__signetDispatch and routes the
// "signet-crx" channel of THIS page to ExtensionHost::webstore_call (not to ExtensionHost::call: the caller is a web page).
// Inject only into the main frame of the store origins (crx::is_store_url).
//
// The store page asks `chrome.webstorePrivate.beginInstallWithManifest3(details, cb)`; the host shows the permission dialog
// and answers "" (accepted) or "user_cancelled"; then the page calls `completeInstall(id, cb)` and the host downloads and
// installs the CRX. The method names and result strings follow what the store pages call; check them against the live
// store when updating the Chrome version (the store changes without notice).
(function () {
  'use strict'
  var g = globalThis
  if (!g.__signetCrx) return
  var bridge = g.__signetCrx
  var chromeObj = g.chrome || (g.chrome = {})

  // Like the native API, every method takes a callback and, without one, returns a Promise (the current store pages use
  // `await chrome.webstorePrivate.getExtensionStatus(id)`).
  function reply(cb, value) {
    if (typeof cb === 'function') {
      cb(value)
      return undefined
    }
    return Promise.resolve(value)
  }

  function ask(method, args, cb, fallback) {
    var p = bridge.call('webstorePrivate.' + method, args).then(function (r) {
      return r.ok ? r.value : fallback
    })
    if (typeof cb !== 'function') return p
    p.then(function (value) { cb(value) })
    return undefined
  }

  // A `chrome.events.Event` look-alike: the store listens to install progress and to management events.
  function makeEvent() {
    var listeners = []
    return {
      addListener: function (fn) { if (typeof fn === 'function' && listeners.indexOf(fn) < 0) listeners.push(fn) },
      removeListener: function (fn) { var i = listeners.indexOf(fn); if (i >= 0) listeners.splice(i, 1) },
      hasListener: function (fn) { return listeners.indexOf(fn) >= 0 },
      hasListeners: function () { return listeners.length > 0 },
      dispatch: function () {
        var args = arguments
        listeners.slice().forEach(function (fn) { try { fn.apply(null, args) } catch (e) { console.error(e) } })
      }
    }
  }
  var onInstallStageChanged = makeEvent()
  var onDownloadProgress = makeEvent()
  var mgmtEvents = { onInstalled: makeEvent(), onUninstalled: makeEvent(), onEnabled: makeEvent(), onDisabled: makeEvent() }

  // The error of a failed call: `chrome.runtime.lastError` during the callback, or a rejected Promise without callback.
  function fail(cb, message) {
    if (typeof cb !== 'function') return Promise.reject(new Error(message))
    var rt = chromeObj.runtime
    var had = false
    try {
      if (rt) { Object.defineProperty(rt, 'lastError', { value: { message: message }, configurable: true }); had = true }
    } catch (e) {}
    try { cb() } finally {
      if (had) { try { delete rt.lastError } catch (e) {} }
    }
    return undefined
  }

  // Runs `method` on the host; the result is handed to `onOk(value)`, an error is reported as `fail` does.
  function call(method, args, cb, onOk) {
    var p = bridge.call(method, args).then(function (r) {
      if (!r.ok) throw new Error(r.error || 'Unknown error')
      return onOk ? onOk(r.value) : r.value
    })
    if (typeof cb !== 'function') return p
    p.then(function (value) { cb(value) }, function (e) { fail(cb, e.message) })
    return undefined
  }

  var api = {
    beginInstallWithManifest3: function (details, cb) {
      return ask('beginInstallWithManifest3', [details], cb, 'unknown_error')
    },
    // Resolves once the extension is installed (no value, like Chrome) and then tells the page through the same events
    // Chrome raises, so a store page waiting on them stops showing its loading state.
    completeInstall: function (id, cb) {
      onInstallStageChanged.dispatch('downloading')
      onDownloadProgress.dispatch(id, 0.5)
      onInstallStageChanged.dispatch('installing')
      var p = bridge.call('webstorePrivate.completeInstall', [id]).then(function (r) {
        if (!r.ok) throw new Error(r.error || 'Unknown error')
        onDownloadProgress.dispatch(id, 1)
        return bridge.call('management.get', [id]).then(function (m) {
          if (m.ok) mgmtEvents.onInstalled.dispatch(m.value)
        })
      }).then(function () { return undefined })
      if (typeof cb !== 'function') return p
      p.then(function () { cb() }, function (e) { fail(cb, e.message) })
      return undefined
    },
    onInstallStageChanged: onInstallStageChanged,
    onDownloadProgress: onDownloadProgress,
    install: function (id, silent, cb) {
      return ask('install', [id], cb, 'unknown_error')
    },
    getExtensionStatus: function (id, manifest, cb) {
      if (typeof manifest === 'function') {
        cb = manifest
        manifest = undefined
      }
      return ask('getExtensionStatus', [id], cb, 'installable')
    },
    enableAppLauncher: function (enable, cb) { return reply(cb) },
    getBrowserLogin: function (cb) { return reply(cb, '') },
    getStoreLogin: function (cb) { return reply(cb, '') },
    setStoreLogin: function (login, cb) { return reply(cb) },
    getWebGLStatus: function (cb) { return reply(cb, 'webgl_allowed') },
    getIsLauncherEnabled: function (cb) { return reply(cb, false) },
    isInIncognitoMode: function (cb) { return reply(cb, false) },
    isPendingCustodianApproval: function (id, cb) { return reply(cb, false) },
    getFullChromeVersion: function (cb) { return ask('getFullChromeVersion', [], cb, { version_number: '' }) },
    getMV2DeprecationStatus: function (cb) { return reply(cb, 'none') },
    getReferrerChain: function (cb) { return reply(cb, '') }
  }
  // `chrome.management` as the store pages use it: the real installed set, so the page shows "Remove from Chrome".
  var management = {
    getAll: function (cb) { return call('management.getAll', [], cb) },
    get: function (id, cb) { return call('management.get', [id], cb) },
    getSelf: function (cb) { return fail(cb, 'Failed to find extension.') },
    setEnabled: function (id, enabled, cb) { return call('management.setEnabled', [id, !!enabled], cb) },
    uninstall: function (id, options, cb) {
      if (typeof options === 'function') { cb = options; options = undefined }
      return call('management.uninstall', [id, options || {}], cb, function () {
        mgmtEvents.onUninstalled.dispatch(id)
      })
    },
    getPermissionWarningsById: function (id, cb) { return reply(cb, []) },
    onInstalled: mgmtEvents.onInstalled,
    onUninstalled: mgmtEvents.onUninstalled,
    onEnabled: mgmtEvents.onEnabled,
    onDisabled: mgmtEvents.onDisabled
  }

  // Other methods of either Chrome namespace the store may call: answered by the host (logged there under
  // SIGNET_DEBUG_STORE) with no value, so the page does not wait forever. Anything else stays undefined (feature tests).
  function wrap(name, target, extra) {
    return new Proxy(target, {
      get: function (t, prop) {
        if (prop in t || typeof prop !== 'string') return t[prop]
        if (/^on[A-Z]/.test(prop)) return (t[prop] = makeEvent())
        if (extra.indexOf(prop) < 0) return undefined
        return function () {
          var args = Array.prototype.slice.call(arguments)
          var cb = typeof args[args.length - 1] === 'function' ? args.pop() : undefined
          return call(name + '.' + prop, args, cb)
        }
      }
    })
  }
  var storeApi = wrap('webstorePrivate', api, ['getEphemeralAppsEnabled', 'launchEphemeralApp', 'requestExtension', 'getInstallationStatus', 'showPermissionPrompt', 'beginInstallWithManifest2', 'beginInstallWithManifest', 'getBrowserLogin', 'getMV2DeprecationStatus'])
  var mgmtApi = wrap('management', management, ['launchApp', 'createAppShortcut', 'setLaunchType', 'generateAppForLink', 'installReplacementWebApp', 'getPermissionWarningsByManifest', 'canInstallReplacementAndroidApp', 'canSetLaunchType'])

  // Chromium (Chrome bootstrap) installs its own `chrome.webstorePrivate` (plus `chrome.runtime` and `chrome.management`)
  // on the store origin AFTER this script ran. It opens Chromium's install dialog and installs into Chromium's internal
  // extension system, which Signet does not use: ours must win. Defining ours as non-configurable breaks Chromium's
  // binding setup (the store page then disables its button), so ours is an accessor that ignores Chromium's assignment
  // and is asserted again whenever Chromium may have replaced it.
  function assertOne(name, getter) {
    var d = Object.getOwnPropertyDescriptor(chromeObj, name)
    if (d && d.get === getter) return
    try {
      Object.defineProperty(chromeObj, name, { get: getter, set: function () {}, enumerable: true, configurable: true })
    } catch (e) {}
  }
  function getStore() { return storeApi }
  function getMgmt() { return mgmtApi }
  function assert() {
    assertOne('webstorePrivate', getStore)
    assertOne('management', getMgmt)
  }
  assert()
  var ticks = 0
  var timer = setInterval(function () {
    assert()
    if (++ticks > 300) clearInterval(timer)
  }, 10)
  ;['DOMContentLoaded', 'load', 'readystatechange'].forEach(function (ev) { document.addEventListener(ev, assert, true) })
  g.addEventListener('load', assert, true)
})()

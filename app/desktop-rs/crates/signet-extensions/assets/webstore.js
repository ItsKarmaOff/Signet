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

  var api = {
    beginInstallWithManifest3: function (details, cb) {
      return ask('beginInstallWithManifest3', [details], cb, 'unknown_error')
    },
    completeInstall: function (id, cb) {
      return ask('completeInstall', [id], typeof cb === 'function' ? function () { cb() } : undefined, undefined)
    },
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
  // Chromium (Chrome bootstrap) installs its own `chrome.webstorePrivate` (plus `chrome.runtime` and `chrome.management`)
  // on the store origin AFTER this script ran. It opens Chromium's install dialog and installs into Chromium's internal
  // extension system, which Signet does not use: ours must win. Defining ours as non-configurable breaks Chromium's
  // binding setup (the store page then disables its button), so ours is an accessor that ignores Chromium's assignment
  // and is asserted again whenever Chromium may have replaced it.
  function assert() {
    var d = Object.getOwnPropertyDescriptor(chromeObj, 'webstorePrivate')
    if (d && d.get === getter) return
    try {
      Object.defineProperty(chromeObj, 'webstorePrivate', { get: getter, set: function () {}, enumerable: true, configurable: true })
    } catch (e) {}
  }
  function getter() { return api }
  assert()
  var ticks = 0
  var timer = setInterval(function () {
    assert()
    if (++ticks > 300) clearInterval(timer)
  }, 10)
  ;['DOMContentLoaded', 'load', 'readystatechange'].forEach(function (ev) { document.addEventListener(ev, assert, true) })
  g.addEventListener('load', assert, true)
})()

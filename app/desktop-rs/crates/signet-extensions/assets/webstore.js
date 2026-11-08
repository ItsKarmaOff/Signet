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
  if (!g.__signetCrx || (g.chrome && g.chrome.webstorePrivate)) return
  var bridge = g.__signetCrx
  var chromeObj = g.chrome || (g.chrome = {})

  function ask(method, args, cb, fallback) {
    bridge.call('webstorePrivate.' + method, args).then(function (r) {
      var value = r.ok ? r.value : fallback
      if (typeof cb === 'function') cb(value)
    })
  }

  var api = {
    beginInstallWithManifest3: function (details, cb) {
      ask('beginInstallWithManifest3', [details], cb, 'unknown_error')
    },
    completeInstall: function (id, cb) {
      ask('completeInstall', [id], function () { if (typeof cb === 'function') cb() }, undefined)
    },
    install: function (id, silent, cb) {
      ask('install', [id], cb, 'unknown_error')
    },
    getExtensionStatus: function (id, manifest, cb) {
      if (typeof manifest === 'function') {
        cb = manifest
        manifest = undefined
      }
      ask('getExtensionStatus', [id], cb, 'installable')
    },
    enableAppLauncher: function (enable, cb) { if (typeof cb === 'function') cb() },
    getBrowserLogin: function (cb) { cb('') },
    getStoreLogin: function (cb) { cb('') },
    setStoreLogin: function (login, cb) { if (typeof cb === 'function') cb() },
    getWebGLStatus: function (cb) { cb('webgl_allowed') },
    getIsLauncherEnabled: function (cb) { cb(false) },
    isInIncognitoMode: function (cb) { cb(false) },
    isPendingCustodianApproval: function (id, cb) { cb(false) },
    getFullChromeVersion: function (cb) { ask('getFullChromeVersion', [], cb, { version_number: '' }) },
    getMV2DeprecationStatus: function (cb) { cb('none') },
    getReferrerChain: function (cb) { cb('') }
  }
  Object.defineProperty(chromeObj, 'webstorePrivate', { value: api, enumerable: true, configurable: true })
})()

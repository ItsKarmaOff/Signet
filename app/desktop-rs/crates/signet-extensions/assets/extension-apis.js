// Entry point injected into every extension context (extension pages, the hidden background page, content scripts).
// Port of app/desktop/src/preload/extension-apis.ts for the Signet bridge.
//
// BRIDGE ASSUMPTION: bridge.js ran first (globalThis.__signetCrx), and signet-web provides
//   window.__signetPost(channel, json)  and  window.__signetDispatch(channel, json)  (see bridge.js).
// The host (ExtensionHost::page_script) prepends `globalThis.__signetExtensionConfig = {...}` with the extension id, its
// manifest, the preloaded i18n messages, the list of implemented methods, the API catalog, the install plan and the context.
//
// It acts only in a `chrome-extension://` document or worker (a web page never gets it) and, for a page, only when it is not
// sandboxed (a sandboxed extension page has an opaque origin and no extension APIs, as in Chrome).
(function () {
  'use strict'
  var g = globalThis
  var config = g.__signetExtensionConfig
  try {
    delete g.__signetExtensionConfig
  } catch (e) {
    // not deletable: harmless
  }
  if (!config || !g.__signetCrx || !g.__signetInstallApis) return

  var kind = (config.context && config.context.kind) || 'page'
  var isWorker = kind === 'worker'

  function contextUrl() {
    try {
      return String(g.location && g.location.href)
    } catch (e) {
      return null
    }
  }
  function isSandboxed() {
    try {
      return !isWorker && g.location.origin === 'null'
    } catch (e) {
      return true
    }
  }
  // Same rule as shared::is_extension_context in Rust.
  function isExtensionContext(url, sandboxed) {
    // `signet-extension://` is the scheme extension pages are really navigated under (see signet-web extension.rs).
    if (!url || (url.indexOf('chrome-extension://') !== 0 && url.indexOf('signet-extension://') !== 0)) return false
    return isWorker || !sandboxed
  }

  g.__signetCrx.context = config.context || null
  var report
  if (kind === 'content') {
    // Content scripts live in a web page: nothing is installed on the page's own `chrome`; the scripts get a private object.
    var holder = {}
    report = g.__signetInstallApis(config, g.__signetCrx, holder)
    var table = (g.__signetContent = g.__signetContent || {})
    Object.defineProperty(table, config.id, { value: holder.chrome, enumerable: false, configurable: true })
    return
  }
  if (!isExtensionContext(contextUrl(), isSandboxed())) return
  try {
    report = g.__signetInstallApis(config, g.__signetCrx, g)
  } catch (e) {
    console.error('[signet] extension APIs: injection failed', e)
    report = { namespaces: [], mode: 'failed', error: String(e && e.message ? e.message : e) }
  }
  report.context = isWorker ? 'worker' : 'page'
  g.__signetCrx.call('__installed', [report])
})()

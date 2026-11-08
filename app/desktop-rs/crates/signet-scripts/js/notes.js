// Port of preload/notes.ts. Bridge for the Note and Easel pages. Main frame only, document start.
// Active only on a document page (`signet://note/<id>`, `signet://easel/<id>`); the browser side rechecks the
// frame (main frame, address) and takes the document id from the address, never from here.
// Defines `window.signet` (locale, appearance) and `window.signetNotes` (load, save, remove), as the Electron preload did
// with contextBridge. They are defined on the page's own window, so they are only created for the app's pages.
(function notes(bridge, config, g) {
  'use strict'
  g = g || globalThis
  const loc = g.location
  const pages = (config && config.notesPages) || []
  // The document address itself (`signet://note/<id>`, served by signet-pages) ...
  const doc = /^signet:\/\/(note|easel)\/[0-9a-f-]{36}\/?$/i.test(loc.href)
  // ... or, if configured, an app page on a document route (`<page>#note/<id>`, the Electron layout).
  const route = /^#(note|easel)\/[0-9a-f-]{36}$/i.test(loc.hash)
  const app = pages.some((p) => loc.protocol === p.protocol && loc.host === p.host && loc.pathname === p.pathname)
  if (g.top !== g || !(doc || (route && app))) return

  const subscribe = (channel) => (cb) => bridge.on(channel, (value) => cb(value))
  const signet = {
    getLocale: () => bridge.invoke('notes:locale', null),
    onLocale: subscribe('locale'),
    getAppearance: () => bridge.invoke('notes:appearance', null),
    onAppearance: subscribe('appearance')
  }
  const signetNotes = {
    load: () => bridge.invoke('notes:load', null),
    save: (patch) => bridge.invoke('notes:save', patch),
    remove: () => bridge.invoke('notes:remove', null)
  }
  const expose = (key, api) =>
    Object.defineProperty(g, key, { value: Object.freeze(api), enumerable: true, configurable: false, writable: false })
  expose('signet', signet)
  expose('signetNotes', signetNotes)
})

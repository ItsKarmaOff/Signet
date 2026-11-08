// Bridge between extension JS (pages, background, content scripts) and the browser process of Signet.
//
// BRIDGE ASSUMPTION (provided by signet-web, see README "What the web layer must do"):
//   window.__signetPost(channel, jsonString)       page -> browser process, fire and forget.
//   window.__signetCrxDispatch(channel, jsonString)   browser process -> page: the app calls it in the page.
// This file adds a tiny promise based request/response helper on top and a subscription for pushed events.
//
// Channels used here:
//   "signet-crx"        page -> app   { id, method, args }                        a call
//   "signet-crx-reply"  app -> page   { id, result: {ok, value?} | {ok:false, error} }
//   "signet-crx-event"  app -> page   { name, args }                              an event for every context of the extension
//   "signet-crx-ctx"    app -> page   { name, args }                              an event for this context only
//
// Result: globalThis.__signetCrx = { call(method, args) -> Promise<{ok, value?|error}>, subscribe(cb), context }.
// `cb(name, args)` gets both kinds of events. `context` is the id the app gave this context (config).
(function () {
  'use strict'
  var root = globalThis
  if (root.__signetCrx) return

  var pending = new Map()
  var listeners = []
  var counter = 0

  function post(payload) {
    if (typeof root.__signetPost !== 'function') throw new Error('Signet bridge is missing (window.__signetPost)')
    root.__signetPost('signet-crx', JSON.stringify(payload))
  }

  // Own entry point: `__signetDispatch` belongs to the page script bridge (locked), which ignores these channels.
  root.__signetCrxDispatch = function (channel, json) {
    if (channel === 'signet-crx-reply') {
      var msg = JSON.parse(json)
      var entry = pending.get(msg.id)
      if (entry) {
        pending.delete(msg.id)
        entry(msg.result)
      }
      return
    }
    if (channel === 'signet-crx-event' || channel === 'signet-crx-ctx') {
      var ev = JSON.parse(json)
      for (var i = 0; i < listeners.length; i++) {
        try {
          listeners[i](ev.name, ev.args || [])
        } catch (e) {
          console.error('[signet] extension event handler failed', e)
        }
      }
      return
    }
  }

  root.__signetCrx = {
    context: null,
    call: function (method, args) {
      return new Promise(function (resolve) {
        var id = 'c' + ++counter
        pending.set(id, resolve)
        try {
          post({ id: id, method: method, args: args || [] })
        } catch (e) {
          pending.delete(id)
          resolve({ ok: false, error: String(e && e.message ? e.message : e) })
        }
      })
    },
    subscribe: function (cb) {
      listeners.push(cb)
    }
  }
})()

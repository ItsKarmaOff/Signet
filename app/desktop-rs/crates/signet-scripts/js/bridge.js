// Shared page script runtime: request/response and events on top of the two hidden functions of the web layer.
//
//   page to browser: g[names.post](channel, jsonString)       (provided by signet-web, removed from `g` here)
//   browser to page: g[names.dispatch](channel, jsonString)   (defined here, called by signet-web / signet-app)
//
// Page to browser payload: JSON of `{ "rid"?: string, "data": any }`. `rid` is present when a reply is expected.
// Browser to page: channel "reply" with `{ "rid": string, "ok": true, "data": any }` or `{ "rid", "ok": false, "error": string }`,
// any other channel is an event whose payload is the JSON of its data.
//
// This file is one function expression: `bridge.js` is evaluated once per frame, before the page's own scripts, and
// everything it needs from the page's world is captured here, so later changes to the built-ins do not affect it.
(function createBridge(names, g) {
  'use strict'

  const stringify = g.JSON.stringify.bind(g.JSON)
  const parse = g.JSON.parse.bind(g.JSON)
  const setTimer = g.setTimeout.bind(g)
  const clearTimer = g.clearTimeout.bind(g)
  const PromiseCtor = g.Promise
  const MapCtor = g.Map
  const defineProperty = Object.defineProperty
  const getRandomValues = g.crypto && g.crypto.getRandomValues ? g.crypto.getRandomValues.bind(g.crypto) : null
  const rawPost = typeof g[names.post] === 'function' ? g[names.post] : null
  const DEFAULT_TIMEOUT = 30000

  // The post function is only reachable through this closure from now on.
  try {
    delete g[names.post]
  } catch {
    // not configurable: nothing more can be done
  }
  if (g[names.post] === rawPost && rawPost) {
    try {
      defineProperty(g, names.post, { value: undefined, configurable: false, writable: false, enumerable: false })
    } catch {
      // keep it
    }
  }

  let counter = 0
  function newId() {
    if (getRandomValues) {
      const a = getRandomValues(new Uint32Array(4))
      return a[0].toString(36) + a[1].toString(36) + a[2].toString(36) + a[3].toString(36)
    }
    counter += 1
    return counter.toString(36) + g.Math.random().toString(36).slice(2)
  }

  const pending = new MapCtor() // rid -> { cb, timer }
  const listeners = new MapCtor() // channel -> Set of callbacks

  function post(channel, envelope) {
    if (!rawPost) throw new Error('bridge unavailable')
    rawPost(channel, stringify(envelope))
  }

  /** Fire and forget. Never throws. */
  function send(channel, data) {
    try {
      post(channel, { data: data === undefined ? null : data })
    } catch {
      // no browser side: nothing to tell
    }
  }

  /** Asks the browser; `cb(error, value)` is called once (error is a string or null). Returns a function that forgets the request. */
  function request(channel, data, cb, timeoutMs) {
    const rid = newId()
    const done = (error, value) => {
      const entry = pending.get(rid)
      if (!entry) return
      pending.delete(rid)
      clearTimer(entry.timer)
      try {
        entry.cb(error, value)
      } catch {
        // a faulty callback must not break the bridge
      }
    }
    const timer = setTimer(() => done('timeout', undefined), timeoutMs > 0 ? timeoutMs : DEFAULT_TIMEOUT)
    pending.set(rid, { cb, timer })
    try {
      post(channel, { rid, data: data === undefined ? null : data })
    } catch (e) {
      done(String(e && e.message ? e.message : e), undefined)
    }
    return () => {
      const entry = pending.get(rid)
      if (entry) {
        pending.delete(rid)
        clearTimer(entry.timer)
      }
    }
  }

  /** Promise flavour of `request`, for results that go to the page anyway (notes, passkeys). */
  function invoke(channel, data, timeoutMs) {
    return new PromiseCtor((resolve, reject) => {
      request(channel, data, (error, value) => (error === null ? resolve(value) : reject(new Error(error))), timeoutMs)
    })
  }

  /** Subscribes to the events of a channel; returns the unsubscribe function. */
  function on(channel, cb) {
    let set = listeners.get(channel)
    if (!set) {
      set = new g.Set()
      listeners.set(channel, set)
    }
    set.add(cb)
    return () => {
      set.delete(cb)
    }
  }

  /** Entry point of browser to page messages. Unknown or malformed messages are dropped. */
  function dispatch(channel, json) {
    if (typeof channel !== 'string' || typeof json !== 'string') return
    let value
    try {
      value = parse(json)
    } catch {
      return
    }
    if (channel === 'reply') {
      if (!value || typeof value !== 'object' || typeof value.rid !== 'string') return
      const entry = pending.get(value.rid)
      if (!entry) return
      pending.delete(value.rid)
      clearTimer(entry.timer)
      try {
        if (value.ok === true) entry.cb(null, value.data)
        else entry.cb(typeof value.error === 'string' ? value.error : 'error', undefined)
      } catch {
        // see above
      }
      return
    }
    const set = listeners.get(channel)
    if (!set) return
    for (const cb of [...set]) {
      try {
        cb(value)
      } catch {
        // see above
      }
    }
  }

  // The dispatch function must be reachable by name, so it is non-writable and non-configurable; it looks native.
  const entry = dispatch.bind(null)
  defineProperty(entry, 'name', { value: names.dispatch })
  try {
    defineProperty(g, names.dispatch, { value: entry, writable: false, configurable: false, enumerable: false })
  } catch {
    // already defined by an earlier bridge in this frame: keep it
  }

  return Object.freeze({ available: rawPost !== null, send, request, invoke, on })
})

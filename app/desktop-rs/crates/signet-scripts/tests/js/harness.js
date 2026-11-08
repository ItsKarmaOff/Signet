// Loads the page scripts (function expressions) the way the Rust bundle does, for Node tests.
'use strict'
const fs = require('node:fs')
const path = require('node:path')
const vm = require('node:vm')

const JS_DIR = path.join(__dirname, '..', '..', 'js')
const NAMES = { post: '__signetPost', dispatch: '__signetDispatch' }

/** A script file evaluated to its function expression. */
function load(name) {
  const file = path.join(JS_DIR, name)
  return vm.runInThisContext(fs.readFileSync(file, 'utf8'), { filename: file })
}

/**
 * A fake window with the bridge installed on it. `posted` collects `[channel, parsedEnvelope]`;
 * `reply(rid, data)` / `fail(rid, error)` / `emit(channel, value)` play the browser.
 */
function makeBridge(extra = {}) {
  const timers = []
  const g = {
    JSON,
    Map,
    Set,
    Promise,
    Math,
    Uint32Array,
    crypto: globalThis.crypto,
    setTimeout: (fn, ms) => {
      timers.push({ fn, ms })
      return timers.length
    },
    clearTimeout: (id) => {
      if (timers[id - 1]) timers[id - 1] = null
    },
    ...extra
  }
  const posted = []
  g[NAMES.post] = (channel, json) => posted.push([channel, JSON.parse(json)])
  const bridge = load('bridge.js')(NAMES, g)
  const dispatch = g[NAMES.dispatch]
  return {
    g,
    bridge,
    posted,
    timers,
    reply: (rid, data) => dispatch('reply', JSON.stringify({ rid, ok: true, data })),
    fail: (rid, error) => dispatch('reply', JSON.stringify({ rid, ok: false, error })),
    emit: (channel, value) => dispatch(channel, JSON.stringify(value)),
    fireTimers: () => timers.forEach((t) => t && t.fn())
  }
}

module.exports = { load, makeBridge, NAMES, JS_DIR }

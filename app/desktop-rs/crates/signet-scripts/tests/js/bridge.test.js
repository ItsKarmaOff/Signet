'use strict'
const test = require('node:test')
const assert = require('node:assert/strict')
const { makeBridge, NAMES, load } = require('./harness')

test('send posts an envelope without rid', () => {
  const t = makeBridge()
  t.bridge.send('page:submit')
  t.bridge.send('swipe:navigate', 'back')
  assert.deepEqual(t.posted, [
    ['page:submit', { data: null }],
    ['swipe:navigate', { data: 'back' }]
  ])
})

test('the post function is removed from the window and the dispatch one is hidden', () => {
  const t = makeBridge()
  assert.equal(t.g[NAMES.post], undefined)
  const d = Object.getOwnPropertyDescriptor(t.g, NAMES.dispatch)
  assert.equal(typeof d.value, 'function')
  assert.equal(d.enumerable, false)
  assert.equal(d.writable, false)
  assert.equal(d.configurable, false)
  assert.equal(t.bridge.available, true)
  assert.ok(Object.isFrozen(t.bridge))
})

test('request resolves from a reply with the same rid', () => {
  const t = makeBridge()
  const got = []
  t.bridge.request('pw:accounts', null, (e, v) => got.push([e, v]))
  const [channel, env] = t.posted[0]
  assert.equal(channel, 'pw:accounts')
  assert.equal(typeof env.rid, 'string')
  assert.ok(env.rid.length >= 8)
  t.reply('other', 1)
  assert.deepEqual(got, [])
  t.reply(env.rid, [{ id: 'a' }])
  assert.deepEqual(got, [[null, [{ id: 'a' }]]])
  // A second reply with the same id is ignored.
  t.reply(env.rid, 2)
  assert.equal(got.length, 1)
})

test('request ids are unique', () => {
  const t = makeBridge()
  for (let i = 0; i < 50; i++) t.bridge.request('swipe:can', null, () => {})
  assert.equal(new Set(t.posted.map((p) => p[1].rid)).size, 50)
})

test('error replies and timeouts reach the callback once', () => {
  const t = makeBridge()
  const got = []
  t.bridge.request('a', null, (e, v) => got.push([e, v]))
  t.bridge.request('b', null, (e, v) => got.push([e, v]))
  t.fail(t.posted[0][1].rid, 'refused')
  assert.deepEqual(got, [['refused', undefined]])
  t.fireTimers()
  assert.deepEqual(got, [['refused', undefined], ['timeout', undefined]])
  t.fireTimers()
  assert.equal(got.length, 2)
})

test('request uses the given timeout, with a default', () => {
  const t = makeBridge()
  t.bridge.request('a', null, () => {}, 1234)
  t.bridge.request('b', null, () => {})
  assert.deepEqual(t.timers.map((x) => x.ms), [1234, 30000])
})

test('the returned function forgets the request', () => {
  const t = makeBridge()
  const got = []
  const cancel = t.bridge.request('a', null, (e, v) => got.push([e, v]))
  cancel()
  t.reply(t.posted[0][1].rid, 1)
  t.fireTimers()
  assert.deepEqual(got, [])
})

test('invoke returns a promise', async () => {
  const t = makeBridge()
  const ok = t.bridge.invoke('notes:load', null)
  t.reply(t.posted[0][1].rid, { id: 1 })
  assert.deepEqual(await ok, { id: 1 })
  const bad = t.bridge.invoke('notes:load', null)
  t.fail(t.posted[1][1].rid, 'Call refused')
  await assert.rejects(bad, /Call refused/)
})

test('events go to subscribers until they unsubscribe', () => {
  const t = makeBridge()
  const seen = []
  const off = t.bridge.on('locale', (v) => seen.push(v))
  t.bridge.on('locale', () => {
    throw new Error('a faulty listener must not break the others')
  })
  t.emit('locale', 'fr')
  t.emit('appearance', { dark: true })
  off()
  t.emit('locale', 'en')
  assert.deepEqual(seen, ['fr'])
})

test('malformed dispatches are dropped', () => {
  const t = makeBridge()
  const d = t.g[NAMES.dispatch]
  const got = []
  t.bridge.request('a', null, (e, v) => got.push([e, v]))
  d('reply', 'not json')
  d('reply', 'null')
  d('reply', '{"rid":5}')
  d(5, '{}')
  d('reply', 5)
  assert.deepEqual(got, [])
})

test('the page cannot reach the post function through the window afterwards', () => {
  const t = makeBridge()
  assert.equal(Object.keys(t.g).includes(NAMES.post), false)
})

test('without a post function the bridge is unavailable and nothing throws', () => {
  const g = { JSON, Map, Set, Promise, Math, Uint32Array, crypto: globalThis.crypto, setTimeout, clearTimeout }
  const bridge = load('bridge.js')(NAMES, g)
  assert.equal(bridge.available, false)
  bridge.send('x', 1)
  const got = []
  bridge.request('x', null, (e) => got.push(e))
  assert.equal(got.length, 1)
  assert.match(got[0], /unavailable/)
})

test('names come from the arguments', () => {
  const g = { JSON, Map, Set, Promise, Math, Uint32Array, crypto: globalThis.crypto, setTimeout, clearTimeout, p: () => {} }
  load('bridge.js')({ post: 'p', dispatch: 'd' }, g)
  assert.equal(typeof g.d, 'function')
  assert.equal(g.p, undefined)
})

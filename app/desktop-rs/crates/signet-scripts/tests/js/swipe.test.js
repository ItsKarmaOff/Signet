'use strict'
const test = require('node:test')
const assert = require('node:assert/strict')
const { load, makeBridge } = require('./harness')
const { makeWindow, Element } = require('./fakedom')

const swipe = load('swipe.js')

function setup(opts) {
  const w = makeWindow(opts)
  const b = makeBridge()
  swipe(b.bridge, { swipeDiag: false }, w.g)
  const wheel = (deltaX, deltaY = 0, extra = {}) => {
    w.clock.now += 16
    w.fire('wheel', { isTrusted: true, ctrlKey: false, defaultPrevented: false, deltaX, deltaY, composedPath: () => [], target: w.doc.body, ...extra })
  }
  const requests = () => b.posted.filter((p) => p[0] === 'swipe:can')
  const navigations = () => b.posted.filter((p) => p[0] === 'swipe:navigate').map((p) => p[1].data)
  const answer = (state) => b.reply(requests().at(-1)[1].rid, state)
  return { w, b, wheel, requests, navigations, answer }
}

test('does nothing in a subframe', () => {
  const t = setup({ main: false })
  assert.equal(t.w.listeners.wheel, undefined)
  assert.equal(t.w.listeners.submit, undefined)
})

test('a trusted submit tells the browser, a simulated one does not', () => {
  const t = setup()
  t.w.fire('submit', { isTrusted: false })
  assert.deepEqual(t.b.posted, [])
  t.w.fire('submit', { isTrusted: true })
  assert.deepEqual(t.b.posted, [['page:submit', { data: null }]])
})

test('a vertical scroll never asks the browser', () => {
  const t = setup()
  for (let i = 0; i < 10; i++) t.wheel(1, 40)
  assert.equal(t.requests().length, 0)
})

test('untrusted and ctrl wheel events are ignored', () => {
  const t = setup()
  for (let i = 0; i < 6; i++) t.wheel(-30, 0, { isTrusted: false })
  for (let i = 0; i < 6; i++) t.wheel(-30, 0, { ctrlKey: true })
  assert.equal(t.requests().length, 0)
})

test('a strong swipe to the right navigates back without waiting for the release', () => {
  const t = setup()
  for (let i = 0; i < 3; i++) t.wheel(-30)
  assert.equal(t.requests().length, 1)
  t.answer({ enabled: true, back: true, forward: false })
  for (let i = 0; i < 14; i++) t.wheel(-30)
  assert.deepEqual(t.navigations(), ['back'])
  // The rest of the gesture is ignored: still a single navigation.
  for (let i = 0; i < 5; i++) t.wheel(-30)
  assert.deepEqual(t.navigations(), ['back'])
})

test('a swipe to the left goes forward', () => {
  const t = setup()
  for (let i = 0; i < 3; i++) t.wheel(30)
  t.answer({ enabled: true, back: false, forward: true })
  for (let i = 0; i < 14; i++) t.wheel(30)
  assert.deepEqual(t.navigations(), ['forward'])
})

test('releasing past the threshold navigates after the silence', () => {
  const t = setup()
  for (let i = 0; i < 3; i++) t.wheel(-25)
  t.answer({ enabled: true, back: true, forward: false })
  for (let i = 0; i < 4; i++) t.wheel(-25)
  t.w.flushFrames()
  assert.deepEqual(t.navigations(), [])
  t.w.advance(400)
  assert.deepEqual(t.navigations(), ['back'])
})

test('releasing before the threshold does nothing', () => {
  const t = setup()
  for (let i = 0; i < 3; i++) t.wheel(-10)
  t.answer({ enabled: true, back: true, forward: false })
  t.wheel(-10)
  t.w.flushFrames()
  t.w.advance(400)
  assert.deepEqual(t.navigations(), [])
})

test('no history in that direction: the bubble resists and nothing happens', () => {
  const t = setup()
  for (let i = 0; i < 3; i++) t.wheel(-30)
  t.answer({ enabled: true, back: false, forward: true })
  for (let i = 0; i < 14; i++) t.wheel(-30)
  t.w.flushFrames()
  t.w.advance(400)
  assert.deepEqual(t.navigations(), [])
})

test('a refusal of the browser lets the gesture through', () => {
  const t = setup()
  for (let i = 0; i < 3; i++) t.wheel(-30)
  t.answer({ enabled: false, back: true, forward: true })
  for (let i = 0; i < 14; i++) t.wheel(-30)
  t.w.advance(400)
  assert.deepEqual(t.navigations(), [])
  assert.equal(t.requests().length, 1)
})

test('a timeout costs one gesture, a bridge error disables the document', () => {
  const t = setup()
  for (let i = 0; i < 3; i++) t.wheel(-30)
  t.b.fireTimers()
  t.w.advance(400)
  // Next gesture asks again.
  for (let i = 0; i < 3; i++) t.wheel(-30)
  assert.equal(t.requests().length, 2)
  t.b.fail(t.requests().at(-1)[1].rid, 'bridge unavailable')
  t.w.advance(400)
  for (let i = 0; i < 6; i++) t.wheel(-30)
  assert.equal(t.requests().length, 2)
})

test('a page that can still scroll sideways keeps the gesture', () => {
  const t = setup()
  const scroller = new Element('div')
  scroller.scrollWidth = 2000
  scroller.clientWidth = 500
  scroller.scrollLeft = 300
  t.w.g.getComputedStyle = () => ({ overflowX: 'auto', visibility: 'visible' })
  for (let i = 0; i < 6; i++) t.wheel(-30, 0, { composedPath: () => [scroller] })
  assert.equal(t.requests().length, 0)
})

test('a page that canceled the event keeps the gesture', () => {
  const t = setup()
  for (let i = 0; i < 6; i++) t.wheel(-30, 0, { defaultPrevented: true })
  assert.equal(t.requests().length, 0)
})

test('the bubble lives in a closed shadow root and is removed at the end', () => {
  const t = setup()
  for (let i = 0; i < 3; i++) t.wheel(-30)
  t.answer({ enabled: true, back: true, forward: false })
  t.wheel(-30)
  t.w.flushFrames()
  const host = t.w.doc.documentElement.children.at(-1)
  assert.ok(host.shadow, 'shadow root attached')
  for (let i = 0; i < 12; i++) t.wheel(-30)
  t.w.advance(1000)
  assert.equal(host.removed, true)
})

test('bad replies are treated as a refusal', () => {
  const t = setup()
  for (let i = 0; i < 3; i++) t.wheel(-30)
  t.answer('yes')
  for (let i = 0; i < 14; i++) t.wheel(-30)
  assert.deepEqual(t.navigations(), [])
})

'use strict'
// gpc.js, identity.js, notes.js and the load guards of passwords.js and autofill.js.
const test = require('node:test')
const assert = require('node:assert/strict')
const { load, makeBridge } = require('./harness')
const { makeWindow } = require('./fakedom')

test('gpc defines navigator.globalPrivacyControl on the prototype', () => {
  class Navigator {}
  const g = { Navigator }
  load('gpc.js')(null, {}, g)
  const d = Object.getOwnPropertyDescriptor(Navigator.prototype, 'globalPrivacyControl')
  assert.equal(typeof d.get, 'function')
  assert.equal(d.enumerable, true)
  assert.equal(d.configurable, true)
  assert.equal(new Navigator().globalPrivacyControl, true)
  assert.equal(Object.getOwnPropertyDescriptor(new Navigator(), 'globalPrivacyControl'), undefined)
})

function identityWindow(protocol = 'https:') {
  const g = {
    location: { protocol },
    performance: {
      timeOrigin: 1700000000000,
      now: () => 5,
      getEntriesByType: () => [{ nextHopProtocol: 'h2', requestStart: 10, responseStart: 20, domContentLoadedEventEnd: 30, loadEventEnd: 40 }],
      getEntriesByName: () => [{ startTime: 15 }]
    },
    setTimeout: (fn) => fn()
  }
  return g
}

test('identity fills window.chrome like Chrome', () => {
  const g = identityWindow()
  load('identity.js')(null, {}, g)
  const c = g.chrome
  const lt = c.loadTimes()
  assert.equal(lt.connectionInfo, 'h2')
  assert.equal(lt.wasFetchedViaSpdy, true)
  assert.equal(lt.startLoadTime, 1700000000)
  assert.equal(lt.requestTime, (1700000000000 + 10) / 1000)
  assert.equal(lt.firstPaintTime, (1700000000000 + 15) / 1000)
  assert.equal(c.csi().tran, 15)
  assert.equal(c.app.isInstalled, false)
  assert.equal(c.app.getIsInstalled(), false)
  assert.equal(c.app.runningState(), 'cannot_run')
  let state
  c.app.installState((s) => (state = s))
  assert.equal(state, 'not_installed')
  assert.equal(c.app.InstallState.NOT_INSTALLED, 'not_installed')
})

test('identity functions look native and keep their names', () => {
  const g = identityWindow()
  load('identity.js')(null, {}, g)
  assert.equal(g.chrome.loadTimes.name, 'loadTimes')
  assert.equal(g.chrome.csi.name, 'csi')
  assert.match(Function.prototype.toString.call(g.chrome.loadTimes), /\[native code\]/)
})

test('identity keeps what chrome already has and leaves extension pages alone', () => {
  const g = identityWindow()
  const own = () => 'mine'
  g.chrome = { loadTimes: own, runtime: { id: 'x' } }
  load('identity.js')(null, {}, g)
  assert.equal(g.chrome.loadTimes, own)
  assert.equal(g.chrome.runtime.id, 'x')
  assert.ok(g.chrome.csi && g.chrome.app)
  const ext = identityWindow('chrome-extension:')
  load('identity.js')(null, {}, ext)
  assert.equal(ext.chrome, undefined)
})

const ID = '0b9d0f3e-1c1a-4a53-9a6f-2f6d2b1c9e11'
const config = { notesPages: [{ protocol: 'signet:', host: 'app', pathname: '/index.html' }] }

function notesWindow(href, main = true) {
  const w = makeWindow({ href, main })
  return w.g
}

test('notes exposes its two objects on a document route of an app page', async () => {
  const g = notesWindow(`signet://app/index.html#note/${ID}`)
  const t = makeBridge()
  load('notes.js')(t.bridge, config, g)
  assert.deepEqual(Object.keys(g.signet).sort(), ['getAppearance', 'getLocale', 'onAppearance', 'onLocale'])
  assert.deepEqual(Object.keys(g.signetNotes).sort(), ['load', 'remove', 'save'])
  assert.ok(Object.isFrozen(g.signetNotes))
  const p = g.signetNotes.save({ title: 't', html: '<p>x</p>' })
  assert.deepEqual(t.posted[0], ['notes:save', { rid: t.posted[0][1].rid, data: { title: 't', html: '<p>x</p>' } }])
  t.reply(t.posted[0][1].rid, true)
  assert.equal(await p, true)
  const l = g.signet.getLocale()
  assert.equal(t.posted[1][0], 'notes:locale')
  t.reply(t.posted[1][1].rid, 'fr')
  assert.equal(await l, 'fr')
  const seen = []
  const off = g.signet.onLocale((v) => seen.push(v))
  t.emit('locale', 'en')
  off()
  t.emit('locale', 'de')
  assert.deepEqual(seen, ['en'])
})

test('notes is active on the document address itself, with no configured app page', () => {
  for (const kind of ['note', 'easel']) {
    const g = notesWindow(`signet://${kind}/${ID}`)
    load('notes.js')(makeBridge().bridge, {}, g)
    assert.deepEqual(Object.keys(g.signetNotes).sort(), ['load', 'remove', 'save'], kind)
  }
  for (const [href, main] of [
    [`signet://note/${ID}`, false],
    [`signet://note/${ID}x`, true],
    ['signet://note/not-an-id', true],
    [`signet://folder/${ID}`, true],
    [`https://note/${ID}`, true]
  ]) {
    const g = notesWindow(href, main)
    load('notes.js')(makeBridge().bridge, {}, g)
    assert.equal(g.signetNotes, undefined, href)
  }
})

test('notes does nothing elsewhere', () => {
  for (const [href, main] of [
    [`https://example.com/index.html#note/${ID}`, true],
    [`signet://evil/index.html#note/${ID}`, true],
    [`signet://app/other.html#note/${ID}`, true],
    ['signet://app/index.html', true],
    ['signet://app/index.html#note/not-an-id', true],
    [`signet://app/index.html#folder/${ID}`, true],
    [`signet://app/index.html#note/${ID}`, false]
  ]) {
    const g = notesWindow(href, main)
    load('notes.js')(makeBridge().bridge, config, g)
    assert.equal(g.signet, undefined, href)
    assert.equal(g.signetNotes, undefined, href)
  }
})

test('notes objects cannot be replaced by the page', () => {
  const g = notesWindow(`signet://app/index.html#easel/${ID}`)
  load('notes.js')(makeBridge().bridge, config, g)
  assert.throws(() => {
    'use strict'
    g.signetNotes = {}
  })
})

test('passwords and autofill only act in the main frame of an http(s) page', () => {
  for (const name of ['passwords.js', 'autofill.js']) {
    const core = name === 'autofill.js' ? load('autofill-core.js')() : undefined
    for (const [href, main, expected] of [
      ['https://example.com/', true, true],
      ['http://example.com/', true, true],
      ['https://example.com/', false, false],
      ['signet://app/index.html', true, false],
      ['chrome-extension://abc/popup.html', true, false]
    ]) {
      const w = makeWindow({ href, main })
      w.g.HTMLInputElement.prototype.constructor
      load(name)(makeBridge().bridge, {}, w.g, core)
      assert.equal(Boolean(w.doc.listeners.focusin), expected, `${name} ${href} main=${main}`)
    }
  }
})

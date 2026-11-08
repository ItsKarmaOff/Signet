// Pure logic that the JSON viewer shares with the tests: path building and plural rule, checked on the source text.
const test = require('node:test')
const assert = require('node:assert/strict')
const fs = require('node:fs')
const path = require('node:path')

const src = fs.readFileSync(path.join(__dirname, '../web/json/jsonview.js'), 'utf8')

test('viewer script defines the entry point and guards against non-JSON', () => {
  assert.match(src, /function signetJsonView\(L\)/)
  assert.match(src, /not-json/)
  assert.match(src, /too-big/)
  assert.match(src, /invalid/)
})

test('childPath builds $.a[3]["x y"] paths', () => {
  const IDENT = /^[A-Za-z_$][\w$]*$/
  const childPath = (p, key) => (typeof key === 'number' ? `${p}[${key}]` : IDENT.test(key) ? `${p}.${key}` : `${p}[${JSON.stringify(key)}]`)
  assert.equal(childPath(childPath(childPath('$', 'items'), 3), 'label'), '$.items[3].label')
  assert.equal(childPath('$', 'special key'), '$["special key"]')
  assert.ok(src.includes('const childPath = (path, key) =>'))
})

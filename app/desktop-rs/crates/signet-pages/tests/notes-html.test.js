// Run with: node --test crates/signet-pages/tests
// Port of notesHtml.test.ts (docIcon) plus the pure parts of the sanitizer (no DOM needed).
const test = require('node:test')
const assert = require('node:assert/strict')
const { docIcon, attrAllowed, safeHref, safeSrc, firstLineOf, DROP, KEEP } = require('../web/shared/notes-html.js')
const { rubberband, project } = require('../web/shared/spring.js')

const decode = (url) => decodeURIComponent(url.replace('data:image/svg+xml,', ''))

test('docIcon returns an SVG data URL', () => {
  assert.match(docIcon('note', 200), /^data:image\/svg\+xml,%3Csvg/)
})

test('docIcon colors the background with the given hue', () => {
  assert.ok(decode(docIcon('note', 123)).includes('fill="hsl(123 60% 52%)"'))
})

test('docIcon draws a different glyph for a note and an Easel', () => {
  const note = decode(docIcon('note', 0))
  const easel = decode(docIcon('easel', 0))
  assert.ok(note.includes('M9 7h14') && !note.includes('<circle'))
  assert.ok(easel.includes('<circle') && !easel.includes('M9 7h14'))
})

test('docIcon is well-formed SVG', () => {
  const svg = decode(docIcon('easel', 10))
  assert.ok(svg.startsWith('<svg') && svg.endsWith('</svg>'))
})

test('links: only http, https and mailto', () => {
  assert.ok(safeHref('https://a.b') && safeHref(' http://a.b') && safeHref('mailto:a@b.c'))
  assert.ok(!safeHref('javascript:alert(1)') && !safeHref('data:text/html,x') && !safeHref('vbscript:x'))
})

test('images: http(s) and base64 raster data URLs only', () => {
  assert.ok(safeSrc('https://a.b/i.png') && safeSrc('data:image/webp;base64,AAAA'))
  assert.ok(!safeSrc('data:image/svg+xml;base64,AAAA') && !safeSrc('data:text/html;base64,AAAA') && !safeSrc('javascript:x'))
})

test('attributes: allow list per tag', () => {
  assert.ok(attrAllowed('A', 'HREF', 'https://x.y'))
  assert.ok(!attrAllowed('A', 'href', 'javascript:x'))
  assert.ok(!attrAllowed('A', 'onclick', 'x()'))
  assert.ok(!attrAllowed('P', 'style', 'color:red'))
  assert.ok(attrAllowed('IMG', 'src', 'https://x.y/a.gif'))
  assert.ok(!attrAllowed('IMG', 'onerror', 'x()'))
  assert.ok(attrAllowed('LI', 'data-checked', 'true') && attrAllowed('LI', 'data-checked', 'false'))
  assert.ok(!attrAllowed('LI', 'data-checked', 'yes'))
  assert.ok(attrAllowed('UL', 'class', 'checklist') && !attrAllowed('UL', 'class', 'x'))
  assert.ok(!attrAllowed('SPAN', 'class', 'checklist'))
})

test('tag lists: dangerous tags are dropped, text tags kept', () => {
  for (const t of ['SCRIPT', 'IFRAME', 'STYLE', 'SVG', 'FORM', 'OBJECT']) assert.ok(DROP.has(t) && !KEEP.has(t), t)
  for (const t of ['P', 'A', 'IMG', 'UL', 'LI', 'BLOCKQUOTE', 'PRE', 'H1']) assert.ok(KEEP.has(t) && !DROP.has(t), t)
})

test('firstLineOf: first non-empty line, cut with an ellipsis', () => {
  assert.equal(firstLineOf('\n  \n Hello \nworld'), 'Hello')
  assert.equal(firstLineOf(''), '')
  assert.equal(firstLineOf('abcdef', 4), 'abc…')
  assert.equal(firstLineOf('abcd', 4), 'abcd')
})

test('spring helpers', () => {
  assert.equal(rubberband(0, 1), 0)
  assert.ok(rubberband(10, 1) > 0 && rubberband(10, 1) < 1)
  assert.equal(rubberband(-3, 1), -rubberband(3, 1))
  assert.ok(project(1000) > 0)
})

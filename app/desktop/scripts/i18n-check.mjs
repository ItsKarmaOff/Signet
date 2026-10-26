// Translation check: same keys in English and French (main.json, ui.json, mobile.json),
// same {{…}} variables, and no visible text hard-coded in src/renderer/src/**, app/mobile/src/** and app/mobile/app/**.
// Usage: npm run i18n:check
import { existsSync, readFileSync, readdirSync, statSync } from 'node:fs'
import { join, relative, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'
import ts from 'typescript'

const root = join(dirname(fileURLToPath(import.meta.url)), '..')
const locales = join(root, '../../packages/shared/src/locales')
const mobileDirs = [join(root, '../mobile/src'), join(root, '../mobile/app')]
const problems = []

// ── 1. Catalogs ──
function flatten(obj, prefix = '', out = {}) {
  for (const [k, v] of Object.entries(obj)) {
    const key = prefix ? `${prefix}.${k}` : k
    if (v && typeof v === 'object') flatten(v, key, out)
    else out[key] = String(v)
  }
  return out
}
const vars = (s) =>
  [...s.matchAll(/\{\{\s*([\w.]+)\s*(?:,[^}]*)?\}\}/g)]
    .map((m) => m[1])
    .sort()
    .join(',')
for (const ns of ['main', 'ui', 'mobile']) {
  const en = flatten(JSON.parse(readFileSync(join(locales, 'en', `${ns}.json`), 'utf8')))
  const fr = flatten(JSON.parse(readFileSync(join(locales, 'fr', `${ns}.json`), 'utf8')))
  for (const k of Object.keys(en)) if (!(k in fr)) problems.push(`${ns}.json: key missing in French: ${k}`)
  for (const k of Object.keys(fr)) if (!(k in en)) problems.push(`${ns}.json: key missing in English: ${k}`)
  for (const k of Object.keys(en)) if (k in fr && vars(en[k]) !== vars(fr[k])) problems.push(`${ns}.json: variables differ for ${k}`)
  for (const [k, v] of Object.entries({ ...en, ...fr })) if (v.trim() === '') problems.push(`${ns}.json: empty value for ${k}`)
}

// ── 2. Hard-coded text in the UI ──
/** Proper nouns and acronyms that stay as they are in every language. */
const ALLOWED = new Set(['Signet', 'Little Signet', 'Peek', 'Boost', 'CSS', 'URL'])
const HAS_LETTER = /\p{L}/u
// Translation key (`sidebar.newTab`) or technical identifier: not displayed text.
const KEY_LIKE = /^[a-z][\w-]*(\.[\w-]+)+$/
// Keyboard shortcuts (⌘T, ⇧⌘G, ⌃1 … ⌃9): key symbols followed by at most one letter.
const SHORTCUT = /^[\s⌘⇧⌥⌃⇥→←↑↓+\-=,.[\]0-9…]*[A-Za-z]?[\s⌘⇧⌥⌃⇥→←↑↓+\-=,.[\]0-9…]*$/u

function visible(text) {
  const t = text.replace(/\s+/g, ' ').trim()
  if (!t || !HAS_LETTER.test(t) || KEY_LIKE.test(t) || ALLOWED.has(t) || SHORTCUT.test(t)) return false
  return true
}

const ATTRS = new Set(['title', 'aria-label', 'placeholder', 'alt', 'accessibilityLabel'])
const files = []
;(function walk(dir) {
  for (const f of readdirSync(dir)) {
    const p = join(dir, f)
    if (statSync(p).isDirectory()) walk(p)
    else if (/\.tsx$/.test(f)) files.push(p)
  }
})(join(root, 'src/renderer/src'))
// Mobile UI (React Native): same rules.
for (const mobileSrc of mobileDirs)
  if (existsSync(mobileSrc))
    (function walkMobile(dir) {
      for (const f of readdirSync(dir)) {
        const p = join(dir, f)
        if (statSync(p).isDirectory()) walkMobile(p)
        else if (/\.tsx$/.test(f)) files.push(p)
      }
    })(mobileSrc)

function literals(node, out) {
  if (ts.isStringLiteralLike(node)) out.push(node)
  else if (ts.isTemplateExpression(node)) out.push(node.head, ...node.templateSpans.map((s) => s.literal))
  // In an attribute expression, a t(...) call or a key does not count; we descend into ternaries and concatenations.
  else if (ts.isConditionalExpression(node)) {
    literals(node.whenTrue, out)
    literals(node.whenFalse, out)
  } else if (ts.isBinaryExpression(node)) {
    const op = node.operatorToken.kind
    // Comparisons (=== 'history') and the condition of an && are not displayed.
    if (op === ts.SyntaxKind.PlusToken || op === ts.SyntaxKind.BarBarToken || op === ts.SyntaxKind.QuestionQuestionToken) literals(node.left, out)
    if (
      op === ts.SyntaxKind.PlusToken ||
      op === ts.SyntaxKind.BarBarToken ||
      op === ts.SyntaxKind.QuestionQuestionToken ||
      op === ts.SyntaxKind.AmpersandAmpersandToken
    )
      literals(node.right, out)
  } else if (ts.isParenthesizedExpression(node)) literals(node.expression, out)
}

for (const file of files) {
  const src = ts.createSourceFile(file, readFileSync(file, 'utf8'), ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX)
  const rel = relative(root, file)
  const report = (node, text) => {
    const { line } = src.getLineAndCharacterOfPosition(node.getStart())
    problems.push(`${rel}:${line + 1}: hard-coded text "${text.replace(/\s+/g, ' ').trim()}"`)
  }
  const visit = (node) => {
    if (ts.isJsxText(node) && visible(node.text)) report(node, node.text)
    else if (ts.isJsxExpression(node) && node.expression && !ts.isJsxAttribute(node.parent)) {
      const out = []
      literals(node.expression, out)
      for (const l of out) if (visible(l.text)) report(l, l.text)
    } else if (ts.isJsxAttribute(node) && ATTRS.has(node.name.getText()) && node.initializer) {
      const out = []
      if (ts.isStringLiteral(node.initializer)) out.push(node.initializer)
      else if (ts.isJsxExpression(node.initializer) && node.initializer.expression) literals(node.initializer.expression, out)
      for (const l of out) if (visible(l.text)) report(l, l.text)
    }
    ts.forEachChild(node, visit)
  }
  visit(src)
}

if (problems.length) {
  console.error(`i18n:check: ${problems.length} problem(s)\n` + problems.map((p) => `  - ${p}`).join('\n'))
  process.exit(1)
}
console.log('i18n:check: OK (catalogs aligned, no hard-coded text in src/renderer/src, app/mobile/src or app/mobile/app)')

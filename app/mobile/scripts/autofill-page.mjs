// Bundles the autofill page script (TypeScript, DOM code) into a JavaScript string the app injects into pages.
//   npm run autofill:page -w @signet/mobile            writes src/features/autofill/pageScript.generated.ts
//   npm run autofill:page -w @signet/mobile -- --check  exits 1 when the committed file is out of date
import { readFileSync, writeFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { build } from 'vite'

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const entry = resolve(root, 'src/features/autofill/page/autofill.page.ts')
const out = resolve(root, 'src/features/autofill/pageScript.generated.ts')

const result = await build({
  root,
  configFile: false,
  logLevel: 'silent',
  build: {
    write: false,
    minify: true,
    target: 'safari15',
    lib: { entry, formats: ['iife'], name: 'SignetAutofillPage', fileName: 'autofill' }
  }
})
const outputs = (Array.isArray(result) ? result : [result]).flatMap((r) => ('output' in r ? r.output : []))
const chunk = outputs.find((o) => o.type === 'chunk')
if (!chunk) throw new Error('autofill page: nothing was built')
// Free identifier `SIGNET_TOKEN` is bound by the app (see pageScript.ts); strip the global the IIFE would assign.
const code = chunk.code.trim().replace(/^var SignetAutofillPage\s*=\s*/, '')

const source = `// GENERATED FILE, do not edit. Source: src/features/autofill/page/autofill.page.ts
// Regenerate with: npm run autofill:page -w @signet/mobile   (check: add -- --check)
// The code expects a variable \`SIGNET_TOKEN\` in scope (see pageScript.ts).
export const AUTOFILL_PAGE_CODE = ${JSON.stringify(code)}
`

if (process.argv.includes('--check')) {
  let current = ''
  try {
    current = readFileSync(out, 'utf8')
  } catch {
    // missing file: out of date
  }
  if (current !== source) {
    console.error('pageScript.generated.ts is out of date: run `npm run autofill:page -w @signet/mobile`')
    process.exit(1)
  }
  console.log('pageScript.generated.ts is up to date')
} else {
  writeFileSync(out, source)
  console.log(`wrote ${out} (${code.length} characters)`)
}

// Converts EasyList and EasyPrivacy into WebKit blocking rules (WKContentRuleList) for the native signet-web module.
// Usage: node scripts/content-rules.mjs <easylist.txt> <easyprivacy.txt>
//   (lists downloaded once: https://easylist.to/easylist/easylist.txt and easyprivacy.txt)
// Output: modules/signet-web/ios/rules/signet-rules-{ads,privacy}.json + signet-rules-manifest.json. Only these JSON files are versioned; the module bundles them into the app.
// Node ≥ 22.18 (the shared package's TypeScript is read directly).
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { convertFilterList } from '../../../packages/shared/src/mobile/content-rules.ts'

const [easylist, easyprivacy] = process.argv.slice(2)
if (!easylist || !easyprivacy) {
  console.error('Usage : node scripts/content-rules.mjs <easylist.txt> <easyprivacy.txt>')
  process.exit(1)
}
const out = join(dirname(fileURLToPath(import.meta.url)), '..', 'modules', 'signet-web', 'ios', 'rules')
mkdirSync(out, { recursive: true })

const LIMIT = 150000
const header = (text, key) => new RegExp(`^! ${key}: (.+)$`, 'm').exec(text)?.[1].trim() ?? ''

const lists = [
  { id: 'ads', title: 'EasyList', file: easylist, options: { siteCosmetic: true } },
  { id: 'privacy', title: 'EasyPrivacy', file: easyprivacy, options: { siteCosmetic: false } }
]
const manifest = { generated: new Date().toISOString().slice(0, 10), license: 'GPL-3.0-or-later OR CC-BY-SA-3.0-or-later', lists: [] }
let total = 0
for (const l of lists) {
  const text = readFileSync(l.file, 'utf8')
  const { rules, stats } = convertFilterList(text, l.options)
  const version = header(text, 'Version') || manifest.generated
  const file = `signet-rules-${l.id}.json`
  writeFileSync(join(out, file), JSON.stringify(rules))
  total += rules.length
  manifest.lists.push({ id: l.id, title: l.title, version, modified: header(text, 'Last modified'), file, count: rules.length, source: 'https://easylist.to/' })
  console.log(
    `${l.title} ${version}: ${rules.length} rules (network ${stats.network}, exceptions ${stats.exceptions}, hiding ${stats.cosmetic} lines, skipped ${stats.skipped})`
  )
}
if (total > LIMIT) {
  console.error(`Too many rules (${total} > ${LIMIT}): WebKit would reject the list.`)
  process.exit(1)
}
writeFileSync(join(out, 'signet-rules-manifest.json'), JSON.stringify(manifest, null, 2) + '\n')
console.log(`Total: ${total} rules in ${out}`)

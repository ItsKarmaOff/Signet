// Checks that a version matches every version field of the repository (all package.json files and the Expo app.json).
// Used by scripts/release.sh before tagging, and by the Build workflow on a `v…` tag.
//
//   node scripts/check-version.mjs 1.2.0    (a leading `v` is accepted: v1.2.0)
//   node scripts/check-version.mjs          prints the root package.json version
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = join(dirname(fileURLToPath(import.meta.url)), '..')

// [file, how to read its version]
const sources = [
  ['package.json', (json) => json.version],
  ['app/desktop/package.json', (json) => json.version],
  ['app/mobile/package.json', (json) => json.version],
  ['packages/shared/package.json', (json) => json.version],
  ['app/mobile/app.json', (json) => json.expo?.version]
]

const versionOf = ([file, read]) => read(JSON.parse(readFileSync(join(root, file), 'utf8')))

const arg = process.argv[2]
if (!arg) {
  console.log(versionOf(sources[0]))
  process.exit(0)
}

const wanted = arg.replace(/^v/, '')
const semver = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(-[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*)?$/
if (!semver.test(wanted)) {
  console.error(`"${arg}" is not a valid version (expected X.Y.Z or X.Y.Z-pre.N, e.g. 1.2.0 or 1.2.0-beta.1).`)
  process.exit(1)
}

const mismatches = sources.map((source) => [source[0], versionOf(source)]).filter(([, version]) => version !== wanted)
if (mismatches.length > 0) {
  console.error(`Version ${wanted} does not match:`)
  for (const [file, version] of mismatches) console.error(`  ${file}: ${version ?? '(no version)'}`)
  console.error('Update these files (and run npm install to refresh package-lock.json), commit, then try again.')
  process.exit(1)
}

console.log(`Version ${wanted} matches all ${sources.length} version fields.`)

// Generates the mobile app icons from the desktop SVGs.
// iOS: 1024x1024 squares, opaque, no margin or rounded corners (iOS applies its own mask).
//   Writes assets/app-icons/<id>.png and assets/icon.png (= aurore, the default icon).
// Android (adaptive icons, 108 dp canvas rendered at 1024 px): three layers per icon in assets/android/:
//   <id>-background.png (full-bleed background, opaque), <id>-foreground.png (the logo alone on transparent, scaled down to fit in
//   the 66% safe zone), <id>-monochrome.png (solid logo silhouette for Android 13+ themed icons).
// Rendering: offscreen Electron, like app/desktop/scripts/icons/generate.mjs. Usage: `npm run icons -w @signet/mobile`
import { copyFileSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs'
import { execFileSync } from 'node:child_process'
import { tmpdir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { createRequire } from 'node:module'

const here = dirname(fileURLToPath(import.meta.url))
const mobile = resolve(here, '..')
const source = resolve(mobile, '../desktop/resources/icons/svg')
const out = join(mobile, 'assets/app-icons')
const outAndroid = join(mobile, 'assets/android')

/** The desktop artwork is an 824 px squircle centered in 1024 px. Backgrounds go full-bleed, the rim is removed
 *  and the logo is scaled up accordingly (824 → 1024) to keep its proportions: the ribbon always touches the top edge. */
const SQUIRCLE = /<rect x="100" y="100" width="824" height="824" rx="185"/g
const RIM = /\s*<rect x="100\.5"[^>]*\/>/
const CLIP = '<g clip-path="url(#sq)">'
const GROW = 'translate(512 512) scale(1.2427) translate(-512 -512)'
function check(svg, id) {
  if (!svg.includes('<rect x="100" y="100" width="824" height="824" rx="185"')) throw new Error(`${id}.svg : squircle introuvable`)
  if (!RIM.test(svg)) throw new Error(`${id}.svg: rim not found`)
  if (!svg.includes(CLIP) || !svg.includes('<g id="glyph">')) throw new Error(`${id}.svg : logo introuvable`)
}
function fullBleed(svg, id) {
  check(svg, id)
  return svg.replace(SQUIRCLE, '<rect x="0" y="0" width="1024" height="1024"').replace(RIM, '').replace(CLIP, `<g transform="${GROW}">`)
}

/** The logo alone: the `<g id="glyph">` group, closed by the second-to-last `</g>` (the last one closes the clipped group). */
function glyphOf(svg, id) {
  check(svg, id)
  const start = svg.indexOf('<g id="glyph">')
  const end = svg.lastIndexOf('</g>', svg.lastIndexOf('</g>') - 1) + 4
  return svg.slice(start, end)
}
const defsOf = (svg) => /<defs>[\s\S]*<\/defs>/.exec(svg)[0]

/** Background alone: the full-bleed squircle, without the logo. */
function backgroundLayer(svg, id) {
  const full = fullBleed(svg, id)
  return full.slice(0, full.indexOf(`<g transform="${GROW}">`)) + '</svg>\n'
}

/** Adaptive icons: only the central 72 dp square out of 108 (683 px out of 1024) is visible. The desktop squircle (824 px)
 *  is scaled down to it (× 0.829): the lines stay inside the 66% safe circle and the ribbon reaches the mask edge. */
const FIT = 'translate(512 512) scale(.829) translate(-512 -512)'

/** Logo alone on transparent, in its original colors. */
function foregroundLayer(svg, id) {
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024">${defsOf(svg)}<g transform="${FIT}">${glyphOf(svg, id)}</g></svg>`
}

/** Silhouette: ribbon and lines in solid black (Android recolors it), no shadow or highlight. */
function monochromeLayer(svg, id) {
  const glyph = glyphOf(svg, id)
    .replace(/ filter="url\(#sh\)"/, '')
    .replace(/\s*<path class="sheen"[^>]*\/>/, '')
    .replace('fill="url(#ribbon)"', 'fill="#000"')
    .replace(/stroke="[^"]*" stroke-opacity="[^"]*"/, 'stroke="#000"')
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024"><g transform="${FIT}">${glyph}</g></svg>`
}

const ids = readdirSync(source)
  .filter((f) => f.endsWith('.svg'))
  .map((f) => f.slice(0, -4))
  .sort()
const work = join(tmpdir(), 'signet-app-icons')
rmSync(work, { recursive: true, force: true })
mkdirSync(join(work, 'svg'), { recursive: true })
mkdirSync(out, { recursive: true })
for (const id of ids) writeFileSync(join(work, 'svg', id + '.svg'), fullBleed(readFileSync(join(source, id + '.svg'), 'utf8'), id))

const androidLayers = []
for (const id of ids) {
  const svg = readFileSync(join(source, id + '.svg'), 'utf8')
  for (const [layer, build] of [
    ['background', backgroundLayer],
    ['foreground', foregroundLayer],
    ['monochrome', monochromeLayer]
  ]) {
    const name = `${id}-${layer}`
    writeFileSync(join(work, 'svg', name + '.svg'), build(svg, id))
    androidLayers.push(name)
  }
}
mkdirSync(outAndroid, { recursive: true })

const electron = createRequire(import.meta.url)('electron')
execFileSync(electron, [join(here, 'app-icons-render.cjs'), work, out, ...ids], { stdio: 'inherit', env: { ...process.env, ELECTRON_RUN_AS_NODE: '' } })
execFileSync(electron, [join(here, 'app-icons-render.cjs'), work, outAndroid, ...androidLayers], {
  stdio: 'inherit',
  env: { ...process.env, ELECTRON_RUN_AS_NODE: '' }
})
copyFileSync(join(out, 'aurore.png'), join(mobile, 'assets/icon.png'))
rmSync(work, { recursive: true, force: true })
console.log('Icons generated:', ids.join(', '))

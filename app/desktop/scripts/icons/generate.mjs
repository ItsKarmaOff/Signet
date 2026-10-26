// Generates the icon variants: SVG (bookmark drawing below), PNG 1024 (via offscreen Electron) and build/icon.icns.
// The drawing: a bookmark (book ribbon) hanging from the top edge, on the left, and three lines of text on the right:
// the sidebar and the page. The variants only change the colors.
import { writeFileSync, mkdirSync, rmSync } from 'node:fs'
import { execFileSync } from 'node:child_process'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { createRequire } from 'node:module'

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..')
const icons = join(root, 'resources/icons')

// bg: 3 background stops, glow: glow opacity, ribbon: 3 ribbon stops (left → right),
// sheen: opacity of the shine on the ribbon, lines/linesOpacity: text lines, shadow/shadowOpacity: ribbon shadow,
// rim/rimOpacity: squircle rim
const V = {
  aurore: {
    bg: ['#2a3864', '#1f2a4b', '#141b33'],
    glow: 0.1,
    ribbon: ['#f0645e', '#d63a3f', '#b42a33'],
    sheen: 0.22,
    lines: '#4a5c8a',
    linesOpacity: 1,
    shadow: '#060a18',
    shadowOpacity: 0.5
  },
  ocean: {
    bg: ['#2563eb', '#0891b2', '#2dd4bf'],
    glow: 0.22,
    ribbon: ['#fde68a', '#fbbf24', '#f59e0b'],
    sheen: 0.35,
    lines: '#ffffff',
    linesOpacity: 0.5,
    shadow: '#0c2d5e',
    shadowOpacity: 0.4
  },
  corail: {
    bg: ['#fb923c', '#f43f5e', '#e11d74'],
    glow: 0.22,
    ribbon: ['#ffffff', '#fff1f4', '#ffe1e8'],
    sheen: 0,
    lines: '#ffffff',
    linesOpacity: 0.5,
    shadow: '#7f1d1d',
    shadowOpacity: 0.4
  },
  menthe: {
    bg: ['#059669', '#10b981', '#a3e635'],
    glow: 0.22,
    ribbon: ['#ffffff', '#effdf5', '#d9f9e6'],
    sheen: 0,
    lines: '#ffffff',
    linesOpacity: 0.5,
    shadow: '#064e3b',
    shadowOpacity: 0.4
  },
  nuit: {
    bg: ['#0b0b14', '#1e1b4b', '#312e81'],
    glow: 0.08,
    ribbon: ['#c4b5fd', '#a78bfa', '#8b5cf6'],
    sheen: 0.3,
    lines: '#6366f1',
    linesOpacity: 0.7,
    shadow: '#000000',
    shadowOpacity: 0.5
  },
  papier: {
    bg: ['#ffffff', '#f8f7fb', '#efedf5'],
    glow: 0,
    ribbon: ['#f0645e', '#d63a3f', '#b42a33'],
    sheen: 0.22,
    lines: '#cfcbdc',
    linesOpacity: 1,
    shadow: '#1d1d1f',
    shadowOpacity: 0.18,
    rim: '#000000',
    rimOpacity: 0.06
  },
  graphite: {
    bg: ['#52525b', '#27272a', '#18181b'],
    glow: 0.08,
    ribbon: ['#fafafa', '#e4e4e7', '#a1a1aa'],
    sheen: 0,
    lines: '#71717a',
    linesOpacity: 1,
    shadow: '#000000',
    shadowOpacity: 0.45
  },
  or: {
    bg: ['#1c1917', '#292524', '#44403c'],
    glow: 0.08,
    ribbon: ['#fde68a', '#f59e0b', '#b45309'],
    sheen: 0.3,
    lines: '#78716c',
    linesOpacity: 1,
    shadow: '#000000',
    shadowOpacity: 0.45
  }
}

// Numbers (opacities): two decimals without leading zero (.10), 0 and 1 as they are
const n = (x) => (x > 0 && x < 1 ? x.toFixed(2).replace(/^0\./, '.') : String(x))

// The squircle is 824 px, centered in 1024 px (100 px margin, like macOS icons).
// The ribbon starts well above the canvas: the squircle cuts it on desktop, and mobile icons (scaled up or
// down around the center) keep a ribbon that touches the top edge. The `id="glyph"` group is the logo alone,
// reused by app/mobile/scripts/app-icons.mjs for the Android layers.
function render(v) {
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024">
  <defs>
    <linearGradient id="bg" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0" stop-color="${v.bg[0]}"/>
      <stop offset=".55" stop-color="${v.bg[1]}"/>
      <stop offset="1" stop-color="${v.bg[2]}"/>
    </linearGradient>
    <radialGradient id="glow" cx=".3" cy=".15" r=".8">
      <stop offset="0" stop-color="#ffffff" stop-opacity="${n(v.glow)}"/>
      <stop offset="1" stop-color="#ffffff" stop-opacity="0"/>
    </radialGradient>
    <linearGradient id="ribbon" gradientUnits="userSpaceOnUse" x1="347" y1="0" x2="512" y2="0">
      <stop offset="0" stop-color="${v.ribbon[0]}"/>
      <stop offset=".55" stop-color="${v.ribbon[1]}"/>
      <stop offset="1" stop-color="${v.ribbon[2]}"/>
    </linearGradient>
    <filter id="sh" x="-40%" y="-10%" width="180%" height="130%">
      <feDropShadow dx="16" dy="20" stdDeviation="14" flood-color="${v.shadow}" flood-opacity="${n(v.shadowOpacity)}"/>
    </filter>
    <clipPath id="sq"><rect x="100" y="100" width="824" height="824" rx="185"/></clipPath>
  </defs>
  <rect x="100" y="100" width="824" height="824" rx="185" fill="url(#bg)"/>
  <rect x="100" y="100" width="824" height="824" rx="185" fill="url(#glow)"/>
  <rect x="100.5" y="100.5" width="823" height="823" rx="184.5" fill="none" stroke="${v.rim ?? '#ffffff'}" stroke-opacity="${n(v.rimOpacity ?? 0.08)}"/>
  <g clip-path="url(#sq)">
    <g id="glyph">
      <g filter="url(#sh)">
        <path d="M347 -300 H512 V726 L430 644 L347 726 Z" fill="url(#ribbon)"/>
        <path class="sheen" d="M360 -300 H378 V695 L360 713 Z" fill="#ffffff" fill-opacity="${n(v.sheen)}"/>
      </g>
      <g stroke="${v.lines}" stroke-opacity="${n(v.linesOpacity)}" stroke-width="37" stroke-linecap="round">
        <line x1="594" y1="397" x2="743" y2="397"/>
        <line x1="594" y1="496" x2="710" y2="496"/>
        <line x1="594" y1="594" x2="734" y2="594"/>
      </g>
    </g>
  </g>
</svg>
`
}

mkdirSync(join(icons, 'svg'), { recursive: true })
for (const [id, v] of Object.entries(V)) writeFileSync(join(icons, 'svg', id + '.svg'), render(v))

// PNG via offscreen Electron
const electron = createRequire(import.meta.url)('electron')
execFileSync(electron, [join(root, 'scripts/icons/render.cjs'), icons, ...Object.keys(V)], {
  stdio: 'inherit',
  env: { ...process.env, ELECTRON_RUN_AS_NODE: '' }
})

// .icns from aurore
const set = join(root, 'build/icon.iconset')
rmSync(set, { recursive: true, force: true })
mkdirSync(set, { recursive: true })
const src = join(icons, 'aurore.png')
for (const s of [16, 32, 128, 256, 512]) {
  for (const k of [1, 2]) {
    const out = join(set, k === 1 ? `icon_${s}x${s}.png` : `icon_${s}x${s}@2x.png`)
    execFileSync('sips', ['-z', String(s * k), String(s * k), src, '--out', out], { stdio: 'ignore' })
  }
}
execFileSync('iconutil', ['-c', 'icns', set, '-o', join(root, 'build/icon.icns')])
rmSync(set, { recursive: true, force: true })
console.log('Icons generated:', Object.keys(V).join(', '))

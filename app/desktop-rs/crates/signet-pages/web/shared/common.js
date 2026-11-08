// Shared by the Note and Easel pages: page configuration injected by the app, translations, icons, small helpers.
// Classic script. The configuration is the JSON in <script type="application/json" id="signet-config">:
//   { "lang": "en", "theme": "auto" | "light" | "dark", "strings": { "notes.bold": "Bold", ... } }
;(function (root) {
  'use strict'

  let config = { lang: 'en', theme: 'auto', strings: {} }
  try {
    const el = document.getElementById('signet-config')
    if (el) config = { ...config, ...JSON.parse(el.textContent || '{}') }
  } catch {
    // keep defaults: keys are shown as is
  }

  /** Translated text for a `section.key` of ui.json (the key itself if missing). */
  const t = (key) => config.strings[key] ?? key

  /** Sets the appearance at runtime: 'auto' follows the system. */
  function setTheme(theme) {
    const cl = document.documentElement.classList
    cl.toggle('theme-light', theme === 'light')
    cl.toggle('theme-dark', theme === 'dark')
  }

  // Inner SVG of 24x24 stroke icons (shapes of the Lucide set used by the React pages).
  const ICONS = {
    h1: '<path d="M4 12h8M4 18V6M12 18V6M17 12l3-2v8"/>',
    h2: '<path d="M4 12h8M4 18V6M12 18V6M21 18h-4c0-4 4-3 4-6 0-1.5-2-2.5-4-1"/>',
    bold: '<path d="M6 12h9a4 4 0 0 1 0 8H7a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1h7a4 4 0 0 1 0 8"/>',
    italic: '<path d="M19 4h-9M14 20H5M15 4 9 20"/>',
    strike: '<path d="M16 4H9a3 3 0 0 0-2.83 4M14 12a4 4 0 0 1 0 8H6M4 12h16"/>',
    list: '<path d="M3 5h.01M3 12h.01M3 19h.01M8 5h13M8 12h13M8 19h13"/>',
    ordered: '<path d="M11 5h10M11 12h10M11 19h10M4 4h1v5M4 9h2M6.5 20H3.4c0-1 2.6-1.9 2.6-3.5a1.5 1.5 0 0 0-2.6-1"/>',
    checks: '<path d="m3 17 2 2 4-4M3 7l2 2 4-4M13 6h8M13 12h8M13 18h8"/>',
    quote: '<path d="M16 3a2 2 0 0 0-2 2v6a2 2 0 0 0 2 2 1 1 0 0 1 1 1v1a2 2 0 0 1-2 2 1 1 0 0 0-1 1v2a1 1 0 0 0 1 1 6 6 0 0 0 6-6V5a2 2 0 0 0-2-2zM5 3a2 2 0 0 0-2 2v6a2 2 0 0 0 2 2 1 1 0 0 1 1 1v1a2 2 0 0 1-2 2 1 1 0 0 0-1 1v2a1 1 0 0 0 1 1 6 6 0 0 0 6-6V5a2 2 0 0 0-2-2z"/>',
    code: '<path d="m16 18 6-6-6-6M8 6l-6 6 6 6"/>',
    link: '<path d="M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71"/>',
    trash: '<path d="M3 6h18M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2M10 11v6M14 11v6"/>',
    pointer: '<path d="M4.04 4.69a.5.5 0 0 1 .65-.65l16 6.5a.5.5 0 0 1-.06.95l-6.12 1.58a2 2 0 0 0-1.44 1.43l-1.58 6.13a.5.5 0 0 1-.95.06z"/>',
    type: '<path d="M12 4v16M4 7V5a1 1 0 0 1 1-1h14a1 1 0 0 1 1 1v2M9 20h6"/>',
    pencil: '<path d="M21.17 6.81a1 1 0 0 0-3.99-3.99L3.84 16.17a2 2 0 0 0-.5.83l-1.32 4.35a.5.5 0 0 0 .62.62l4.35-1.32a2 2 0 0 0 .83-.5zM15 5l4 4"/>',
    imagePlus: '<path d="M16 5h6M19 2v6M21 11.5V19a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h7.5M21 15l-3.09-3.09a2 2 0 0 0-2.82 0L6 21"/><circle cx="9" cy="9" r="2"/>',
    minus: '<path d="M5 12h14"/>',
    plus: '<path d="M5 12h14M12 5v14"/>',
    erase: '<path d="M10 5a2 2 0 0 0-1.34.52l-6.33 5.74a1 1 0 0 0 0 1.48l6.33 5.74A2 2 0 0 0 10 19h10a2 2 0 0 0 2-2V7a2 2 0 0 0-2-2zM12 9l6 6M18 9l-6 6"/>'
  }

  /** SVG element of an icon. */
  function icon(name, size = 16) {
    const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg')
    svg.setAttribute('class', 'icon')
    svg.setAttribute('viewBox', '0 0 24 24')
    svg.setAttribute('width', String(size))
    svg.setAttribute('height', String(size))
    svg.setAttribute('aria-hidden', 'true')
    svg.innerHTML = ICONS[name] || ''
    return svg
  }

  /** Element helper: h('div', 'class', {attr}, ...children). */
  function h(tag, cls, attrs, ...kids) {
    const el = document.createElement(tag)
    if (cls) el.className = cls
    if (attrs) for (const [k, v] of Object.entries(attrs)) if (v !== undefined && v !== null) el.setAttribute(k, String(v))
    for (const k of kids) el.append(k)
    return el
  }

  /** Toolbar button with an icon. `mousedown` is prevented so the editor keeps its selection. */
  function tool(label, iconName, size, run, { keepFocus = true } = {}) {
    const b = h('button', 'note-tool', { type: 'button', title: label, 'aria-label': label }, icon(iconName, size))
    if (keepFocus) b.addEventListener('mousedown', (e) => e.preventDefault())
    b.addEventListener('click', run)
    return b
  }

  const clamp = (n, min, max) => Math.min(max, Math.max(min, n))

  setTheme(config.theme)
  if (config.lang) document.documentElement.lang = config.lang

  root.Signet = { config, t, setTheme, icon, h, tool, clamp }
})(window)

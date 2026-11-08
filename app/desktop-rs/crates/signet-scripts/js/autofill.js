// Port of preload/autofill.ts. Address and card autofill. Main frame only, document start, http(s) pages.
// Shows Signet's saved addresses or cards under a field the user touches (never silent filling), fills the form on a
// real click, and reports submitted addresses and cards so the browser side can offer to save them.
// Origin, settings, authentication and the https rule for cards are all decided by the browser side.
// Channels: af:menu (request), af:fill (request), af:manage (send), af:submit (send).
// `core` is the object returned by autofill-core.js.
(function autofill(bridge, config, g, core) {
  'use strict'
  g = g || globalThis
  if (g.top !== g || !/^https?:$/.test(g.location.protocol)) return

  const {
    classifyFields, isAddressForm, isAddressKind, isCardForm, isCardKind, sameGroup,
    formatExpiry, formatYear, groupCardNumber, NETWORK_NAMES,
    addressValue, pickMonthOption, pickOption, placeCandidates, yearCandidates
  } = core

  const doc = g.document
  const MAX_FIELDS = 150
  const locales = [...g.navigator.languages]
  const nowMs = () => g.Date.now()

  // ───────────── Field detection ─────────────

  const isField = (el) => el instanceof g.HTMLInputElement || el instanceof g.HTMLSelectElement || el instanceof g.HTMLTextAreaElement
  const isText = (el) => el instanceof g.HTMLInputElement || el instanceof g.HTMLTextAreaElement
  const visible = (el) => el.getClientRects().length > 0 && g.getComputedStyle(el).visibility !== 'hidden'

  function labelOf(el) {
    const parts = []
    for (const l of el.labels || []) parts.push(l.textContent || '')
    for (const id of (el.getAttribute('aria-labelledby') || '').split(/\s+/).filter(Boolean)) {
      const target = doc.getElementById(id)
      parts.push((target && target.textContent) || '')
    }
    return parts.join(' ').replace(/\s+/g, ' ').trim().slice(0, 120)
  }

  function describe(el) {
    const type = el instanceof g.HTMLSelectElement ? 'select' : el instanceof g.HTMLTextAreaElement ? 'textarea' : el.type
    // Cheap attribute checks first: the layout test only runs for fields that could matter.
    const hidden = type === 'hidden' || el.disabled || !visible(el)
    return {
      type,
      autocomplete: el.getAttribute('autocomplete') || '',
      name: el.name,
      id: el.id,
      label: labelOf(el),
      placeholder: 'placeholder' in el ? el.placeholder : '',
      ariaLabel: el.getAttribute('aria-label') || '',
      hidden,
      disabled: el.disabled,
      readOnly: 'readOnly' in el && el.readOnly
    }
  }

  /** The fields of the form around `el` (or of the whole page when it has no form), classified. */
  function scan(root) {
    const fields = [...root.querySelectorAll('input, select, textarea')].slice(0, MAX_FIELDS)
    return { fields, classes: classifyFields(fields.map(describe)) }
  }

  /** Scope of a scan: the element's form, or the whole page when it has none. */
  const rootOf = (el) => (el instanceof g.HTMLFormElement ? el : ((isField(el) || el instanceof g.HTMLButtonElement) && el.form) || doc)

  const isEmpty = (f) => (f instanceof g.HTMLSelectElement ? f.value === '' || (f.selectedIndex <= 0 && !(f.options[0] && f.options[0].value)) : !f.value.trim())

  // ───────────── Dropdown ─────────────

  let host = null
  let shadow = null
  let field = null
  let menu = null
  let rows = []
  let active = -1
  let items = []
  let shownAt = 0
  let token = 0

  const CSS = `
    :host { all: initial }
    .menu { position: fixed; z-index: 2147483647; min-width: 220px; max-width: 360px; box-sizing: border-box; padding: 4px;
      background: rgba(252,252,253,.97); color: #1d1d1f; border: .5px solid rgba(0,0,0,.14); border-radius: 10px;
      box-shadow: 0 10px 30px rgba(0,0,0,.18), 0 1px 3px rgba(0,0,0,.1); font: 13px/1.3 -apple-system, BlinkMacSystemFont, system-ui, sans-serif;
      animation: in .12s ease-out; transform-origin: top left }
    @keyframes in { from { opacity: 0; transform: scale(.97) } }
    @media (prefers-reduced-motion: reduce) { .menu { animation: none } }
    @media (prefers-color-scheme: dark) { .menu { background: rgba(40,40,44,.97); color: #ececf0; border-color: rgba(255,255,255,.14) } }
    .title { padding: 5px 8px 4px; font-size: 11px; opacity: .6 }
    .item { display: flex; align-items: center; gap: 8px; padding: 6px 8px; border-radius: 6px; cursor: default; user-select: none }
    .item.on { background: hsl(250 70% 55%); color: #fff }
    .ico { flex: none; width: 16px; height: 16px; opacity: .7 }
    .text { display: flex; flex-direction: column; min-width: 0 }
    .name, .detail { overflow: hidden; text-overflow: ellipsis; white-space: nowrap }
    .detail { font-size: 11px; opacity: .65 }
    .sep { height: .5px; margin: 4px 6px; background: currentColor; opacity: .15 }
    .manage { opacity: .85 }
  `
  const SVG_NS = 'http://www.w3.org/2000/svg'
  // Icons as [tag, attributes] lists, built with DOM calls (no innerHTML: Trusted Types pages refuse it).
  const ICONS = {
    address: [
      ['path', { d: 'M20 10c0 6-8 12-8 12s-8-6-8-12a8 8 0 0 1 16 0Z' }],
      ['circle', { cx: '12', cy: '10', r: '3' }]
    ],
    card: [
      ['rect', { width: '20', height: '14', x: '2', y: '5', rx: '2' }],
      ['path', { d: 'M2 10h20' }]
    ]
  }
  function icon(kind) {
    const svg = doc.createElementNS(SVG_NS, 'svg')
    const attrs = { class: 'ico', viewBox: '0 0 24 24', fill: 'none', stroke: 'currentColor', 'stroke-width': '2', 'stroke-linecap': 'round', 'stroke-linejoin': 'round' }
    for (const k of Object.keys(attrs)) svg.setAttribute(k, attrs[k])
    for (const [tag, a] of ICONS[kind]) {
      const part = doc.createElementNS(SVG_NS, tag)
      for (const k of Object.keys(a)) part.setAttribute(k, a[k])
      svg.append(part)
    }
    return svg
  }

  function highlight(i) {
    active = i
    items.forEach((el, j) => el.classList.toggle('on', j === i))
  }

  function hide() {
    token++
    if (host) host.remove()
    host = null
    shadow = null
    field = null
    menu = null
    rows = []
    active = -1
    items = []
  }

  function render() {
    if (!shadow || !field || !menu) return
    const r = field.getBoundingClientRect()
    const box = doc.createElement('div')
    box.className = 'menu'
    box.style.left = Math.max(4, Math.min(r.left, g.innerWidth - 224)) + 'px'
    box.style.top = (r.bottom + 4 + 200 > g.innerHeight && r.top > 200 ? Math.max(4, r.top - 4 - 44 * (rows.length + 2)) : r.bottom + 4) + 'px'
    box.style.width = Math.max(220, Math.min(360, r.width)) + 'px'
    const title = doc.createElement('div')
    title.className = 'title'
    title.textContent = menu.title
    box.append(title)
    const make = (i, cls, build) => {
      const item = doc.createElement('div')
      item.className = 'item ' + cls + (i === active ? ' on' : '')
      build(item)
      // The field keeps focus; only a real click fills.
      item.addEventListener('mousedown', (e) => e.preventDefault())
      item.addEventListener('mouseenter', () => highlight(i))
      item.addEventListener('click', (e) => {
        if (e.isTrusted && nowMs() - shownAt > 300) pick(i)
      })
      box.append(item)
      return item
    }
    const kind = menu.kind
    items = rows.map((row, i) =>
      make(i, '', (item) => {
        item.append(icon(kind))
        const text = doc.createElement('span')
        text.className = 'text'
        const name = doc.createElement('span')
        name.className = 'name'
        name.textContent = row.label
        text.append(name)
        if (row.detail) {
          const detail = doc.createElement('span')
          detail.className = 'detail'
          detail.textContent = row.detail
          text.append(detail)
        }
        item.append(text)
      })
    )
    const sep = doc.createElement('div')
    sep.className = 'sep'
    const last = items[items.length - 1]
    box.insertBefore(sep, (last && last.nextSibling) || null)
    items.push(
      make(rows.length, 'manage', (item) => {
        const name = doc.createElement('span')
        name.className = 'name'
        name.textContent = (menu && menu.manage) || ''
        item.append(name)
      })
    )
    const style = doc.createElement('style')
    style.textContent = CSS
    shadow.replaceChildren(style, box)
  }

  const isString = (v) => typeof v === 'string'
  function asMenu(v) {
    if (!v || typeof v !== 'object') return null
    if ((v.kind !== 'address' && v.kind !== 'card') || !isString(v.title) || !isString(v.manage) || !Array.isArray(v.items) || v.items.length > 20) return null
    const ok = v.items.every((i) => i && isString(i.id) && isString(i.label) && isString(i.detail) && isString(i.match))
    return ok ? { kind: v.kind, title: v.title, manage: v.manage, items: v.items.map((i) => ({ id: i.id, label: i.label, detail: i.detail, match: i.match })) } : null
  }

  const HOST_STYLE = {
    all: 'initial',
    display: 'block',
    opacity: '1',
    visibility: 'visible',
    filter: 'none',
    'clip-path': 'none',
    transform: 'none',
    'mix-blend-mode': 'normal',
    'pointer-events': 'auto',
    position: 'static'
  }

  function show(el, kind) {
    const mine = ++token
    bridge.request(
      'af:menu',
      { kind },
      (error, value) => {
        const reply = error === null ? asMenu(value) : null
        if (mine !== token || !reply || !reply.items.length || doc.activeElement !== el) return
        menu = reply
        field = el
        refresh()
        if (!rows.length) return hide()
        if (!host) {
          host = doc.createElement('signet-autofill')
          shadow = host.attachShadow({ mode: 'closed' })
          doc.documentElement.append(host)
        }
        // The page must not be able to hide the menu (clickjacking): inline styles take priority.
        for (const k of Object.keys(HOST_STYLE)) host.style.setProperty(k, HOST_STYLE[k], 'important')
        shownAt = nowMs()
        active = -1
        render()
      },
      5000
    )
  }

  /** Rows that fit what the user already typed in the field. */
  function refresh() {
    if (!menu || !field) return
    const typed = field.value.trim().toLowerCase()
    rows = typed ? menu.items.filter((i) => i.match.includes(typed)) : menu.items
    if (!rows.length) hide()
  }

  /** Offers the dropdown for a field the user touched, if it is a classified field of an address or card form. */
  function consider(el) {
    if (!isText(el)) return hide()
    if (el.value.trim() && !host) return
    const root = rootOf(el)
    const s = scan(root)
    const c = s.classes[s.fields.indexOf(el)]
    if (!c) return hide()
    if (isCardKind(c.kind)) {
      if (c.kind === 'cc-csc' || c.kind === 'cc-type' || !isCardForm(s.classes)) return hide()
      show(el, 'card')
    } else if (isAddressForm(s.classes)) show(el, 'address')
    else hide()
  }

  // ───────────── Filling ─────────────

  const setters = {
    select: Object.getOwnPropertyDescriptor(g.HTMLSelectElement.prototype, 'value').set,
    textarea: Object.getOwnPropertyDescriptor(g.HTMLTextAreaElement.prototype, 'value').set,
    input: Object.getOwnPropertyDescriptor(g.HTMLInputElement.prototype, 'value').set
  }
  function setValue(el, value) {
    const setter = el instanceof g.HTMLSelectElement ? setters.select : el instanceof g.HTMLTextAreaElement ? setters.textarea : setters.input
    setter.call(el, value)
    el.dispatchEvent(new g.Event('input', { bubbles: true }))
    el.dispatchEvent(new g.Event('change', { bubbles: true }))
  }

  const optionsOf = (sel) => [...sel.options].map((o) => ({ value: o.value, text: o.text }))

  /** Picks the option at `index` of a select; false if there is none. */
  function setOption(sel, index) {
    if (index < 0 || !sel.options[index]) return false
    setValue(sel, sel.options[index].value)
    return true
  }

  function fillAddress(target, a) {
    const s = scan(rootOf(target))
    const tc = s.classes[s.fields.indexOf(target)]
    if (!tc || isCardKind(tc.kind)) return
    const done = new Set()
    s.fields.forEach((f, j) => {
      const c = s.classes[j]
      if (!c || !isAddressKind(c.kind) || !sameGroup(tc, c) || done.has(c.kind)) return
      done.add(c.kind)
      // Only empty fields, plus the one the user is in.
      if (f !== target && !isEmpty(f)) return
      if (f instanceof g.HTMLSelectElement) {
        if (c.kind === 'country' || c.kind === 'country-name' || c.kind === 'region') setOption(f, pickOption(optionsOf(f), placeCandidates(c.kind, a, locales)))
        return
      }
      const value = addressValue(a, c.kind, { multiline: f instanceof g.HTMLTextAreaElement, locales })
      if (value) setValue(f, value)
    })
  }

  function fillCard(target, card) {
    const s = scan(rootOf(target))
    const tc = s.classes[s.fields.indexOf(target)]
    if (!tc || !isCardKind(tc.kind)) return
    const done = new Set()
    s.fields.forEach((f, j) => {
      const c = s.classes[j]
      // The security code is never filled: the user types it.
      if (!c || !isCardKind(c.kind) || c.kind === 'cc-csc' || !sameGroup(tc, c) || done.has(c.kind)) return
      done.add(c.kind)
      if (f !== target && !isEmpty(f)) return
      const hint = { maxLength: 'maxLength' in f && f.maxLength > 0 ? f.maxLength : undefined, placeholder: 'placeholder' in f ? f.placeholder : '' }
      const select = f instanceof g.HTMLSelectElement ? f : null
      switch (c.kind) {
        case 'cc-name':
          if (card.holder && !select) setValue(f, card.holder)
          break
        case 'cc-number': {
          if (select) break
          const grouped = /\d{4}\s\d{4}|[x#•]{4}\s/i.test(hint.placeholder) ? groupCardNumber(card.number, card.network) : card.number
          setValue(f, hint.maxLength && grouped.length > hint.maxLength ? card.number : grouped)
          break
        }
        case 'cc-exp':
          if (select) {
            const long = formatExpiry(card.expMonth, card.expYear, { maxLength: 7 })
            setOption(select, pickOption(optionsOf(select), [formatExpiry(card.expMonth, card.expYear), long, formatExpiry(card.expMonth, card.expYear, { maxLength: 4 })]))
          } else setValue(f, formatExpiry(card.expMonth, card.expYear, hint))
          break
        case 'cc-exp-month':
          if (select) setOption(select, pickMonthOption(optionsOf(select), card.expMonth))
          else setValue(f, String(card.expMonth).padStart(2, '0'))
          break
        case 'cc-exp-year':
          if (select) setOption(select, pickOption(optionsOf(select), yearCandidates(card.expYear)))
          else setValue(f, formatYear(card.expYear, hint))
          break
        case 'cc-type':
          if (select) setOption(select, pickOption(optionsOf(select), [NETWORK_NAMES[card.network], card.network].filter(Boolean)))
          break
      }
    })
  }

  const num = (v) => (typeof v === 'number' ? v : NaN)
  const ADDRESS_KEYS = ['givenName', 'familyName', 'organization', 'street', 'city', 'postalCode', 'region', 'country', 'phone', 'email']

  function pick(i) {
    const target = field
    const m = menu
    // Read before hide(), which empties `rows`.
    const count = rows.length
    const row = rows[i]
    hide()
    if (!target || !m) return
    if (i >= count) return bridge.send('af:manage', { kind: m.kind })
    if (!row) return
    bridge.request(
      'af:fill',
      { kind: m.kind, id: row.id },
      (error, reply) => {
        if (error !== null || !reply || typeof reply !== 'object' || !target.isConnected) return
        if (m.kind === 'address') {
          const a = reply.address
          if (a && typeof a === 'object') {
            const clean = {}
            for (const k of ADDRESS_KEYS) clean[k] = isString(a[k]) ? a[k] : ''
            fillAddress(target, clean)
          }
        } else if (isString(reply.number) && isString(reply.holder) && Number.isInteger(num(reply.expMonth)) && Number.isInteger(num(reply.expYear))) {
          fillCard(target, {
            network: isString(reply.network) ? reply.network : 'unknown',
            holder: reply.holder,
            number: reply.number,
            expMonth: num(reply.expMonth),
            expYear: num(reply.expYear)
          })
        }
      },
      30000
    )
  }

  doc.addEventListener(
    'focusin',
    (e) => {
      if (isText(e.target)) consider(e.target)
      else hide()
    },
    true
  )
  doc.addEventListener(
    'mousedown',
    (e) => {
      const el = e.target
      if (e.isTrusted && isText(el) && el === doc.activeElement && !host) consider(el)
    },
    true
  )
  doc.addEventListener(
    'focusout',
    (e) => {
      if (e.target === field) hide()
    },
    true
  )
  doc.addEventListener(
    'input',
    (e) => {
      if (!e.isTrusted || e.target !== doc.activeElement) return
      if (host && e.target === field) {
        refresh()
        if (host) render()
      } else if (!host) consider(e.target)
    },
    true
  )
  g.addEventListener('resize', hide)
  g.addEventListener('scroll', () => host && render(), true)

  // ───────────── Sending submitted values ─────────────

  let lastKey = ''
  let lastAt = 0

  /** Typed value of a field; a select gives its code when it looks like one, otherwise the text of the chosen option. */
  function valueOf(f) {
    if (f instanceof g.HTMLSelectElement) {
      if (f.selectedIndex <= 0 && !f.value) return ''
      const o = f.selectedOptions[0]
      return /^[A-Za-z]{2}$/.test(f.value) ? f.value : ((o && o.text) || '').trim()
    }
    return f.value.trim()
  }

  function collect(root) {
    const s = scan(root)
    const out = {}
    const first = (match) => {
      for (let j = 0; j < s.fields.length; j++) {
        const c = s.classes[j]
        if (c && match(c)) {
          const v = valueOf(s.fields[j])
          if (v) return v
        }
      }
      return ''
    }
    if (isAddressForm(s.classes)) {
      const lead = s.classes.find((c) => !!c && isAddressKind(c.kind) && c.kind !== 'email' && c.kind !== 'tel')
      if (lead) {
        const of = (...kinds) => first((c) => isAddressKind(c.kind) && kinds.includes(c.kind) && sameGroup(lead, c))
        const given = of('given-name')
        const family = of('family-name')
        const street = [of('street-address'), of('address-line1'), of('address-line2'), of('address-line3')].filter(Boolean).join('\n')
        out.address = {
          // Separate first and last name fields are stored as they are; a lone full-name field is split by the browser side.
          ...(given || family ? { givenName: given, familyName: family } : { name: of('name') }),
          organization: of('organization'),
          street,
          city: of('city'),
          postalCode: of('postal-code'),
          region: of('region'),
          country: of('country', 'country-name'),
          phone: of('tel'),
          email: of('email')
        }
      }
    }
    if (isCardForm(s.classes)) {
      // The security code is never read.
      const of = (kind) => first((c) => c.kind === kind)
      out.card = { holder: of('cc-name'), number: of('cc-number'), exp: of('cc-exp'), expMonth: of('cc-exp-month'), expYear: of('cc-exp-year') }
    }
    return out.address || out.card ? out : null
  }

  function report(from) {
    const payload = collect(rootOf(from))
    if (!payload) return
    // A cheap fingerprint is enough to drop the same submission seen twice (submit event + button click).
    const text = JSON.stringify(payload)
    let hash = text.length
    for (let i = 0; i < text.length; i++) hash = (hash * 31 + text.charCodeAt(i)) | 0
    const key = String(hash)
    if (key === lastKey && nowMs() - lastAt < 2000) return
    lastKey = key
    lastAt = nowMs()
    bridge.send('af:submit', payload)
  }

  doc.addEventListener(
    'submit',
    (e) => {
      // Submissions simulated by the page (requestSubmit in a loop) ignored.
      if (!e.isTrusted && !(g.navigator.userActivation && g.navigator.userActivation.isActive)) return
      if (e.target instanceof g.HTMLFormElement) report(e.target)
    },
    true
  )
  doc.addEventListener(
    'keydown',
    (e) => {
      // Navigating the list: real keys only (a page must not be able to pick an item with simulated keys).
      if (host && e.target === field && e.isTrusted) {
        const total = rows.length + 1
        if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
          e.preventDefault()
          return highlight((active + (e.key === 'ArrowDown' ? 1 : total + (active < 0 ? 0 : -1))) % total)
        }
        if (e.key === 'Enter' && active >= 0) {
          e.preventDefault()
          e.stopPropagation()
          return pick(active)
        }
        if (e.key === 'Escape') return hide()
      }
      if (e.key === 'Enter' && e.isTrusted && e.target instanceof g.HTMLInputElement) report(e.target)
    },
    true
  )
  doc.addEventListener(
    'click',
    (e) => {
      if (!e.isTrusted || !(e.target instanceof g.Element)) return
      const button = e.target.closest('button, input[type="submit"], input[type="image"], [role="button"]')
      if (!button) return
      if (button instanceof g.HTMLButtonElement && button.type !== 'submit' && button.form) return // simple button of a real form
      report(button)
    },
    true
  )
})

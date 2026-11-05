/// <reference lib="dom" />
// Page script: address and card autofill (main frame only, preload's isolated world).
// Shows Signet's saved addresses or cards under a field the user touches (never silent filling), fills the form on
// a real click, and reports submitted addresses and cards so main can offer to save them.
// Origin, settings, authentication and the https rule for cards are all decided by the main process.
import { ipcRenderer } from 'electron'
import {
  classifyFields,
  isAddressForm,
  isAddressKind,
  isCardForm,
  isCardKind,
  sameGroup,
  type FieldClass,
  type FieldDescriptor
} from '@signet/shared/autofill/classify'
import { formatExpiry, formatYear, groupCardNumber, NETWORK_NAMES } from '@signet/shared/autofill/card'
import { addressValue, pickMonthOption, pickOption, placeCandidates, yearCandidates } from '@signet/shared/autofill/values'
import type { AddressInput, CardNetwork } from '@shared/types'

// Main frame only (the preload also runs in subframes).
if (process.isMainFrame && /^https?:$/.test(location.protocol)) start()

type Field = HTMLInputElement | HTMLSelectElement | HTMLTextAreaElement
type Kind = 'address' | 'card'

interface Item {
  id: string
  label: string
  detail: string
  match: string
}

interface Menu {
  kind: Kind
  title: string
  manage: string
  items: Item[]
}

interface CardValues {
  network: CardNetwork
  holder: string
  number: string
  expMonth: number
  expYear: number
}

function start(): void {
  const MAX_FIELDS = 150
  const locales = [...navigator.languages]

  // ───────────── Field detection ─────────────

  const isField = (el: EventTarget | null): el is Field =>
    el instanceof HTMLInputElement || el instanceof HTMLSelectElement || el instanceof HTMLTextAreaElement
  const isText = (el: EventTarget | null): el is HTMLInputElement | HTMLTextAreaElement => el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement
  const visible = (el: HTMLElement) => el.getClientRects().length > 0 && getComputedStyle(el).visibility !== 'hidden'

  function labelOf(el: Field): string {
    const parts: string[] = []
    for (const l of el.labels ?? []) parts.push(l.textContent ?? '')
    for (const id of (el.getAttribute('aria-labelledby') ?? '').split(/\s+/).filter(Boolean)) parts.push(document.getElementById(id)?.textContent ?? '')
    return parts.join(' ').replace(/\s+/g, ' ').trim().slice(0, 120)
  }

  function describe(el: Field): FieldDescriptor {
    const type = el instanceof HTMLSelectElement ? 'select' : el instanceof HTMLTextAreaElement ? 'textarea' : el.type
    // Cheap attribute checks first: the layout test only runs for fields that could matter.
    const hidden = type === 'hidden' || el.disabled || !visible(el)
    return {
      type,
      autocomplete: el.getAttribute('autocomplete') ?? '',
      name: el.name,
      id: el.id,
      label: labelOf(el),
      placeholder: 'placeholder' in el ? el.placeholder : '',
      ariaLabel: el.getAttribute('aria-label') ?? '',
      hidden,
      disabled: el.disabled,
      readOnly: 'readOnly' in el && el.readOnly
    }
  }

  interface Scan {
    fields: Field[]
    classes: (FieldClass | null)[]
  }

  /** The fields of the form around `el` (or of the whole page when it has no form), classified. */
  function scan(root: ParentNode): Scan {
    const fields = [...root.querySelectorAll<Field>('input, select, textarea')].slice(0, MAX_FIELDS)
    return { fields, classes: classifyFields(fields.map(describe)) }
  }

  /** Scope of a scan: the element's form, or the whole page when it has none. */
  const rootOf = (el: Element): ParentNode => (el instanceof HTMLFormElement ? el : ((isField(el) || el instanceof HTMLButtonElement) && el.form) || document)

  const isEmpty = (f: Field) => (f instanceof HTMLSelectElement ? f.value === '' || (f.selectedIndex <= 0 && !f.options[0]?.value) : !f.value.trim())

  // ───────────── Dropdown ─────────────

  let host: HTMLElement | null = null
  let shadow: ShadowRoot | null = null
  let field: Field | null = null
  let menu: Menu | null = null
  let rows: Item[] = []
  let active = -1
  let items: HTMLElement[] = []
  let shownAt = 0
  let request = 0

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
  const ICONS: Record<Kind, string> = {
    address:
      '<svg class="ico" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M20 10c0 6-8 12-8 12s-8-6-8-12a8 8 0 0 1 16 0Z"/><circle cx="12" cy="10" r="3"/></svg>',
    card: '<svg class="ico" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect width="20" height="14" x="2" y="5" rx="2"/><path d="M2 10h20"/></svg>'
  }

  function highlight(i: number): void {
    active = i
    items.forEach((el, j) => el.classList.toggle('on', j === i))
  }

  function hide(): void {
    request++
    host?.remove()
    host = null
    shadow = null
    field = null
    menu = null
    rows = []
    active = -1
    items = []
  }

  function render(): void {
    if (!shadow || !field || !menu) return
    const r = field.getBoundingClientRect()
    const box = document.createElement('div')
    box.className = 'menu'
    box.style.left = `${Math.max(4, Math.min(r.left, innerWidth - 224))}px`
    box.style.top = `${r.bottom + 4 + 200 > innerHeight && r.top > 200 ? Math.max(4, r.top - 4 - 44 * (rows.length + 2)) : r.bottom + 4}px`
    box.style.width = `${Math.max(220, Math.min(360, r.width))}px`
    const title = document.createElement('div')
    title.className = 'title'
    title.textContent = menu.title
    box.append(title)
    const make = (i: number, cls: string, build: (el: HTMLElement) => void) => {
      const item = document.createElement('div')
      item.className = `item ${cls}${i === active ? ' on' : ''}`
      build(item)
      // The field keeps focus; only a real click fills.
      item.addEventListener('mousedown', (e) => e.preventDefault())
      item.addEventListener('mouseenter', () => highlight(i))
      item.addEventListener('click', (e) => {
        if (e.isTrusted && Date.now() - shownAt > 300) void pick(i)
      })
      box.append(item)
      return item
    }
    const kind = menu.kind
    items = rows.map((row, i) =>
      make(i, '', (item) => {
        item.innerHTML = ICONS[kind]
        const text = document.createElement('span')
        text.className = 'text'
        const name = document.createElement('span')
        name.className = 'name'
        name.textContent = row.label
        text.append(name)
        if (row.detail) {
          const detail = document.createElement('span')
          detail.className = 'detail'
          detail.textContent = row.detail
          text.append(detail)
        }
        item.append(text)
      })
    )
    const sep = document.createElement('div')
    sep.className = 'sep'
    box.insertBefore(sep, items[items.length - 1]?.nextSibling ?? null)
    items.push(
      make(rows.length, 'manage', (item) => {
        const name = document.createElement('span')
        name.className = 'name'
        name.textContent = menu?.manage ?? ''
        item.append(name)
      })
    )
    shadow.replaceChildren(Object.assign(document.createElement('style'), { textContent: CSS }), box)
  }

  const isString = (v: unknown): v is string => typeof v === 'string'
  function asMenu(v: unknown): Menu | null {
    if (!v || typeof v !== 'object') return null
    const m = v as Partial<Menu>
    if ((m.kind !== 'address' && m.kind !== 'card') || !isString(m.title) || !isString(m.manage) || !Array.isArray(m.items) || m.items.length > 20) return null
    const ok = m.items.every((i) => i && isString(i.id) && isString(i.label) && isString(i.detail) && isString(i.match))
    return ok ? (m as Menu) : null
  }

  async function show(el: Field, kind: Kind): Promise<void> {
    const token = ++request
    let reply: Menu | null
    try {
      reply = asMenu(await ipcRenderer.invoke('af:menu', kind))
    } catch {
      return
    }
    if (token !== request || !reply || !reply.items.length || document.activeElement !== el) return
    menu = reply
    field = el
    refresh()
    if (!rows.length) return hide()
    if (!host) {
      host = document.createElement('signet-autofill')
      shadow = host.attachShadow({ mode: 'closed' })
      document.documentElement.append(host)
    }
    // The page must not be able to hide the menu (clickjacking): inline styles take priority.
    for (const [k, v] of Object.entries({
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
    }))
      host.style.setProperty(k, v, 'important')
    shownAt = Date.now()
    active = -1
    render()
  }

  /** Rows that fit what the user already typed in the field. */
  function refresh(): void {
    if (!menu || !field) return
    const typed = field.value.trim().toLowerCase()
    rows = typed ? menu.items.filter((i) => i.match.includes(typed)) : menu.items
    if (!rows.length) hide()
  }

  /** Offers the dropdown for a field the user touched, if it is a classified field of an address or card form. */
  function consider(el: EventTarget | null): void {
    if (!isText(el)) return hide()
    if (el.value.trim() && !host) return
    const root = rootOf(el)
    const s = scan(root)
    const c = s.classes[s.fields.indexOf(el)]
    if (!c) return hide()
    if (isCardKind(c.kind)) {
      if (c.kind === 'cc-csc' || c.kind === 'cc-type' || !isCardForm(s.classes)) return hide()
      void show(el, 'card')
    } else if (isAddressForm(s.classes)) void show(el, 'address')
    else hide()
  }

  // ───────────── Filling ─────────────

  function setValue(el: Field, value: string): void {
    const proto =
      el instanceof HTMLSelectElement
        ? HTMLSelectElement.prototype
        : el instanceof HTMLTextAreaElement
          ? HTMLTextAreaElement.prototype
          : HTMLInputElement.prototype
    Object.getOwnPropertyDescriptor(proto, 'value')?.set?.call(el, value)
    el.dispatchEvent(new Event('input', { bubbles: true }))
    el.dispatchEvent(new Event('change', { bubbles: true }))
  }

  const optionsOf = (sel: HTMLSelectElement) => [...sel.options].map((o) => ({ value: o.value, text: o.text }))

  /** Picks the option at `index` of a select; false if there is none. */
  function setOption(sel: HTMLSelectElement, index: number): boolean {
    if (index < 0 || !sel.options[index]) return false
    setValue(sel, sel.options[index].value)
    return true
  }

  function fillAddress(target: Field, a: AddressInput): void {
    const s = scan(rootOf(target))
    const tc = s.classes[s.fields.indexOf(target)]
    if (!tc || isCardKind(tc.kind)) return
    const done = new Set<string>()
    s.fields.forEach((f, j) => {
      const c = s.classes[j]
      if (!c || !isAddressKind(c.kind) || !sameGroup(tc, c) || done.has(c.kind)) return
      done.add(c.kind)
      // Only empty fields, plus the one the user is in.
      if (f !== target && !isEmpty(f)) return
      if (f instanceof HTMLSelectElement) {
        if (c.kind === 'country' || c.kind === 'country-name' || c.kind === 'region')
          setOption(f, pickOption(optionsOf(f), placeCandidates(c.kind, a, locales)))
        return
      }
      const value = addressValue(a, c.kind, { multiline: f instanceof HTMLTextAreaElement, locales })
      if (value) setValue(f, value)
    })
  }

  function fillCard(target: Field, card: CardValues): void {
    const s = scan(rootOf(target))
    const tc = s.classes[s.fields.indexOf(target)]
    if (!tc || !isCardKind(tc.kind)) return
    const done = new Set<string>()
    s.fields.forEach((f, j) => {
      const c = s.classes[j]
      // The security code is never filled: the user types it.
      if (!c || !isCardKind(c.kind) || c.kind === 'cc-csc' || !sameGroup(tc, c) || done.has(c.kind)) return
      done.add(c.kind)
      if (f !== target && !isEmpty(f)) return
      const hint = { maxLength: 'maxLength' in f && f.maxLength > 0 ? f.maxLength : undefined, placeholder: 'placeholder' in f ? f.placeholder : '' }
      const select = f instanceof HTMLSelectElement ? f : null
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
            setOption(
              select,
              pickOption(optionsOf(select), [formatExpiry(card.expMonth, card.expYear), long, formatExpiry(card.expMonth, card.expYear, { maxLength: 4 })])
            )
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

  const num = (v: unknown): number => (typeof v === 'number' ? v : NaN)

  async function pick(i: number): Promise<void> {
    const target = field
    const m = menu
    // Read before hide(), which empties `rows`.
    const count = rows.length
    const row = rows[i]
    hide()
    if (!target || !m) return
    if (i >= count) return ipcRenderer.send('af:manage', m.kind)
    if (!row) return
    let reply: unknown
    try {
      reply = await ipcRenderer.invoke('af:fill', m.kind, row.id)
    } catch {
      return
    }
    if (!reply || typeof reply !== 'object' || !target.isConnected) return
    if (m.kind === 'address') {
      const a = (reply as { address?: Record<string, unknown> }).address
      if (a && typeof a === 'object')
        fillAddress(target, Object.fromEntries(Object.entries(a).map(([k, v]) => [k, isString(v) ? v : ''])) as unknown as AddressInput)
    } else {
      const c = reply as Partial<CardValues>
      if (isString(c.number) && isString(c.holder) && Number.isInteger(num(c.expMonth)) && Number.isInteger(num(c.expYear)))
        fillCard(target, {
          network: (c.network as CardNetwork) ?? 'unknown',
          holder: c.holder,
          number: c.number,
          expMonth: num(c.expMonth),
          expYear: num(c.expYear)
        })
    }
  }

  document.addEventListener(
    'focusin',
    (e) => {
      if (isText(e.target)) consider(e.target)
      else hide()
    },
    true
  )
  document.addEventListener(
    'mousedown',
    (e) => {
      const el = e.target
      if (e.isTrusted && isText(el) && el === document.activeElement && !host) consider(el)
    },
    true
  )
  document.addEventListener(
    'focusout',
    (e) => {
      if (e.target === field) hide()
    },
    true
  )
  document.addEventListener(
    'input',
    (e) => {
      if (!e.isTrusted || e.target !== document.activeElement) return
      if (host && e.target === field) {
        refresh()
        if (host) render()
      } else if (!host) consider(e.target)
    },
    true
  )
  addEventListener('resize', hide)
  addEventListener('scroll', () => host && render(), true)

  // ───────────── Sending submitted values ─────────────

  let lastKey = ''
  let lastAt = 0

  /** Typed value of a field; a select gives its code when it looks like one, otherwise the text of the chosen option. */
  function valueOf(f: Field): string {
    if (f instanceof HTMLSelectElement) {
      if (f.selectedIndex <= 0 && !f.value) return ''
      const o = f.selectedOptions[0]
      return /^[A-Za-z]{2}$/.test(f.value) ? f.value : (o?.text ?? '').trim()
    }
    return f.value.trim()
  }

  function collect(root: ParentNode): { address?: Record<string, string>; card?: Record<string, string> } | null {
    const s = scan(root)
    const out: { address?: Record<string, string>; card?: Record<string, string> } = {}
    const first = (match: (c: FieldClass) => boolean): string => {
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
      const lead = s.classes.find((c): c is FieldClass => !!c && isAddressKind(c.kind) && c.kind !== 'email' && c.kind !== 'tel')
      if (lead) {
        const of = (...kinds: string[]) => first((c) => isAddressKind(c.kind) && kinds.includes(c.kind) && sameGroup(lead, c))
        const given = of('given-name')
        const family = of('family-name')
        const street = [of('street-address'), of('address-line1'), of('address-line2'), of('address-line3')].filter(Boolean).join('\n')
        out.address = {
          // Separate first and last name fields are stored as they are; a lone full-name field is split in main.
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
      const of = (kind: string) => first((c) => c.kind === kind)
      out.card = { holder: of('cc-name'), number: of('cc-number'), exp: of('cc-exp'), expMonth: of('cc-exp-month'), expYear: of('cc-exp-year') }
    }
    return out.address || out.card ? out : null
  }

  function report(from: Element): void {
    const payload = collect(rootOf(from))
    if (!payload) return
    // A cheap fingerprint is enough to drop the same submission seen twice (submit event + button click).
    const text = JSON.stringify(payload)
    let hash = text.length
    for (let i = 0; i < text.length; i++) hash = (hash * 31 + text.charCodeAt(i)) | 0
    const key = String(hash)
    if (key === lastKey && Date.now() - lastAt < 2000) return
    lastKey = key
    lastAt = Date.now()
    ipcRenderer.send('af:submit', payload)
  }

  document.addEventListener(
    'submit',
    (e) => {
      // Submissions simulated by the page (requestSubmit in a loop) ignored.
      if (!e.isTrusted && !navigator.userActivation?.isActive) return
      if (e.target instanceof HTMLFormElement) report(e.target)
    },
    true
  )
  document.addEventListener(
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
          return void pick(active)
        }
        if (e.key === 'Escape') return hide()
      }
      if (e.key === 'Enter' && e.isTrusted && e.target instanceof HTMLInputElement) report(e.target)
    },
    true
  )
  document.addEventListener(
    'click',
    (e) => {
      if (!e.isTrusted || !(e.target instanceof Element)) return
      const button = e.target.closest('button, input[type="submit"], input[type="image"], [role="button"]')
      if (!button) return
      if (button instanceof HTMLButtonElement && button.type !== 'submit' && button.form) return // simple button of a real form
      report(button)
    },
    true
  )
}

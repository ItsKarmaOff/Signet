/// <reference lib="dom" />
/// <reference lib="dom.iterable" />
// Page script of autofill on mobile (DOM code, runs in the page, main frame only). It is NOT part of the app bundle: the script
// `app/mobile/scripts/autofill-page.mjs` bundles it into `../pageScript.generated.ts` (a string), and only that string is imported.
// It detects address and card fields (shared classifier), tells the app when one gets focus (the native bar above the keyboard
// offers the saved items), fills the form when the app asks, and reports submitted addresses and cards so the app can offer to save them.
// Settings, origin, Face ID and the https rule for cards are all decided by the app. The security code is never read nor filled.
// `SIGNET_TOKEN` is replaced by the app (see `../pageScript.ts`): it authenticates our messages, since any frame of the page can
// call `ReactNativeWebView.postMessage`; it lives in this closure and in the name of the fill entry point, never as a plain property.
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
import type { AddressInput, CardNetwork } from '@signet/shared/types'

declare const SIGNET_TOKEN: string

type Field = HTMLInputElement | HTMLSelectElement | HTMLTextAreaElement
type Kind = 'address' | 'card'

interface CardValues {
  network: CardNetwork
  holder: string
  number: string
  expMonth: number
  expYear: number
}

interface FillRequest {
  /** Origin the app thinks the page is on. */
  o: string
  kind: Kind
  address?: AddressInput
  card?: CardValues
}

const ENTRY = `__signetAf_${SIGNET_TOKEN}`
const MAX_FIELDS = 150

function start(): void {
  const locales = [...navigator.languages]

  const post = (msg: Record<string, unknown>): void => {
    try {
      ;(window as unknown as { ReactNativeWebView: { postMessage: (s: string) => void } }).ReactNativeWebView.postMessage(
        JSON.stringify({ ...msg, k: SIGNET_TOKEN })
      )
    } catch {
      /* no bridge */
    }
  }

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

  /** The fields of the form around an element (or of the whole page when it has no form), classified. */
  function scan(root: ParentNode): Scan {
    const fields = [...root.querySelectorAll<Field>('input, select, textarea')].slice(0, MAX_FIELDS)
    return { fields, classes: classifyFields(fields.map(describe)) }
  }

  const rootOf = (el: Element): ParentNode => (el instanceof HTMLFormElement ? el : ((isField(el) || el instanceof HTMLButtonElement) && el.form) || document)

  const isEmpty = (f: Field) => (f instanceof HTMLSelectElement ? f.value === '' || (f.selectedIndex <= 0 && !f.options[0]?.value) : !f.value.trim())

  // ───────────── Focus tracking ─────────────

  /** Last field where the app was told to offer something; the fill goes to its group. */
  let target: Field | null = null
  let targetKind: Kind | null = null
  let shown = false
  let blurTimer = 0

  /** The kind of fill a text field gets, or null (same rules as the desktop dropdown). */
  function kindOf(el: HTMLInputElement | HTMLTextAreaElement): Kind | null {
    if (el instanceof HTMLInputElement && !['text', 'email', 'tel', 'search', 'number', ''].includes(el.type)) return null
    if (el.readOnly || el.disabled) return null
    const s = scan(rootOf(el))
    const c = s.classes[s.fields.indexOf(el)]
    if (!c) return null
    if (isCardKind(c.kind)) return c.kind === 'cc-csc' || c.kind === 'cc-type' || !isCardForm(s.classes) ? null : 'card'
    return isAddressForm(s.classes) ? 'address' : null
  }

  function hideBar(): void {
    clearTimeout(blurTimer)
    blurTimer = 0
    if (!shown) return
    shown = false
    post({ type: 'af:blur' })
  }

  /** Tells the app a fillable field has focus (only empty fields: a field the user is typing in gets no offer). */
  function consider(el: EventTarget | null): void {
    if (!isText(el) || el.value.trim()) return hideBar()
    const kind = kindOf(el)
    if (!kind) return hideBar()
    clearTimeout(blurTimer)
    blurTimer = 0
    target = el
    targetKind = kind
    shown = true
    post({ type: 'af:focus', kind, origin: location.origin })
  }

  document.addEventListener('focusin', (e) => consider(e.target), true)
  // Focus leaving: a short delay, so moving from one field to the next does not flicker the bar.
  document.addEventListener(
    'focusout',
    () => {
      if (!shown) return
      clearTimeout(blurTimer)
      blurTimer = window.setTimeout(hideBar, 150)
    },
    true
  )
  // Tap in the field that already has focus (keyboard dismissed then back): the app may have hidden the bar meanwhile.
  document.addEventListener(
    'click',
    (e) => {
      if (e.isTrusted && isText(e.target) && e.target === document.activeElement) consider(e.target)
    },
    true
  )
  // Typing: the bar steps aside once the field has something in it, and comes back if it is emptied.
  document.addEventListener(
    'input',
    (e) => {
      if (!e.isTrusted || e.target !== document.activeElement || !isText(e.target)) return
      if (e.target.value.trim()) hideBar()
      else if (!shown) consider(e.target)
    },
    true
  )

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

  /** Fills the address group of `el`; returns how many fields were set. */
  function fillAddress(el: Field, a: AddressInput): number {
    const s = scan(rootOf(el))
    const tc = s.classes[s.fields.indexOf(el)]
    if (!tc || isCardKind(tc.kind)) return 0
    const done = new Set<string>()
    let count = 0
    s.fields.forEach((f, j) => {
      const c = s.classes[j]
      if (!c || !isAddressKind(c.kind) || !sameGroup(tc, c) || done.has(c.kind)) return
      done.add(c.kind)
      // Only empty fields, plus the one the user is in.
      if (f !== el && !isEmpty(f)) return
      if (f instanceof HTMLSelectElement) {
        if (
          (c.kind === 'country' || c.kind === 'country-name' || c.kind === 'region') &&
          setOption(f, pickOption(optionsOf(f), placeCandidates(c.kind, a, locales)))
        )
          count++
        return
      }
      const value = addressValue(a, c.kind, { multiline: f instanceof HTMLTextAreaElement, locales })
      if (value) {
        setValue(f, value)
        count++
      }
    })
    return count
  }

  /** Fills the card group of `el` (never the security code); returns how many fields were set. */
  function fillCard(el: Field, card: CardValues): number {
    const s = scan(rootOf(el))
    const tc = s.classes[s.fields.indexOf(el)]
    if (!tc || !isCardKind(tc.kind)) return 0
    const done = new Set<string>()
    let count = 0
    const put = (f: Field, value: string) => {
      setValue(f, value)
      count++
    }
    const pick = (sel: HTMLSelectElement, index: number) => {
      if (setOption(sel, index)) count++
    }
    s.fields.forEach((f, j) => {
      const c = s.classes[j]
      if (!c || !isCardKind(c.kind) || c.kind === 'cc-csc' || !sameGroup(tc, c) || done.has(c.kind)) return
      done.add(c.kind)
      if (f !== el && !isEmpty(f)) return
      const hint = { maxLength: 'maxLength' in f && f.maxLength > 0 ? f.maxLength : undefined, placeholder: 'placeholder' in f ? f.placeholder : '' }
      const select = f instanceof HTMLSelectElement ? f : null
      switch (c.kind) {
        case 'cc-name':
          if (card.holder && !select) put(f, card.holder)
          break
        case 'cc-number': {
          if (select) break
          const grouped = /\d{4}\s\d{4}|[x#•]{4}\s/i.test(hint.placeholder) ? groupCardNumber(card.number, card.network) : card.number
          put(f, hint.maxLength && grouped.length > hint.maxLength ? card.number : grouped)
          break
        }
        case 'cc-exp':
          if (select) {
            const long = formatExpiry(card.expMonth, card.expYear, { maxLength: 7 })
            pick(
              select,
              pickOption(optionsOf(select), [formatExpiry(card.expMonth, card.expYear), long, formatExpiry(card.expMonth, card.expYear, { maxLength: 4 })])
            )
          } else put(f, formatExpiry(card.expMonth, card.expYear, hint))
          break
        case 'cc-exp-month':
          if (select) pick(select, pickMonthOption(optionsOf(select), card.expMonth))
          else put(f, String(card.expMonth).padStart(2, '0'))
          break
        case 'cc-exp-year':
          if (select) pick(select, pickOption(optionsOf(select), yearCandidates(card.expYear)))
          else put(f, formatYear(card.expYear, hint))
          break
        case 'cc-type':
          if (select) pick(select, pickOption(optionsOf(select), [NETWORK_NAMES[card.network], card.network].filter(Boolean)))
          break
      }
    })
    return count
  }

  const str = (v: unknown): string => (typeof v === 'string' ? v : '')

  /** Fill entry point: only the app can reach it (injected JavaScript, with the token in the name). */
  function fill(json: string): void {
    let ok = false
    try {
      const req = JSON.parse(json) as Partial<FillRequest>
      const el = target
      // The page must still be on the origin the app checked, and the field still there and of the same kind.
      if (req && req.o === location.origin && el && el.isConnected && targetKind === req.kind) {
        if (req.kind === 'address' && req.address && typeof req.address === 'object') {
          const a = req.address as unknown as Record<string, unknown>
          const typed = Object.fromEntries(Object.keys(a).map((k) => [k, str(a[k])])) as unknown as AddressInput
          ok = fillAddress(el, typed) > 0
        } else if (req.kind === 'card' && req.card && typeof req.card === 'object') {
          const c = req.card
          if (typeof c.number === 'string' && /^\d{8,19}$/.test(c.number) && Number.isInteger(c.expMonth) && Number.isInteger(c.expYear))
            ok = fillCard(el, { network: c.network ?? 'unknown', holder: str(c.holder), number: c.number, expMonth: c.expMonth, expYear: c.expYear }) > 0
        }
      }
    } catch {
      ok = false
    }
    hideBar()
    post({ type: 'af:filled', ok })
  }

  Object.defineProperty(window, ENTRY, { value: fill, enumerable: false, configurable: false, writable: false })

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
    post({ type: 'af:submit', origin: location.origin, ...payload })
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

// Main frame only, http(s) only, and only once per page (the entry point's name marks the injection).
if (window.top === window && /^https?:$/.test(location.protocol) && !(ENTRY in window)) start()

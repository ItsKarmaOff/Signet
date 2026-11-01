// Signet autofill: saved addresses and payment cards, offered under the field being filled, and a save offer
// after a form is submitted. Cards are encrypted with safeStorage (system keychain) and need the user's
// authentication before they leave main; the CVC is never stored.
import { app, dialog, ipcMain, safeStorage, type WebContents } from 'electron'
import { copyFileSync, readFileSync } from 'node:fs'
import { randomUUID } from 'node:crypto'
import { join } from 'node:path'
import type { AddressInput, AutofillAddress, AutofillCard, AutofillInfo, CardInput, CardNetwork } from '@shared/types'
import { fullName, splitName } from '@shared/autofill/values'
import { cardDigits, detectNetwork, formatExpiry, fullYear, luhn, NETWORK_NAMES } from '@shared/autofill/card'
import { isPlain } from '@shared/ipc/validate'
import type { Browser } from '../../browser/browser'
import { AtomicWriter } from '../../core/atomic'
import { t } from '../../app/i18n'
import { verifyUser, type AuthResult } from '../../app/osauth'
import { siteOf } from '../../core/urls'
import {
  byUsage,
  canFill,
  cleanAddress,
  isEmptyAddress,
  mayOffer,
  parseCardSubmission,
  planAddress,
  planCard,
  sameAddress,
  type AutofillKind,
  type CardCandidate
} from './rules'

/** Card as written on disk: the number is encrypted (base64). */
interface StoredCard extends AutofillCard {
  number: string
}

/** What the page script may ask for. */
interface PageEvent {
  sender: Pick<WebContents, 'id'> & { mainFrame?: { processId: number; routingId: number } }
  senderFrame?: { origin?: string; processId: number; routingId: number } | null
}

/** Address read from disk: files written before the name was split carry a single `name`, cut into first and last name. */
export function migrateAddress(raw: AutofillAddress): AutofillAddress {
  const { name, ...rest } = raw as AutofillAddress & { name?: unknown }
  const str = (v: unknown) => (typeof v === 'string' ? v : '')
  const givenName = str(rest.givenName)
  const familyName = str(rest.familyName)
  if (givenName || familyName || typeof name !== 'string') return { ...rest, givenName, familyName }
  const split = splitName(name)
  return { ...rest, givenName: split.given, familyName: split.family }
}

/** Dropdown shown in a page: only the frame that opened it may pick from it. */
interface MenuState {
  kind: AutofillKind
  tabId: string
  origin: string
  ids: Set<string>
  at: number
}

/** Offer to save awaiting an answer (in memory only; the card number never goes to the interface). */
interface Pending {
  tabId: string
  origin: string
  kind: AutofillKind
  summary: string
  update: boolean
  existingId?: string
  address?: AddressInput
  card?: CardCandidate
}

export interface AutofillDeps {
  verify?: (reason: string) => Promise<AuthResult>
  /** Weaker fallback when the system cannot authenticate: simple confirmation. */
  confirm?: (reason: string) => Promise<boolean>
}

/** What the page script receives to build its dropdown. */
export interface MenuReply {
  kind: AutofillKind
  title: string
  manage: string
  items: { id: string; label: string; detail: string; match: string }[]
}

export interface AddressFill {
  address: AddressInput
}

export interface CardFill {
  network: CardNetwork
  holder: string
  number: string
  expMonth: number
  expYear: number
}

const MENU_TTL = 10 * 60_000
const MENU_MAX = 8
const MAX_NICK = 40

const isKind = (v: unknown): v is AutofillKind => v === 'address' || v === 'card'

export class Autofill {
  private addresses: AutofillAddress[] = []
  private cards: StoredCard[] = []
  private loaded = false
  /** Unreadable file: the existing file is not overwritten during the session. */
  private blocked = false
  private file = join(app.getPath('userData'), 'autofill.json')
  private writer = new AtomicWriter(this.file, () =>
    this.loaded && !this.blocked ? JSON.stringify({ version: 1, addresses: this.addresses, cards: this.cards }) : null
  )
  private menus = new Map<string, MenuState>()
  private pending = new Map<string, Pending>()
  private lastOffer = new Map<string, number>()
  private overlayToken: string | null = null
  private authBusy = false

  constructor(
    private b: Browser,
    private deps: AutofillDeps = {}
  ) {}

  // ───────────────────────── Storage ─────────────────────────

  private load(): void {
    if (this.loaded) return
    this.loaded = true
    try {
      const raw = JSON.parse(readFileSync(this.file, 'utf8')) as { addresses?: AutofillAddress[]; cards?: StoredCard[] }
      this.addresses = (raw.addresses ?? []).filter((a) => a && typeof a.id === 'string').map(migrateAddress)
      this.cards = (raw.cards ?? []).filter((c) => c && typeof c.id === 'string' && typeof c.number === 'string' && typeof c.last4 === 'string')
    } catch (err) {
      const code = (err as NodeJS.ErrnoException).code
      if (code === 'ENOENT') return // first launch
      if (err instanceof SyntaxError) {
        // Corrupted file: separate copy before the next save overwrites it.
        try {
          copyFileSync(this.file, `${this.file}.corrupt-${Date.now()}`)
        } catch {
          this.blocked = true
        }
      } else this.blocked = true
    }
  }

  private save(): void {
    this.writer.request()
  }

  flush(): void {
    if (this.loaded) this.writer.flushSync()
  }

  get encryption(): boolean {
    try {
      return safeStorage.isEncryptionAvailable()
    } catch {
      return false
    }
  }

  private encrypt(number: string): string {
    return safeStorage.encryptString(number).toString('base64')
  }

  private decrypt(c: StoredCard): string | null {
    try {
      return safeStorage.decryptString(Buffer.from(c.number, 'base64'))
    } catch {
      return null
    }
  }

  private get settings() {
    return this.b.store.state.settings
  }

  private enabled(kind: AutofillKind): boolean {
    return (kind === 'address' ? this.settings.autofillAddresses : this.settings.autofillCards) !== false
  }

  // ───────────────────────── Authentication ─────────────────────────

  /** System authentication before a card leaves main. Where the system cannot ask (Linux), a plain confirmation. */
  async authenticate(reason: string): Promise<boolean> {
    const res = await (this.deps.verify ?? verifyUser)(reason)
    if (res === 'denied') return false
    if (res === 'unavailable') return (this.deps.confirm ?? confirmDialog)(reason)
    return true
  }

  // ───────────────────────── Settings ─────────────────────────

  list(): AutofillInfo {
    this.load()
    return {
      addresses: [...this.addresses].sort((a, b) => (fullName(a) || a.street).localeCompare(fullName(b) || b.street)),
      cards: this.cards.map(({ number: _number, ...rest }) => rest).sort((a, b) => a.createdAt - b.createdAt),
      encryption: this.encryption
    }
  }

  /** Adds (null id) or edits an address; returns a translated error message, or null. */
  upsertAddress(id: string | null, input: Partial<Record<keyof AddressInput | 'name', unknown>>): string | null {
    this.load()
    const typed = cleanAddress(input)
    if (isEmptyAddress(typed)) return t('autofill.error.empty')
    const existing = id ? this.addresses.find((a) => a.id === id) : undefined
    if (id && !existing) return t('autofill.error.missing')
    if (this.addresses.some((a) => a !== existing && sameAddress(a, typed))) return t('autofill.error.duplicateAddress')
    const now = Date.now()
    if (existing) Object.assign(existing, typed, { updatedAt: now })
    else this.addresses.push({ id: randomUUID(), ...typed, createdAt: now, updatedAt: now, useCount: 0 })
    this.save()
    return null
  }

  removeAddress(id: string): void {
    this.load()
    const before = this.addresses.length
    this.addresses = this.addresses.filter((a) => a.id !== id)
    if (this.addresses.length !== before) this.save()
  }

  /** Adds (null id) or edits a card; a new number for an existing card needs the user's authentication. */
  async upsertCard(id: string | null, input: CardInput): Promise<string | null> {
    this.load()
    if (!this.encryption) return t('autofill.error.encryption')
    const existing = id ? this.cards.find((c) => c.id === id) : undefined
    if (id && !existing) return t('autofill.error.missing')
    const expMonth = Number(input.expMonth)
    const expYear = fullYear(Number(input.expYear))
    if (!Number.isInteger(expMonth) || expMonth < 1 || expMonth > 12 || expYear === null) return t('autofill.error.expiry')
    const holder = String(input.holder ?? '')
      .trim()
      .slice(0, 200)
    const nickname = String(input.nickname ?? '')
      .trim()
      .slice(0, MAX_NICK)
    const typed = typeof input.number === 'string' && input.number.trim() ? input.number : null
    let digits: string | null = null
    if (typed !== null) {
      digits = cardDigits(typed)
      if (!digits || !luhn(digits)) return t('autofill.error.number')
      if (this.cards.some((c) => c !== existing && this.decrypt(c) === digits)) return t('autofill.error.duplicateCard')
      if (existing && !(await this.authenticate(t('autofill.authEdit')))) return t('autofill.error.auth')
    } else if (!existing) return t('autofill.error.number')
    const now = Date.now()
    if (existing) {
      Object.assign(existing, { holder, nickname, expMonth, expYear, updatedAt: now })
      if (digits) Object.assign(existing, { number: this.encrypt(digits), last4: digits.slice(-4), network: detectNetwork(digits) })
    } else {
      const d = digits as string
      this.cards.push({
        id: randomUUID(),
        holder,
        nickname,
        expMonth,
        expYear,
        number: this.encrypt(d),
        last4: d.slice(-4),
        network: detectNetwork(d),
        createdAt: now,
        updatedAt: now,
        useCount: 0
      })
    }
    this.save()
    return null
  }

  removeCard(id: string): void {
    this.load()
    const before = this.cards.length
    this.cards = this.cards.filter((c) => c.id !== id)
    if (this.cards.length !== before) this.save()
  }

  /** Full card number for Settings, after the user's authentication. */
  async revealCard(id: string): Promise<string | null> {
    this.load()
    const card = this.cards.find((c) => c.id === id)
    if (!card || !(await this.authenticate(t('autofill.authReveal')))) return null
    return this.decrypt(card)
  }

  // ───────────────────────── Pages ─────────────────────────

  /**
   * Channels of the page script (src/preload/autofill.ts). The origin comes from the sending frame as seen by main,
   * never from the page; only the main frame of a tab is served.
   */
  registerPageIpc(): void {
    ipcMain.handle('af:menu', (e, kind: unknown) => this.menu(e, kind))
    ipcMain.handle('af:fill', (e, kind: unknown, id: unknown) => this.fill(e, kind, id))
    ipcMain.on('af:manage', (e, kind: unknown) => this.manage(e, kind))
    ipcMain.on('af:submit', (e, payload: unknown) => this.submitted(e, payload))
  }

  /** Context of a page request: the tab, the frame's http(s) origin and the key identifying the frame; null if it must be refused. */
  private context(e: PageEvent) {
    const tabId = this.b.views.tabIdOfContents(e.sender.id)
    const frame = e.senderFrame
    let main: { processId: number; routingId: number } | undefined
    try {
      main = e.sender.mainFrame
    } catch {
      return null
    }
    if (!tabId || !frame || !main || frame.processId !== main.processId || frame.routingId !== main.routingId) return null
    const origin = frame.origin
    if (!origin || !/^https?:\/\/[^/]+$/i.test(origin)) return null
    return { tabId, origin, key: `${e.sender.id}:${frame.processId}:${frame.routingId}`, isPrivate: this.b.privates.owns(tabId) }
  }

  /** Items for the dropdown under a field; null when nothing should show. */
  menu(e: PageEvent, kind: unknown): MenuReply | null {
    const ctx = this.context(e)
    if (!ctx || !isKind(kind) || !this.enabled(kind) || !canFill(kind, ctx.origin)) return null
    this.load()
    if (kind === 'card') {
      if (!this.encryption) return null
      const cards = [...this.cards].sort(byUsage).slice(0, MENU_MAX)
      if (!cards.length) return null
      this.menus.set(ctx.key, { kind, tabId: ctx.tabId, origin: ctx.origin, ids: new Set(cards.map((c) => c.id)), at: Date.now() })
      return {
        kind,
        title: t('autofill.menu.cardsTitle'),
        manage: t('autofill.menu.manageCards'),
        items: cards.map((c) => {
          const label = `${c.nickname || NETWORK_NAMES[c.network] || t('autofill.card')} •••• ${c.last4}`
          const detail = t('autofill.menu.expires', { date: formatExpiry(c.expMonth, c.expYear) })
          return { id: c.id, label, detail, match: `${label} ${c.holder}`.toLowerCase() }
        })
      }
    }
    const addresses = [...this.addresses].sort(byUsage).slice(0, MENU_MAX)
    if (!addresses.length) return null
    this.menus.set(ctx.key, { kind, tabId: ctx.tabId, origin: ctx.origin, ids: new Set(addresses.map((a) => a.id)), at: Date.now() })
    return {
      kind,
      title: t('autofill.menu.addressesTitle'),
      manage: t('autofill.menu.manageAddresses'),
      items: addresses.map((a) => {
        const name = fullName(a)
        const label = name || a.street.split('\n')[0]
        const detail = name ? a.street.split('\n')[0] : ''
        return { id: a.id, label, detail, match: `${name} ${a.street} ${a.city}`.toLowerCase() }
      })
    }
  }

  /** The menu this frame opened, if the request is for one of its items and it is still valid. */
  private ownedMenu(ctx: NonNullable<ReturnType<Autofill['context']>>, kind: AutofillKind, id: string): MenuState | null {
    const m = this.menus.get(ctx.key)
    return m && m.kind === kind && m.origin === ctx.origin && m.ids.has(id) && Date.now() - m.at < MENU_TTL ? m : null
  }

  /** Values for a chosen item. A card is only released after the user's authentication. */
  async fill(e: PageEvent, kind: unknown, id: unknown): Promise<AddressFill | CardFill | null> {
    const ctx = this.context(e)
    if (!ctx || !isKind(kind) || typeof id !== 'string' || !this.enabled(kind) || !canFill(kind, ctx.origin)) return null
    const menu = this.ownedMenu(ctx, kind, id)
    if (!menu) return null
    this.load()
    const now = Date.now()
    if (kind === 'address') {
      const a = this.addresses.find((x) => x.id === id)
      if (!a) return null
      if (!ctx.isPrivate) {
        Object.assign(a, { useCount: a.useCount + 1, lastUsedAt: now })
        this.save()
      }
      const { id: _id, createdAt: _c, updatedAt: _u, useCount: _n, lastUsedAt: _l, ...address } = a
      return { address }
    }
    const card = this.cards.find((x) => x.id === id)
    if (!card || !this.encryption || this.authBusy) return null
    this.authBusy = true
    let ok: boolean
    try {
      ok = await this.authenticate(t('autofill.authCard', { site: ctx.origin.replace(/^\w+:\/\//, '') }))
    } finally {
      this.authBusy = false
    }
    // The page may have moved on while the system prompt was open: same frame, same origin, same menu.
    const again = this.context(e)
    if (!ok || !again || again.key !== ctx.key || again.origin !== ctx.origin || this.menus.get(ctx.key) !== menu) return null
    this.menus.delete(ctx.key)
    const number = this.decrypt(card)
    if (number === null) return null
    if (!ctx.isPrivate) {
      Object.assign(card, { useCount: card.useCount + 1, lastUsedAt: now })
      this.save()
    }
    return { network: card.network, holder: card.holder, number, expMonth: card.expMonth, expYear: card.expYear }
  }

  manage(e: PageEvent, kind: unknown): void {
    if (!this.context(e) || !isKind(kind)) return
    this.b.settings.open('autofill')
  }

  // ───────────────────────── Save offer ─────────────────────────

  /** Form submitted in a tab: offers to save a new address or card, or to update a known one. */
  submitted(e: PageEvent, payload: unknown): void {
    const ctx = this.context(e)
    if (!ctx || !isPlain(payload)) return
    this.load()
    const { tabId, origin } = ctx
    const base = { isPrivate: ctx.isPrivate, origin, never: this.settings.neverSaveAutofill, encryption: this.encryption }
    const offers: Pending[] = []
    if (isPlain(payload.address) && mayOffer({ ...base, kind: 'address', enabled: this.settings.autofillAddresses })) {
      const typed = cleanAddress(payload.address)
      const plan = planAddress(this.addresses, typed)
      if (plan.action === 'save') offers.push({ tabId, origin, kind: 'address', summary: addressSummary(typed), update: false, address: typed })
      else if (plan.action === 'update')
        offers.push({ tabId, origin, kind: 'address', summary: addressSummary(plan.merged), update: true, existingId: plan.id, address: plan.merged })
    }
    if (isPlain(payload.card) && mayOffer({ ...base, kind: 'card', enabled: this.settings.autofillCards })) {
      const card = parseCardSubmission(payload.card)
      if (card) {
        const plan = planCard(
          this.cards.map((c) => ({ id: c.id, number: this.decrypt(c), holder: c.holder, expMonth: c.expMonth, expYear: c.expYear })),
          card
        )
        if (plan.action !== 'none')
          offers.push({
            tabId,
            origin,
            kind: 'card',
            summary: cardSummary(card),
            update: plan.action === 'update',
            existingId: plan.action === 'update' ? plan.id : undefined,
            card
          })
      }
    }
    if (!offers.length) return
    // Anti-spam: one batch of offers per tab and origin every 3 s.
    const now = Date.now()
    const rateKey = `${tabId}\n${origin}`
    if (now - (this.lastOffer.get(rateKey) ?? 0) < 3000) return
    this.lastOffer.set(rateKey, now)
    if (this.lastOffer.size > 200) for (const [k, at] of this.lastOffer) if (now - at > 3000) this.lastOffer.delete(k)
    // Only the last submission of a tab counts.
    for (const [token, p] of this.pending) if (p.tabId === tabId) this.pending.delete(token)
    for (const offer of offers) this.pending.set(randomUUID(), offer)
    this.showNext(tabId)
  }

  private showNext(tabId: string): void {
    if (this.overlayToken) return
    const entry = [...this.pending].find(([, p]) => p.tabId === tabId)
    if (!entry) return
    const [token, p] = entry
    if (this.b.overlay.openPassword(tabId, { token, origin: p.origin, username: p.summary, update: p.update, kind: p.kind })) this.overlayToken = token
    else if (this.b.model.activeTab()?.id === tabId && tabId !== this.b.peek.tabId) void this.askInDialog(token)
    else this.pending.delete(token) // background tab or busy overlay: no prompt to come back to
  }

  /** Fallback outside the main window (Little Signet, Peek, overlay busy): native dialog box. */
  private async askInDialog(token: string): Promise<void> {
    const p = this.pending.get(token)
    if (!p) return
    const host = p.origin.replace(/^\w+:\/\//, '')
    const win = this.b.win
    const options: Electron.MessageBoxOptions = {
      type: 'question',
      buttons: [p.update ? t('passwords.prompt.update') : t('passwords.prompt.save'), t('passwords.prompt.never'), t('passwords.prompt.notNow')],
      defaultId: 0,
      cancelId: 2,
      message: t(titleKey(p), { site: host }),
      detail: p.summary
    }
    const { response } = win && !win.isDestroyed() ? await dialog.showMessageBox(win, options) : await dialog.showMessageBox(options)
    this.answer(token, response === 0 ? 'save' : response === 1 ? 'never' : 'dismiss')
  }

  /** The prompt shown in the overlay tucked itself away: same as "Not now". */
  dismissPrompt(): void {
    if (this.overlayToken) this.answer(this.overlayToken, 'dismiss')
  }

  /** Answer to a prompt; false when the token is not one of ours. */
  answer(token: string, answer: 'save' | 'never' | 'dismiss'): boolean {
    const p = this.pending.get(token)
    if (!p) return false
    this.pending.delete(token)
    this.load()
    if (answer === 'save') {
      if (p.kind === 'address' && p.address) this.storeAddress(p)
      else if (p.kind === 'card' && p.card && this.encryption) this.storeCard(p)
    } else if (answer === 'never') {
      this.settings.neverSaveAutofill = [...new Set([...(this.settings.neverSaveAutofill ?? []), p.origin])]
      // Other offers for this site go with it.
      for (const [other, o] of this.pending) if (o.tabId === p.tabId && o.origin === p.origin) this.pending.delete(other)
      this.b.broadcast.changed()
    }
    if (token === this.overlayToken) {
      this.overlayToken = null
      if (this.b.overlay.open) this.b.overlay.close()
    }
    // The next offer of the tab (a card after an address) once the overlay has left.
    setTimeout(() => this.showNext(p.tabId), 350)
    return true
  }

  private storeAddress(p: Pending): void {
    const typed = p.address as AddressInput
    const now = Date.now()
    const existing = (p.existingId && this.addresses.find((a) => a.id === p.existingId)) || this.addresses.find((a) => sameAddress(a, typed))
    if (existing) Object.assign(existing, typed, { updatedAt: now, lastUsedAt: now })
    else this.addresses.push({ id: randomUUID(), ...typed, createdAt: now, updatedAt: now, useCount: 1, lastUsedAt: now })
    this.save()
  }

  private storeCard(p: Pending): void {
    const c = p.card as CardCandidate
    const now = Date.now()
    const existing = (p.existingId && this.cards.find((x) => x.id === p.existingId)) || this.cards.find((x) => this.decrypt(x) === c.number)
    if (existing) {
      Object.assign(existing, { holder: c.holder || existing.holder, expMonth: c.expMonth, expYear: c.expYear, updatedAt: now, lastUsedAt: now })
    } else {
      this.cards.push({
        id: randomUUID(),
        holder: c.holder,
        nickname: '',
        expMonth: c.expMonth,
        expYear: c.expYear,
        number: this.encrypt(c.number),
        last4: c.last4,
        network: c.network,
        createdAt: now,
        updatedAt: now,
        useCount: 1,
        lastUsedAt: now
      })
    }
    this.save()
  }

  /** Tab navigation: dropdowns and offers about another site are forgotten. */
  onNavigate(tabId: string, url: string): void {
    const site = siteOf(url)
    for (const [key, m] of this.menus) if (m.tabId === tabId) this.menus.delete(key)
    for (const [token, p] of this.pending) {
      if (p.tabId !== tabId || siteOf(p.origin) === site) continue
      this.pending.delete(token)
      if (token === this.overlayToken) {
        this.overlayToken = null
        if (this.b.overlay.open) this.b.overlay.close()
      }
    }
  }
}

const titleKey = (p: Pending) =>
  p.kind === 'address'
    ? p.update
      ? ('autofill.prompt.updateAddressTitle' as const)
      : ('autofill.prompt.saveAddressTitle' as const)
    : p.update
      ? ('autofill.prompt.updateCardTitle' as const)
      : ('autofill.prompt.saveCardTitle' as const)

/** One line describing an address in a prompt. */
export function addressSummary(a: AddressInput): string {
  return [fullName(a), a.street.split('\n')[0], a.city || a.postalCode].filter(Boolean).join(', ')
}

/** One line describing a card in a prompt: never more than the last four digits. */
export function cardSummary(c: CardCandidate): string {
  const mm = String(c.expMonth).padStart(2, '0')
  return `${NETWORK_NAMES[c.network] || t('autofill.card')} •••• ${c.last4}, ${mm}/${String(c.expYear).slice(-2)}`
}

async function confirmDialog(reason: string): Promise<boolean> {
  const { response } = await dialog.showMessageBox({
    type: 'warning',
    buttons: [t('passwords.confirm.allow'), t('passwords.confirm.cancel')],
    defaultId: 1,
    cancelId: 1,
    message: t('passwords.confirm.title'),
    detail: reason
  })
  return response === 0
}

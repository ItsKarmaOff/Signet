import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const env = vi.hoisted(() => ({ dir: '', encryption: true, encrypted: [] as string[] }))
vi.mock('electron', () => ({
  app: { getPath: () => env.dir, getPreferredSystemLanguages: () => ['en-US'] },
  webContents: { getAllWebContents: () => [] },
  BrowserWindow: class {},
  dialog: { showMessageBox: async () => ({ response: 0 }) },
  ipcMain: {},
  systemPreferences: { canPromptTouchID: () => false },
  safeStorage: {
    isEncryptionAvailable: () => env.encryption,
    // Fake reversible encryption, enough for the test; records what it was asked to encrypt.
    encryptString: (s: string) => {
      env.encrypted.push(s)
      return Buffer.from('enc:' + s)
    },
    decryptString: (b: Buffer) => b.toString().replace(/^enc:/, '')
  }
}))

import type { Browser } from '../../browser/browser'
import { Autofill, type AutofillDeps, type CardFill, type MenuReply } from './autofill'

const VISA = '4242424242424242'
const address = {
  givenName: 'Jeanne',
  familyName: 'Dupont',
  organization: '',
  street: '12 rue de la Paix',
  city: 'Paris',
  postalCode: '75002',
  region: '',
  country: 'FR',
  phone: '',
  email: ''
}
const card = { holder: 'Jeanne Dupont', number: VISA, expMonth: 8, expYear: 2099, nickname: '' }

/** Fake page event: tab 1, main frame (process 1, routing 2), origin of the page. */
const page = (origin = 'https://shop.example', over: { sender?: number; routing?: number } = {}) => ({
  sender: { id: over.sender ?? 1, mainFrame: { processId: 1, routingId: 2 } },
  senderFrame: { origin, processId: 1, routingId: over.routing ?? 2 }
})

interface Fake {
  settings: Record<string, unknown>
  priv: boolean
  opened: unknown[]
  settingsOpened: string[]
}

function setup(deps: AutofillDeps = {}) {
  const fake: Fake = { settings: {}, priv: false, opened: [], settingsOpened: [] }
  const b = {
    store: { state: { tabs: {}, settings: fake.settings } },
    views: { tabIdOfContents: (id: number) => (id === 1 ? 'tab-1' : null) },
    privates: { owns: () => fake.priv },
    overlay: { openPassword: (...a: unknown[]) => (fake.opened.push(a), true), open: false, close: () => {} },
    model: { activeTab: () => ({ id: 'tab-1' }) },
    peek: { tabId: null },
    broadcast: { changed: () => {} },
    settings: { open: (s: string) => fake.settingsOpened.push(s) }
  } as unknown as Browser
  return { fake, af: new Autofill(b, deps) }
}

const asCard = (r: unknown) => r as CardFill | null

describe('Autofill', () => {
  let dir: string
  beforeEach(() => {
    dir = mkdtempSync(join(tmpdir(), 'signet-af-'))
    env.dir = dir
    env.encryption = true
    env.encrypted = []
  })
  afterEach(() => rmSync(dir, { recursive: true, force: true, maxRetries: 5, retryDelay: 50 }))

  describe('storage', () => {
    it('migrates an address saved with a single full name', () => {
      const old = { id: 'a1', name: 'Jeanne van Dupont', street: '1 rue X', city: 'Paris', createdAt: 1, updatedAt: 1, useCount: 0 }
      const mono = { id: 'a2', name: 'Cher', street: '2 rue Y', createdAt: 1, updatedAt: 1, useCount: 0 }
      const done = { id: 'a3', givenName: 'Paul', familyName: 'Martin', name: 'ignored', street: '3 rue Z', createdAt: 1, updatedAt: 1, useCount: 0 }
      writeFileSync(join(dir, 'autofill.json'), JSON.stringify({ version: 1, addresses: [old, mono, done], cards: [] }))
      const list = setup().af.list().addresses
      const by = (id: string) => list.find((a) => a.id === id)!
      expect(by('a1')).toMatchObject({ givenName: 'Jeanne', familyName: 'van Dupont' })
      expect(by('a2')).toMatchObject({ givenName: 'Cher', familyName: '' })
      expect(by('a3')).toMatchObject({ givenName: 'Paul', familyName: 'Martin' })
      for (const a of list) expect(a).not.toHaveProperty('name')
    })

    it('persists addresses and cards, the number only encrypted, never the CVC', async () => {
      const { af } = setup()
      expect(af.upsertAddress(null, address)).toBeNull()
      expect(await af.upsertCard(null, card)).toBeNull()
      af.flush()
      const raw = readFileSync(join(dir, 'autofill.json'), 'utf8')
      expect(raw).not.toContain(VISA)
      expect(raw).not.toMatch(/cvc|cvv|csc/i)
      const file = JSON.parse(raw)
      expect(file.version).toBe(1)
      expect(file.addresses[0]).toMatchObject({ givenName: 'Jeanne', familyName: 'Dupont', country: 'FR' })
      expect(file.addresses[0]).not.toHaveProperty('name')
      expect(file.cards[0]).toMatchObject({ last4: '4242', network: 'visa', expMonth: 8, expYear: 2099 })
      expect(Buffer.from(file.cards[0].number, 'base64').toString()).toBe('enc:' + VISA)
      expect(env.encrypted).toEqual([VISA])
      // A new instance finds them; the list never carries a number.
      const again = setup().af.list()
      expect(again.addresses).toHaveLength(1)
      expect(again.cards).toHaveLength(1)
      expect(JSON.stringify(again)).not.toContain(VISA)
    })

    it('without encryption, no card is saved but addresses still are', async () => {
      env.encryption = false
      const { af } = setup()
      expect(await af.upsertCard(null, card)).toMatch(/Encryption/)
      expect(af.upsertAddress(null, address)).toBeNull()
      expect(af.list()).toMatchObject({ encryption: false, cards: [] })
      expect(env.encrypted).toEqual([])
    })

    it('validates cards: Luhn, expiry, duplicates', async () => {
      const { af } = setup()
      expect(await af.upsertCard(null, { ...card, number: '4242424242424241' })).toMatch(/card number/)
      expect(await af.upsertCard(null, { ...card, number: null })).toMatch(/card number/)
      expect(await af.upsertCard(null, { ...card, expMonth: 13 })).toMatch(/expiry/)
      expect(await af.upsertCard(null, card)).toBeNull()
      expect(await af.upsertCard(null, card)).toMatch(/already/)
    })

    it('edits and deletes', async () => {
      const { af } = setup()
      af.upsertAddress(null, address)
      const id = af.list().addresses[0].id
      expect(af.upsertAddress(id, { ...address, city: 'Lyon' })).toBeNull()
      expect(af.list().addresses[0].city).toBe('Lyon')
      expect(af.upsertAddress('nope', address)).toMatch(/no longer/)
      expect(af.upsertAddress(null, { ...address, city: 'Nice' })).toMatch(/already/)
      af.removeAddress(id)
      expect(af.list().addresses).toEqual([])
    })
  })

  describe('OS authentication', () => {
    it('reveal and a new number for an existing card need the user to verify', async () => {
      const verify = vi.fn(async () => 'ok' as const)
      const { af } = setup({ verify })
      await af.upsertCard(null, card)
      const id = af.list().cards[0].id
      expect(verify).not.toHaveBeenCalled() // adding a card needs nothing
      expect(await af.revealCard(id)).toBe(VISA)
      expect(verify).toHaveBeenCalledTimes(1)
      expect(await af.upsertCard(id, { ...card, number: '5555555555554444' })).toBeNull()
      expect(verify).toHaveBeenCalledTimes(2)
      expect(await af.upsertCard(id, { ...card, number: null, nickname: 'Perso' })).toBeNull() // no new number, no prompt
      expect(verify).toHaveBeenCalledTimes(2)
    })

    it('denied: nothing is revealed or changed', async () => {
      const { af } = setup({ verify: async () => 'denied' })
      await af.upsertCard(null, card)
      const id = af.list().cards[0].id
      expect(await af.revealCard(id)).toBeNull()
      expect(await af.upsertCard(id, { ...card, number: '5555555555554444' })).toMatch(/Authentication/)
      expect(af.list().cards[0].last4).toBe('4242')
    })

    it('unavailable (Linux): falls back to the confirmation, like passwords', async () => {
      const confirm = vi.fn(async () => true)
      const { af } = setup({ verify: async () => 'unavailable', confirm })
      await af.upsertCard(null, card)
      expect(await af.revealCard(af.list().cards[0].id)).toBe(VISA)
      expect(confirm).toHaveBeenCalledOnce()
      const no = setup({ verify: async () => 'unavailable', confirm: async () => false })
      await no.af.upsertCard(null, card)
      expect(await no.af.revealCard(no.af.list().cards[0].id)).toBeNull()
    })
  })

  describe('page menu and fill', () => {
    async function withBoth(deps: AutofillDeps = { verify: async () => 'ok' }) {
      const ctx = setup(deps)
      ctx.af.upsertAddress(null, address)
      await ctx.af.upsertCard(null, card)
      return ctx
    }
    const items = (m: MenuReply | null) => m?.items ?? []

    it('lists addresses and cards for the page, the card without number', async () => {
      const { af } = await withBoth()
      const a = af.menu(page(), 'address')!
      expect(a.items[0]).toMatchObject({ label: 'Jeanne Dupont', detail: '12 rue de la Paix' })
      const c = af.menu(page(), 'card')!
      expect(c.items[0]).toMatchObject({ label: 'Visa •••• 4242', detail: 'exp 08/99' })
      expect(JSON.stringify(c)).not.toContain(VISA)
    })

    it('only the main frame of a tab, on http(s) pages', async () => {
      const { af } = await withBoth()
      expect(items(af.menu(page('https://x.example', { sender: 7 }), 'address'))).toEqual([]) // not a tab
      expect(af.menu({ ...page(), senderFrame: { origin: 'https://x.example', processId: 1, routingId: 99 } }, 'address')).toBeNull() // subframe
      expect(af.menu({ ...page(), senderFrame: null }, 'address')).toBeNull()
      expect(af.menu(page('file:///a'), 'address')).toBeNull()
      expect(af.menu(page('signetui://app'), 'address')).toBeNull()
      expect(af.menu(page(), 'bogus')).toBeNull()
    })

    it('a card needs https (or localhost), an address does not', async () => {
      const { af } = await withBoth()
      expect(af.menu(page('http://shop.example'), 'card')).toBeNull()
      expect(af.menu(page('http://shop.example'), 'address')).not.toBeNull()
      expect(af.menu(page('http://localhost:3000'), 'card')).not.toBeNull()
    })

    it('respects the two switches', async () => {
      const { af, fake } = await withBoth()
      fake.settings.autofillCards = false
      expect(af.menu(page(), 'card')).toBeNull()
      expect(af.menu(page(), 'address')).not.toBeNull()
      fake.settings.autofillAddresses = false
      expect(af.menu(page(), 'address')).toBeNull()
    })

    it('an address fills without authentication and counts the use', async () => {
      const verify = vi.fn(async () => 'ok' as const)
      const { af } = await withBoth({ verify })
      const id = af.menu(page(), 'address')!.items[0].id
      const res = await af.fill(page(), 'address', id)
      expect(res).toMatchObject({ address: { givenName: 'Jeanne', familyName: 'Dupont', city: 'Paris' } })
      expect(verify).not.toHaveBeenCalled()
      expect(af.list().addresses[0].useCount).toBe(1)
    })

    it('a card is only released after verifyUser says ok', async () => {
      const verify = vi.fn(async () => 'ok' as const)
      const { af } = await withBoth({ verify })
      const id = af.menu(page(), 'card')!.items[0].id
      const res = asCard(await af.fill(page(), 'card', id))
      expect(verify).toHaveBeenCalledOnce()
      expect(res).toMatchObject({ number: VISA, holder: 'Jeanne Dupont', expMonth: 8, expYear: 2099, network: 'visa' })
      expect(res).not.toHaveProperty('cvc')
    })

    it('denied: no number leaves main', async () => {
      const { af } = await withBoth({ verify: async () => 'denied' })
      const id = af.menu(page(), 'card')!.items[0].id
      expect(await af.fill(page(), 'card', id)).toBeNull()
      expect(af.list().cards[0].useCount).toBe(0)
    })

    it('unavailable: asks for a confirmation instead', async () => {
      const confirm = vi.fn(async () => false)
      const { af } = await withBoth({ verify: async () => 'unavailable', confirm })
      const id = af.menu(page(), 'card')!.items[0].id
      expect(await af.fill(page(), 'card', id)).toBeNull()
      expect(confirm).toHaveBeenCalledOnce()
    })

    it('refuses a fill the frame never asked a menu for, or for an item the menu did not list', async () => {
      const { af } = await withBoth()
      const id = af.menu(page(), 'card')!.items[0].id
      expect(await af.fill(page(), 'address', id)).toBeNull() // asked for a card menu
      expect(await af.fill(page('https://other.example'), 'card', id)).toBeNull() // another origin
      expect(await af.fill(page(), 'card', 'unknown-id')).toBeNull()
      const sub = { ...page(), senderFrame: { origin: 'https://shop.example', processId: 1, routingId: 77 } }
      expect(await af.fill(sub, 'card', id)).toBeNull() // another frame
    })

    it('a navigation closes the menus', async () => {
      const { af } = await withBoth()
      const id = af.menu(page(), 'address')!.items[0].id
      af.onNavigate('tab-1', 'https://elsewhere.example/')
      expect(await af.fill(page(), 'address', id)).toBeNull()
    })

    it('private window: filling works, usage is not recorded', async () => {
      const { af, fake } = await withBoth()
      fake.priv = true
      const id = af.menu(page(), 'address')!.items[0].id
      expect(await af.fill(page(), 'address', id)).not.toBeNull()
      expect(af.list().addresses[0].useCount).toBe(0)
    })

    it('"Manage" opens the Autofill section of Settings', async () => {
      const { af, fake } = await withBoth()
      af.manage(page(), 'card')
      af.manage(page('https://x.example', { sender: 9 }), 'card')
      expect(fake.settingsOpened).toEqual(['autofill'])
    })
  })

  describe('save offer', () => {
    const submit = { address: { ...address, country: 'France' }, card: { holder: 'Jeanne', number: '4242 4242 4242 4242', exp: '08/99' } }

    it('offers a new address and a new card, one prompt at a time', () => {
      vi.useFakeTimers()
      try {
        const { af, fake } = setup()
        af.submitted(page(), submit)
        expect(fake.opened).toHaveLength(1)
        const [tab, prompt] = fake.opened[0] as [string, { kind: string; username: string; update: boolean; token: string }]
        expect(tab).toBe('tab-1')
        expect(prompt).toMatchObject({ kind: 'address', update: false })
        expect(prompt.username).toContain('Jeanne Dupont')
        // Save: stored, then the card prompt comes.
        expect(af.answer(prompt.token, 'save')).toBe(true)
        expect(af.list().addresses).toHaveLength(1)
        vi.advanceTimersByTime(400)
        const next = (fake.opened[1] as [string, { kind: string; username: string; token: string }])[1]
        expect(next.kind).toBe('card')
        expect(next.username).toBe('Visa •••• 4242, 08/99')
        expect(JSON.stringify(fake.opened)).not.toContain(VISA)
        af.answer(next.token, 'save')
        expect(af.list().cards[0]).toMatchObject({ last4: '4242', holder: 'Jeanne' })
        expect(env.encrypted).toEqual([VISA])
      } finally {
        vi.useRealTimers()
      }
    })

    it('does not offer what is already saved, and updates what clearly matches', async () => {
      const { af, fake } = setup()
      af.upsertAddress(null, address)
      await af.upsertCard(null, card)
      af.submitted(page(), { address, card: { number: VISA, holder: card.holder, expMonth: '8', expYear: '2099' } })
      expect(fake.opened).toHaveLength(0)
      af.submitted(page(), { address: { ...address, postalCode: '75008' } })
      const prompt = (fake.opened[0] as [string, { update: boolean; token: string }])[1]
      expect(prompt.update).toBe(true)
      af.answer(prompt.token, 'save')
      const { addresses } = af.list()
      expect(addresses).toHaveLength(1)
      expect(addresses[0].postalCode).toBe('75008')
    })

    it('"Not now" stores nothing and does not remember', () => {
      const { af, fake } = setup()
      af.submitted(page(), { address })
      const token = (fake.opened[0] as [string, { token: string }])[1].token
      af.answer(token, 'dismiss')
      expect(af.list().addresses).toEqual([])
      expect(af.answer(token, 'save')).toBe(false) // gone
    })

    it('"Never for this site" stores the origin and stops the offers there', () => {
      const { af, fake } = setup()
      af.submitted(page(), { address })
      af.answer((fake.opened[0] as [string, { token: string }])[1].token, 'never')
      expect(fake.settings.neverSaveAutofill).toEqual(['https://shop.example'])
      expect(af.list().addresses).toEqual([])
      fake.opened.length = 0
      af.submitted(page(), { address: { ...address, street: '1 other street' } })
      expect(fake.opened).toHaveLength(0)
      af.submitted(page('https://another.example'), { address: { ...address, street: '1 other street' } })
      expect(fake.opened).toHaveLength(1)
    })

    it('never in a private window', () => {
      const { af, fake } = setup()
      fake.priv = true
      af.submitted(page(), submit)
      expect(fake.opened).toHaveLength(0)
    })

    it('respects the switches, the card rules and the sender', () => {
      const a = setup()
      a.fake.settings.autofillAddresses = false
      a.af.submitted(page(), { address })
      expect(a.fake.opened).toHaveLength(0)
      const c = setup()
      c.fake.settings.autofillCards = false
      c.af.submitted(page(), { card: submit.card })
      expect(c.fake.opened).toHaveLength(0)
      const http = setup()
      http.af.submitted(page('http://shop.example'), { card: submit.card })
      expect(http.fake.opened).toHaveLength(0)
      env.encryption = false
      const none = setup()
      none.af.submitted(page(), { card: submit.card })
      expect(none.fake.opened).toHaveLength(0)
      env.encryption = true
      const other = setup()
      other.af.submitted(page('https://x.example', { sender: 5 }), { address })
      other.af.submitted(page(), 'not an object')
      expect(other.fake.opened).toHaveLength(0)
    })

    it('invalid cards and incomplete addresses are not offered', () => {
      const { af, fake } = setup()
      af.submitted(page(), { card: { number: '4242424242424241', exp: '08/99' }, address: { givenName: 'J', street: 'x' } })
      af.submitted(page(), { card: { number: VISA, exp: '01/20' } })
      expect(fake.opened).toHaveLength(0)
    })
  })
})

// Bridge between the page (injected script) and the autofill store: fill bar above the keyboard, save offers after a submit.
// One instance per tab (BrowserView). Card numbers only live in memory for the time of a fill (read after Face ID, injected,
// dropped) or of an unanswered save offer (2 minutes at most); they are never logged nor stored outside the Keychain.
import { useRouter } from 'expo-router'
import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import { Keyboard, StyleSheet } from 'react-native'
import Animated, { FadeInDown, FadeOutDown } from 'react-native-reanimated'
import { cleanAddress } from '@signet/shared/autofill/rules'
import { parseAutofillMessage, type AddressFields, type CardFields } from '@signet/shared/mobile/autofill-messages'
import { siteOf } from '@signet/shared/site'
import type { AddressInput, TabInfo } from '@signet/shared/types'
import { haptic } from '@/lib/haptics'
import { t } from '@/lib/i18n'
import { useStore } from '@/state'
import { gutter, icons, toast, useReduceMotion } from '@/ui'
import { authenticate } from '../passwords/auth'
import { OfferLayer, SaveCard } from '../passwords/PasswordOverlay'
import { AutofillBar, type BarItem } from './AutofillBar'
import { addressChipLabel, addressSummary, cardChipLabel, cardSummary, errorMessage } from './format'
import { autofillScript, fillCall } from './pageScript'
import {
  addressComplete,
  cardNumber,
  loadAutofill,
  markUsed,
  mayFill,
  mayOfferSave,
  neverOfferFor,
  parseCardSubmission,
  planAddressSubmission,
  planCardSubmission,
  saveCandidate,
  sortedByUsage,
  upsertAddress,
  useAddresses,
  useCards,
  type AutofillKind,
  type CardCandidate
} from './store'

/** Items in the fill bar. */
const BAR_MAX = 8
/** An unanswered offer is forgotten after 2 minutes. */
const OFFER_TTL = 2 * 60_000
/** One batch of offers per tab and origin every 3 seconds. */
const OFFER_GAP = 3000

interface WebHandle {
  injectJavaScript: (js: string) => void
}

type Offer =
  | { kind: 'address'; origin: string; summary: string; existingId?: string; address: AddressInput }
  | { kind: 'card'; origin: string; summary: string; existingId?: string; card: CardCandidate }

const originOf = (url: string): string | null => {
  try {
    const u = new URL(url)
    return u.protocol === 'https:' || u.protocol === 'http:' ? u.origin : null
  } catch {
    return null
  }
}

export interface AutofillBridge {
  /** To add to the injected scripts after the page loads (main frame only). */
  injectedScript: string
  /** Receives the page's `af:*` messages (already parsed JSON; the token is checked here too). */
  onMessage: (msg: unknown) => void
  /** Fill bar above the keyboard (to render in the tab's view). */
  bar: ReactNode
  /** Save offer card (to render in the tab's view, after the password overlay). */
  overlay: ReactNode
}

/**
 * @param token session token of the WebView
 * @param bottomInset height at the bottom of the screen that the tab's view does not cover (to follow the keyboard)
 * @param blocked another prompt (a password save card) is showing: an offer waits for it to go
 */
export function useAutofillBridge(
  webRef: { current: WebHandle | null },
  tab: TabInfo,
  isPrivate: boolean,
  visible: boolean,
  token: string,
  bottomInset = 0,
  blocked = false
): AutofillBridge {
  const router = useRouter()
  const reduce = useReduceMotion()
  const addressesOn = useStore((s) => s.settings.autofillAddresses)
  const cardsOn = useStore((s) => s.settings.autofillCards)
  const addresses = useAddresses()
  const cards = useCards()
  const [bar, setBar] = useState<{ kind: AutofillKind } | null>(null)
  const [offers, setOffers] = useState<Offer[]>([])
  const urlRef = useRef(tab.url)
  const visibleRef = useRef(visible)
  const lastFocus = useRef<{ kind: AutofillKind; origin: string } | null>(null)
  const lastBatch = useRef(new Map<string, number>())
  const busy = useRef(false)
  urlRef.current = tab.url
  visibleRef.current = visible

  const enabled = useCallback((kind: AutofillKind) => {
    const s = useStore.getState().settings
    return kind === 'address' ? s.autofillAddresses : s.autofillCards
  }, [])

  const hideBar = useCallback(() => setBar(null), [])

  // Navigation, tab hidden, switch turned off: no bar.
  useEffect(() => {
    hideBar()
  }, [tab.url, visible, addressesOn, cardsOn, hideBar])

  // Keyboard gone: no bar above it.
  useEffect(() => {
    const sub = Keyboard.addListener('keyboardDidHide', hideBar)
    return () => sub.remove()
  }, [hideBar])

  // Offers: never in private tabs, forgotten after 2 minutes and when the tab leaves the site.
  useEffect(() => {
    if (isPrivate || !addressesOn) setOffers((o) => o.filter((x) => x.kind !== 'address'))
    if (isPrivate || !cardsOn) setOffers((o) => o.filter((x) => x.kind !== 'card'))
  }, [isPrivate, addressesOn, cardsOn])
  useEffect(() => {
    if (!offers.length) return
    const timer = setTimeout(() => setOffers([]), OFFER_TTL)
    return () => clearTimeout(timer)
  }, [offers])
  useEffect(() => {
    setOffers((list) => {
      const kept = list.filter((o) => (siteOf(o.origin) || o.origin) === (siteOf(tab.url) || originOf(tab.url)))
      return kept.length === list.length ? list : kept
    })
  }, [tab.url])

  const items = useMemo<BarItem[]>(() => {
    if (!bar) return []
    if (bar.kind === 'address')
      return sortedByUsage(addresses)
        .slice(0, BAR_MAX)
        .map((a) => {
          const label = addressChipLabel(t, a)
          return { id: a.id, label, a11y: t('autofill.fillAddressLabel', { name: label }) }
        })
    return sortedByUsage(cards)
      .slice(0, BAR_MAX)
      .map((c) => {
        const label = cardChipLabel(t, c)
        return { id: c.id, label, a11y: t('autofill.fillCardLabel', { name: label }) }
      })
  }, [bar, addresses, cards])

  // The page fill still applies: same site as when the bar showed, switch on, page rules.
  const stillApplies = useCallback(
    (kind: AutofillKind, origin: string) => visibleRef.current && originOf(urlRef.current) === origin && mayFill(kind, origin, enabled(kind)),
    [enabled]
  )

  const onFocus = useCallback(
    (kind: AutofillKind, origin: string) => {
      // The origin announced by the page must be the tab's.
      if (!visibleRef.current || origin !== originOf(urlRef.current) || !mayFill(kind, origin, enabled(kind))) return setBar(null)
      lastFocus.current = { kind, origin }
      void loadAutofill()
      setBar({ kind })
    },
    [enabled]
  )

  const pick = useCallback(
    async (id: string) => {
      const focus = lastFocus.current
      if (!focus || busy.current || !stillApplies(focus.kind, focus.origin)) return
      busy.current = true
      try {
        const { kind, origin } = focus
        if (kind === 'address') {
          const a = addresses.find((x) => x.id === id)
          if (!a) return
          const address: AddressInput = {
            givenName: a.givenName,
            familyName: a.familyName,
            organization: a.organization,
            street: a.street,
            city: a.city,
            postalCode: a.postalCode,
            region: a.region,
            country: a.country,
            phone: a.phone,
            email: a.email
          }
          webRef.current?.injectJavaScript(fillCall(token, { o: origin, kind, address }))
          if (!isPrivate) void markUsed('address', id)
        } else {
          const c = cards.find((x) => x.id === id)
          if (!c) return
          // Face ID every time: a card number never reaches a page without it.
          if (!(await authenticate(t('autofill.authFill'), { fresh: true }))) return
          // The page may have changed while the prompt was up.
          if (!stillApplies(kind, origin)) return
          const number = await cardNumber(id)
          if (!number) return void toast.show({ message: t('autofill.fillFailed') })
          webRef.current?.injectJavaScript(
            fillCall(token, { o: origin, kind, card: { network: c.network, holder: c.holder, number, expMonth: c.expMonth, expYear: c.expYear } })
          )
          if (!isPrivate) void markUsed('card', id)
        }
        haptic('success')
        setBar(null)
      } finally {
        busy.current = false
      }
    },
    [addresses, cards, isPrivate, stillApplies, token, webRef]
  )

  const manage = useCallback(() => {
    setBar(null)
    router.push('/settings/autofill')
  }, [router])

  const onSubmit = useCallback(
    async (origin: string, rawAddress: AddressFields | undefined, rawCard: CardFields | undefined) => {
      // The origin announced by the page must be the tab's, and offers never happen in a private tab.
      if (isPrivate || origin !== originOf(urlRef.current)) return
      await loadAutofill()
      const settings = useStore.getState().settings
      const next: Offer[] = []
      if (rawAddress && mayOfferSave('address', origin, { isPrivate, enabled: settings.autofillAddresses })) {
        const typed = cleanAddress(rawAddress)
        if (addressComplete(typed)) {
          const plan = planAddressSubmission(typed)
          if (plan.action === 'save') next.push({ kind: 'address', origin, summary: addressSummary(typed), address: typed })
          else if (plan.action === 'update')
            next.push({ kind: 'address', origin, summary: addressSummary(plan.merged), existingId: plan.id, address: plan.merged })
        }
      }
      if (rawCard && mayOfferSave('card', origin, { isPrivate, enabled: settings.autofillCards })) {
        const card = parseCardSubmission(rawCard)
        if (card) {
          const plan = await planCardSubmission(card)
          if (plan.action !== 'none')
            next.push({ kind: 'card', origin, summary: cardSummary(t, card), existingId: plan.action === 'update' ? plan.id : undefined, card })
        }
      }
      if (!next.length) return
      // Anti-spam: one batch per origin every 3 s. The latest submission replaces what was waiting.
      const now = Date.now()
      if (now - (lastBatch.current.get(origin) ?? 0) < OFFER_GAP) return
      lastBatch.current.set(origin, now)
      haptic('light')
      setOffers(next)
    },
    [isPrivate]
  )

  const onMessage = useCallback(
    (raw: unknown) => {
      const msg = parseAutofillMessage(raw, token)
      if (!msg) return
      if (msg.type === 'af:focus') onFocus(msg.kind, msg.origin)
      else if (msg.type === 'af:blur') hideBar()
      else if (msg.type === 'af:submit') void onSubmit(msg.origin, msg.address, msg.card)
      else if (msg.type === 'af:filled' && !msg.ok && visibleRef.current) toast.show({ message: t('autofill.fillFailed') })
    },
    [hideBar, onFocus, onSubmit, token]
  )

  const offer = visible && !blocked && !isPrivate ? offers[0] : undefined

  const answer = useCallback(
    async (what: 'save' | 'later' | 'never') => {
      const current = offers[0]
      if (!current) return
      if (what === 'never') {
        setOffers((l) => l.filter((o) => o.origin !== current.origin))
        await neverOfferFor(current.origin).catch(() => undefined)
        toast.show({ message: t('autofill.offer.neverDone', { site: siteOf(current.origin) || current.origin }) })
        return
      }
      setOffers((l) => l.slice(1))
      if (what === 'later') return
      try {
        if (current.kind === 'address') await upsertAddress(current.address, current.existingId)
        else await saveCandidate(current.card, current.existingId)
        toast.show({ message: t(current.existingId ? 'autofill.offer.updated' : 'autofill.offer.saved') })
        haptic('success')
      } catch (err) {
        toast.show({ message: errorMessage(t, err) })
      }
    },
    [offers]
  )

  const barNode = visible ? (
    <AutofillBar kind={bar?.kind ?? 'address'} items={items} bottomInset={bottomInset} onPick={(id) => void pick(id)} onManage={manage} />
  ) : null

  const site = offer ? siteOf(offer.origin) || offer.origin : ''
  const overlay = offer ? (
    <OfferLayer>
      <Animated.View
        key={`${offer.kind}:${offer.existingId ?? 'new'}`}
        entering={reduce ? undefined : FadeInDown.duration(200)}
        exiting={reduce ? undefined : FadeOutDown.duration(150)}
        style={styles.wrap}
      >
        <SaveCard
          icon={offer.kind === 'card' ? icons.CreditCard : icons.MapPin}
          title={offerTitle(offer, site)}
          summary={offer.summary}
          saveLabel={t(offer.existingId ? 'passwords.updateAction' : 'passwords.saveAction')}
          onAnswer={(what) => void answer(what)}
        />
      </Animated.View>
    </OfferLayer>
  ) : null

  return { injectedScript: autofillScript(token), onMessage, bar: barNode, overlay }
}

const offerTitle = (o: Offer, site: string): string => {
  if (o.kind === 'card') return t(o.existingId ? 'autofill.offer.updateCardTitle' : 'autofill.offer.saveCardTitle', { site })
  return t(o.existingId ? 'autofill.offer.updateAddressTitle' : 'autofill.offer.saveAddressTitle', { site })
}

const styles = StyleSheet.create({
  wrap: { paddingHorizontal: gutter - 4 }
})

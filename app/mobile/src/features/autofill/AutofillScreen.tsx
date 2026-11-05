// Settings › Autofill: switches, saved addresses and cards (protected by Face ID), sites never offered.
import { useMemo } from 'react'
import { useRouter } from 'expo-router'
import { useT } from '@/lib/i18n'
import { EmptyState, ListRow, ListSection, Screen, SwitchRow, icons, space } from '@/ui'
import { useStore } from '@/state'
import { addressSubtitle, addressTitle, cardLabel, expiryText } from './format'
import { useAddresses, useAutofillNever, useCards } from './store'
import { useAutofillUnlock } from './useAutofillUnlock'

export function AutofillScreen() {
  const t = useT()
  const router = useRouter()
  const addressesOn = useStore((s) => s.settings.autofillAddresses)
  const cardsOn = useStore((s) => s.settings.autofillCards)
  const setSettings = useStore((s) => s.setSettings)
  const { available, unlocked, unlock } = useAutofillUnlock(t('autofill.authOpen'))
  const addresses = useAddresses()
  const cards = useCards()
  const never = useAutofillNever()

  const sortedAddresses = useMemo(() => [...addresses].sort((a, b) => addressTitle(t, a).localeCompare(addressTitle(t, b))), [addresses, t])
  const sortedCards = useMemo(() => [...cards].sort((a, b) => a.createdAt - b.createdAt), [cards])

  return (
    <Screen scroll contentStyle={{ paddingTop: space.lg, paddingBottom: space.huge }}>
      <ListSection footer={t('autofill.switchAddressesHint')}>
        <SwitchRow
          title={t('autofill.switchAddresses')}
          icon={icons.MapPin}
          iconColor="#3a82f7"
          value={addressesOn}
          onValueChange={(autofillAddresses) => setSettings({ autofillAddresses })}
        />
      </ListSection>
      <ListSection footer={t('autofill.switchCardsHint')}>
        <SwitchRow
          title={t('autofill.switchCards')}
          icon={icons.CreditCard}
          iconColor="#34a56f"
          value={cardsOn}
          onValueChange={(autofillCards) => setSettings({ autofillCards })}
        />
      </ListSection>

      {available === false ? (
        <EmptyState icon={icons.Lock} title={t('autofill.unavailableTitle')} message={t('autofill.unavailable')} />
      ) : !unlocked ? (
        <EmptyState
          icon={icons.Lock}
          title={t('autofill.lockedTitle')}
          message={t('autofill.lockedMessage')}
          action={{ label: t('autofill.unlock'), onPress: () => void unlock() }}
        />
      ) : (
        <>
          <ListSection title={t('autofill.addresses')} footer={addresses.length === 0 ? t('autofill.emptyAddresses') : undefined}>
            <ListRow
              title={t('autofill.addAddress')}
              icon={icons.Plus}
              onPress={() => router.push({ pathname: '/settings/autofill-address', params: { id: 'new' } })}
            />
            {sortedAddresses.map((a) => (
              <ListRow
                key={a.id}
                title={addressTitle(t, a)}
                subtitle={addressSubtitle(a)}
                icon={icons.MapPin}
                iconColor="#3a82f7"
                onPress={() => router.push({ pathname: '/settings/autofill-address', params: { id: a.id } })}
              />
            ))}
          </ListSection>

          <ListSection title={t('autofill.cards')} footer={cards.length === 0 ? t('autofill.emptyCards') : undefined}>
            <ListRow
              title={t('autofill.addCard')}
              icon={icons.Plus}
              onPress={() => router.push({ pathname: '/settings/autofill-card', params: { id: 'new' } })}
            />
            {sortedCards.map((c) => (
              <ListRow
                key={c.id}
                title={c.nickname || cardLabel(t, c)}
                subtitle={c.nickname ? `${cardLabel(t, c)} - ${expiryText(t, c)}` : expiryText(t, c)}
                icon={icons.CreditCard}
                iconColor="#34a56f"
                onPress={() => router.push({ pathname: '/settings/autofill-card', params: { id: c.id } })}
              />
            ))}
          </ListSection>

          <ListSection>
            <ListRow
              title={t('autofill.never')}
              icon={icons.ShieldOff}
              iconColor="#8e8e93"
              value={never.length ? String(never.length) : undefined}
              onPress={() => router.push('/settings/autofill-never')}
            />
          </ListSection>
        </>
      )}
    </Screen>
  )
}

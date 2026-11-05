// Card form: `id=new` adds, otherwise edits (and can delete) a saved card. There is never a security code field.
// The number of a saved card is shown, or replaced, only after a fresh Face ID check, and is dropped from memory as soon
// as it is hidden, the app goes to the background or the screen closes.
import { useEffect, useState } from 'react'
import { View } from 'react-native'
import { Stack, useLocalSearchParams, useRouter } from 'expo-router'
import { cardDigits, detectNetwork, fullYear, groupCardNumber, luhn } from '@signet/shared/autofill/card'
import { haptic } from '@/lib/haptics'
import { useT } from '@/lib/i18n'
import { ActionSheet, Button, EmptyState, ListRow, ListSection, Screen, TextField, Text, gutter, icons, space, toast } from '@/ui'
import { authenticate } from '../passwords/auth'
import { cardLabel, errorMessage } from './format'
import { Locked } from './Locked'
import { cardNumber, removeCard, upsertCard, useCards } from './store'
import { useAutofillUnlock } from './useAutofillUnlock'

const digitsOf = (text: string, max: number) => text.replace(/\D/g, '').slice(0, max)

export function CardEditScreen() {
  const t = useT()
  const router = useRouter()
  const { id } = useLocalSearchParams<{ id?: string }>()
  const isNew = id === 'new'
  const unlock = useAutofillUnlock(t('autofill.authOpen'))
  const cards = useCards()
  const saved = isNew ? undefined : cards.find((c) => c.id === id)
  const [holder, setHolder] = useState('')
  const [month, setMonth] = useState('')
  const [year, setYear] = useState('')
  const [nickname, setNickname] = useState('')
  /** Number being typed (grouped). Empty and unused while a saved number is kept. */
  const [number, setNumber] = useState('')
  const [changing, setChanging] = useState(isNew)
  /** Full number of a saved card while it is on screen. */
  const [revealed, setRevealed] = useState<string | null>(null)
  const [filled, setFilled] = useState(isNew)
  const [confirmDelete, setConfirmDelete] = useState(false)
  const [saving, setSaving] = useState(false)

  useEffect(() => {
    if (filled || !saved) return
    setHolder(saved.holder)
    setMonth(String(saved.expMonth).padStart(2, '0'))
    setYear(String(saved.expYear))
    setNickname(saved.nickname)
    setFilled(true)
  }, [saved, filled])

  // Nothing sensitive stays in memory when the screen locks (background) or closes.
  useEffect(() => {
    if (!unlock.unlocked) {
      setRevealed(null)
      setNumber('')
      setChanging(isNew)
    }
  }, [unlock.unlocked, isNew])

  const digits = digitsOf(number, 19)
  const numberOk = !!cardDigits(digits) && luhn(digits)
  const numberError = digits.length >= 13 && !numberOk ? t('autofill.error.number') : undefined
  const expMonth = Number(month)
  const expYear = fullYear(Number(year))
  const expiryOk = month.length > 0 && Number.isInteger(expMonth) && expMonth >= 1 && expMonth <= 12 && year.length > 0 && expYear !== null
  const needsNumber = isNew || changing
  const canSave = expiryOk && (!needsNumber || numberOk) && !saving

  const reveal = async () => {
    if (!id || isNew) return
    if (!(await authenticate(t('autofill.authReveal'), { fresh: true }))) return void toast.show({ message: t('autofill.error.auth') })
    const full = await cardNumber(id)
    if (full) setRevealed(groupCardNumber(full, detectNetwork(full)))
    else toast.show({ message: t('autofill.error.missing') })
  }

  const change = async () => {
    if (!(await authenticate(t('autofill.authEdit'), { fresh: true }))) return void toast.show({ message: t('autofill.error.auth') })
    setRevealed(null)
    setChanging(true)
  }

  const save = async () => {
    setSaving(true)
    try {
      await upsertCard({ holder, number: needsNumber ? digits : null, expMonth, expYear: expYear ?? 0, nickname }, isNew ? undefined : id)
      haptic('success')
      router.back()
      toast.show({ message: t('autofill.saved') })
    } catch (err) {
      toast.show({ message: errorMessage(t, err) })
    } finally {
      setSaving(false)
    }
  }

  const remove = async () => {
    setConfirmDelete(false)
    try {
      await removeCard(id ?? '')
      haptic('success')
      router.back()
      toast.show({ message: t('autofill.deleted') })
    } catch (err) {
      toast.show({ message: errorMessage(t, err) })
    }
  }

  const header = (
    <Stack.Screen
      options={{
        title: isNew ? t('autofill.addCard') : t('autofill.editCard'),
        headerRight: () =>
          unlock.unlocked && filled ? <Button title={t('autofill.save')} variant="plain" disabled={!canSave} onPress={() => void save()} /> : null
      }}
    />
  )

  if (unlock.available === false || !unlock.unlocked) {
    return (
      <Screen>
        {header}
        <Locked available={unlock.available} unlock={unlock.unlock} />
      </Screen>
    )
  }
  if (!isNew && !saved && !filled) {
    return (
      <Screen>
        {header}
        <EmptyState icon={icons.CreditCard} title={t('autofill.notFound')} />
      </Screen>
    )
  }

  return (
    <Screen scroll contentStyle={{ paddingTop: space.lg, paddingBottom: space.huge }}>
      {header}
      <View style={{ paddingHorizontal: gutter, gap: space.lg, paddingBottom: space.lg }}>
        <TextField label={t('autofill.holder')} value={holder} onChangeText={setHolder} textContentType="name" autoCapitalize="words" autoFocus={isNew} />
        {needsNumber ? (
          <TextField
            label={t('autofill.number')}
            value={number}
            onChangeText={(text) => {
              const d = digitsOf(text, 19)
              setNumber(groupCardNumber(d, detectNetwork(d)))
            }}
            keyboardType="number-pad"
            textContentType="creditCardNumber"
            autoCorrect={false}
            error={numberError}
          />
        ) : (
          <ListSection>
            <ListRow
              title={t('autofill.number')}
              subtitle={revealed ?? `•••• ${saved?.last4 ?? ''}`}
              right={
                <View style={{ flexDirection: 'row' }}>
                  <Button
                    title={revealed ? t('autofill.hide') : t('autofill.show')}
                    variant="plain"
                    onPress={() => (revealed ? setRevealed(null) : void reveal())}
                  />
                  <Button title={t('autofill.change')} variant="plain" onPress={() => void change()} />
                </View>
              }
            />
          </ListSection>
        )}
        {changing && !isNew ? (
          <Button
            title={t('autofill.cancelChange')}
            variant="secondary"
            onPress={() => {
              setChanging(false)
              setNumber('')
            }}
          />
        ) : null}
        <View style={{ flexDirection: 'row', gap: space.md }}>
          <View style={{ flex: 1 }}>
            <TextField
              label={t('autofill.expMonth')}
              value={month}
              onChangeText={(text) => setMonth(digitsOf(text, 2))}
              placeholder={t('autofill.expMonthPlaceholder')}
              keyboardType="number-pad"
              textContentType="creditCardExpirationMonth"
            />
          </View>
          <View style={{ flex: 1 }}>
            <TextField
              label={t('autofill.expYear')}
              value={year}
              onChangeText={(text) => setYear(digitsOf(text, 4))}
              placeholder={t('autofill.expYearPlaceholder')}
              keyboardType="number-pad"
              textContentType="creditCardExpirationYear"
            />
          </View>
        </View>
        <TextField label={t('autofill.nickname')} value={nickname} onChangeText={setNickname} maxLength={40} />
        <Text variant="footnote" color="secondary">
          {t('autofill.cvcFooter')}
        </Text>
      </View>

      {!isNew && saved ? (
        <ListSection>
          <ListRow title={t('autofill.deleteCard')} icon={icons.Trash} destructive onPress={() => setConfirmDelete(true)} />
        </ListSection>
      ) : null}

      <ActionSheet
        visible={confirmDelete}
        onClose={() => setConfirmDelete(false)}
        title={t('autofill.deleteCardTitle')}
        message={saved ? `${cardLabel(t, saved)}. ${t('autofill.deleteMessage')}` : t('autofill.deleteMessage')}
        cancelLabel={t('common.cancel')}
        actions={[{ label: t('autofill.deleteCard'), destructive: true, onPress: () => void remove() }]}
      />
    </Screen>
  )
}

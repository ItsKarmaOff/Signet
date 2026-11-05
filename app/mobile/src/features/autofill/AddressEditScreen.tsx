// Address form: `id=new` adds, otherwise edits (and can delete) a saved address.
import { useEffect, useState } from 'react'
import { View } from 'react-native'
import { Stack, useLocalSearchParams, useRouter } from 'expo-router'
import { countryName } from '@signet/shared/autofill/values'
import { haptic } from '@/lib/haptics'
import { useLocale, useT } from '@/lib/i18n'
import { ActionSheet, Button, EmptyState, ListRow, ListSection, Screen, TextField, gutter, icons, space, toast } from '@/ui'
import { CountrySheet } from './CountrySheet'
import { errorMessage } from './format'
import { Locked } from './Locked'
import { removeAddress, upsertAddress, useAddresses } from './store'
import { useAutofillUnlock } from './useAutofillUnlock'

const MAX_STREET_LINES = 4

interface Form {
  givenName: string
  familyName: string
  organization: string
  street: string
  city: string
  postalCode: string
  region: string
  country: string
  phone: string
  email: string
}

const EMPTY: Form = { givenName: '', familyName: '', organization: '', street: '', city: '', postalCode: '', region: '', country: '', phone: '', email: '' }

export function AddressEditScreen() {
  const t = useT()
  const locale = useLocale()
  const router = useRouter()
  const { id } = useLocalSearchParams<{ id?: string }>()
  const isNew = id === 'new'
  const unlock = useAutofillUnlock(t('autofill.authOpen'))
  const addresses = useAddresses()
  const saved = isNew ? undefined : addresses.find((a) => a.id === id)
  const [form, setForm] = useState<Form>(EMPTY)
  const [filled, setFilled] = useState(isNew)
  const [pickCountry, setPickCountry] = useState(false)
  const [confirmDelete, setConfirmDelete] = useState(false)
  const [saving, setSaving] = useState(false)

  // Loads the saved address into the form once (later changes come from this form).
  useEffect(() => {
    if (filled || !saved) return
    const { givenName, familyName, organization, street, city, postalCode, region, country, phone, email } = saved
    setForm({ givenName, familyName, organization, street, city, postalCode, region, country, phone, email })
    setFilled(true)
  }, [saved, filled])

  const set = (key: keyof Form) => (value: string) => setForm((f) => ({ ...f, [key]: value }))
  const setStreet = (value: string) => setForm((f) => ({ ...f, street: value.split('\n').slice(0, MAX_STREET_LINES).join('\n') }))

  const canSave = Object.values(form).some((v) => v.trim()) && !saving

  const save = async () => {
    setSaving(true)
    try {
      await upsertAddress(form, isNew ? undefined : id)
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
      await removeAddress(id ?? '')
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
        title: isNew ? t('autofill.addAddress') : t('autofill.editAddress'),
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
    // The list is loaded after unlocking: an unknown id means the address was deleted.
    return (
      <Screen>
        {header}
        <EmptyState icon={icons.MapPin} title={t('autofill.notFound')} />
      </Screen>
    )
  }

  return (
    <Screen scroll contentStyle={{ paddingTop: space.lg, paddingBottom: space.huge }}>
      {header}
      <View style={{ paddingHorizontal: gutter, gap: space.lg, paddingBottom: space.lg }}>
        <TextField
          label={t('autofill.givenName')}
          value={form.givenName}
          onChangeText={set('givenName')}
          textContentType="givenName"
          autoCapitalize="words"
          autoFocus={isNew}
        />
        <TextField
          label={t('autofill.familyName')}
          value={form.familyName}
          onChangeText={set('familyName')}
          textContentType="familyName"
          autoCapitalize="words"
        />
        <TextField
          label={t('autofill.organization')}
          value={form.organization}
          onChangeText={set('organization')}
          textContentType="organizationName"
          autoCapitalize="words"
        />
        <TextField
          label={t('autofill.street')}
          value={form.street}
          onChangeText={setStreet}
          textContentType="streetAddressLine1"
          autoCapitalize="words"
          multiline
        />
        <TextField label={t('autofill.city')} value={form.city} onChangeText={set('city')} textContentType="addressCity" autoCapitalize="words" />
        <TextField
          label={t('autofill.postalCode')}
          value={form.postalCode}
          onChangeText={set('postalCode')}
          textContentType="postalCode"
          autoCapitalize="characters"
        />
        <TextField label={t('autofill.region')} value={form.region} onChangeText={set('region')} textContentType="addressState" autoCapitalize="words" />
      </View>
      <ListSection>
        <ListRow
          title={t('autofill.country')}
          value={form.country ? countryName(form.country, locale) : t('autofill.countryNone')}
          onPress={() => setPickCountry(true)}
        />
      </ListSection>
      <View style={{ paddingHorizontal: gutter, gap: space.lg, paddingBottom: space.lg }}>
        <TextField label={t('autofill.phone')} value={form.phone} onChangeText={set('phone')} textContentType="telephoneNumber" keyboardType="phone-pad" />
        <TextField
          label={t('autofill.email')}
          value={form.email}
          onChangeText={set('email')}
          textContentType="emailAddress"
          keyboardType="email-address"
          autoCapitalize="none"
          autoCorrect={false}
        />
      </View>

      {!isNew ? (
        <ListSection>
          <ListRow title={t('autofill.deleteAddress')} icon={icons.Trash} destructive onPress={() => setConfirmDelete(true)} />
        </ListSection>
      ) : null}

      <CountrySheet
        visible={pickCountry}
        value={form.country}
        onSelect={(country) => {
          setForm((f) => ({ ...f, country }))
          setPickCountry(false)
        }}
        onClose={() => setPickCountry(false)}
      />
      <ActionSheet
        visible={confirmDelete}
        onClose={() => setConfirmDelete(false)}
        title={t('autofill.deleteAddressTitle')}
        message={t('autofill.deleteMessage')}
        cancelLabel={t('common.cancel')}
        actions={[{ label: t('autofill.deleteAddress'), destructive: true, onPress: () => void remove() }]}
      />
    </Screen>
  )
}

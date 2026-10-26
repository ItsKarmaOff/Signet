// Login detail: show / copy the username and password, edit, delete. `id=new`: new login.
import { useEffect, useState } from 'react'
import { View } from 'react-native'
import { Stack, useLocalSearchParams, useRouter } from 'expo-router'
import { hostOf, type Credential } from '@signet/shared/mobile/passwords'
import { haptic } from '@/lib/haptics'
import { useT } from '@/lib/i18n'
import { ActionSheet, Button, EmptyState, IconButton, ListRow, ListSection, Screen, TextField, Text, gutter, icons, space, toast, useColors } from '@/ui'
import { copyText } from './secretClipboard'
import { useUnlock } from './useUnlock'
import { addCredential, getCredential, removeCredential, updateCredential } from './vault'

export function PasswordDetailScreen() {
  const t = useT()
  const c = useColors()
  const router = useRouter()
  const { id } = useLocalSearchParams<{ id?: string }>()
  const isNew = id === 'new'
  const { available, unlocked, unlock } = useUnlock(t('passwords.authOpen'))
  const [cred, setCred] = useState<Credential | null>(null)
  const [missing, setMissing] = useState(false)
  const [editing, setEditing] = useState(isNew)
  const [revealed, setRevealed] = useState(false)
  const [site, setSite] = useState('')
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const [confirmDelete, setConfirmDelete] = useState(false)
  const [saving, setSaving] = useState(false)

  useEffect(() => {
    if (!unlocked || isNew || !id) return
    let alive = true
    void getCredential(id).then((found) => {
      if (!alive) return
      if (!found) return setMissing(true)
      setCred(found)
      setSite(found.origin)
      setUsername(found.username)
      setPassword(found.password)
    })
    return () => {
      alive = false
    }
  }, [unlocked, id, isNew])

  const copy = async (value: string, secret: boolean) => {
    await copyText(value, secret)
    haptic('success')
    toast.show({ message: t(secret ? 'passwords.copiedPassword' : 'passwords.copiedUsername') })
  }

  const save = async () => {
    setSaving(true)
    try {
      const ok = isNew ? !!(await addCredential({ origin: site, username, password })) : await updateCredential(id ?? '', { origin: site, username, password })
      if (!ok) return void toast.show({ message: t('passwords.saveFailed') })
      haptic('success')
      if (isNew) router.back()
      else {
        const fresh = await getCredential(id ?? '')
        if (fresh) setCred(fresh)
        setEditing(false)
      }
    } finally {
      setSaving(false)
    }
  }

  const remove = async () => {
    setConfirmDelete(false)
    await removeCredential(id ?? '')
    haptic('success')
    router.back()
    toast.show({ message: t('passwords.deleted') })
  }

  const title = isNew ? t('passwords.add') : cred ? hostOf(cred.origin) : t('passwords.title')
  const canSave = site.trim().length > 0 && password.length > 0 && !saving

  const header = (
    <Stack.Screen
      options={{
        title,
        headerRight: () =>
          unlocked && (isNew || cred) ? (
            editing ? (
              <Button title={t('passwords.saveAction')} variant="plain" disabled={!canSave} onPress={() => void save()} />
            ) : (
              <Button title={t('passwords.edit')} variant="plain" onPress={() => setEditing(true)} />
            )
          ) : null
      }}
    />
  )

  if (available === false || !unlocked) {
    return (
      <Screen>
        {header}
        <EmptyState
          icon={icons.Lock}
          title={t(available === false ? 'passwords.unavailableTitle' : 'passwords.lockedTitle')}
          message={t(available === false ? 'passwords.unavailable' : 'passwords.lockedMessage')}
          action={available === false ? undefined : { label: t('passwords.unlock'), onPress: () => void unlock() }}
        />
      </Screen>
    )
  }
  if (missing) {
    return (
      <Screen>
        {header}
        <EmptyState icon={icons.Key} title={t('passwords.notFound')} />
      </Screen>
    )
  }
  if (!isNew && !cred) return <Screen>{header}</Screen>

  return (
    <Screen scroll contentStyle={{ paddingTop: space.lg, paddingBottom: space.huge }}>
      {header}
      {editing ? (
        <View style={{ paddingHorizontal: gutter, gap: space.lg }}>
          <TextField
            label={t('passwords.website')}
            value={site}
            onChangeText={setSite}
            placeholder="example.com"
            autoCapitalize="none"
            autoCorrect={false}
            keyboardType="url"
            autoFocus={isNew}
          />
          <TextField
            label={t('passwords.username')}
            value={username}
            onChangeText={setUsername}
            autoCapitalize="none"
            autoCorrect={false}
            textContentType="username"
          />
          <TextField
            label={t('passwords.password')}
            value={password}
            onChangeText={setPassword}
            secureTextEntry={!revealed}
            autoCapitalize="none"
            autoCorrect={false}
            textContentType="password"
            right={
              <IconButton
                icon={revealed ? icons.EyeOff : icons.Eye}
                size={18}
                tone="muted"
                label={t(revealed ? 'passwords.hide' : 'passwords.show')}
                onPress={() => setRevealed((v) => !v)}
              />
            }
          />
          {!isNew ? (
            <Button
              title={t('common.cancel')}
              variant="secondary"
              onPress={() => {
                if (cred) {
                  setSite(cred.origin)
                  setUsername(cred.username)
                  setPassword(cred.password)
                }
                setEditing(false)
              }}
            />
          ) : null}
        </View>
      ) : cred ? (
        <>
          <ListSection>
            <ListRow title={t('passwords.website')} subtitle={cred.origin} />
            <ListRow
              title={t('passwords.username')}
              subtitle={cred.username || t('passwords.noUsername')}
              right={
                cred.username ? (
                  <IconButton icon={icons.Copy} size={18} tone="accent" label={t('passwords.copyUsername')} onPress={() => void copy(cred.username, false)} />
                ) : undefined
              }
            />
            <ListRow
              title={t('passwords.password')}
              subtitle={revealed ? cred.password : '••••••••••'}
              right={
                <View style={{ flexDirection: 'row' }}>
                  <IconButton
                    icon={revealed ? icons.EyeOff : icons.Eye}
                    size={18}
                    tone="accent"
                    label={t(revealed ? 'passwords.hide' : 'passwords.show')}
                    onPress={() => setRevealed((v) => !v)}
                  />
                  <IconButton icon={icons.Copy} size={18} tone="accent" label={t('passwords.copyPassword')} onPress={() => void copy(cred.password, true)} />
                </View>
              }
            />
          </ListSection>
          <ListSection footer={t('passwords.copyFooter')}>
            <ListRow title={t('passwords.delete')} icon={icons.Trash} destructive onPress={() => setConfirmDelete(true)} />
          </ListSection>
          <Text variant="footnote" color="tertiary" align="center" style={{ color: c.textTertiary }}>
            {t('passwords.added', { date: new Date(cred.createdAt).toLocaleDateString() })}
          </Text>
        </>
      ) : null}

      <ActionSheet
        visible={confirmDelete}
        onClose={() => setConfirmDelete(false)}
        title={t('passwords.deleteTitle')}
        message={t('passwords.deleteMessage')}
        cancelLabel={t('common.cancel')}
        actions={[{ label: t('passwords.delete'), destructive: true, onPress: () => void remove() }]}
      />
    </Screen>
  )
}

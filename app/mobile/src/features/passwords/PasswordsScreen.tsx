// Settings › Passwords: list protected by Face ID, search, add, CSV import and export, "never saved" sites.
import { useMemo, useState } from 'react'
import { View } from 'react-native'
import { useRouter } from 'expo-router'
import { File, Paths } from 'expo-file-system'
import * as Sharing from 'expo-sharing'
import { exportPasswordCsv, hostOf, parsePasswordCsv } from '@signet/shared/mobile/passwords'
import { useT } from '@/lib/i18n'
import { ActionSheet, EmptyState, Favicon, ListRow, ListSection, Screen, SearchField, SwitchRow, gutter, icons, space, toast } from '@/ui'
import { useStore } from '@/state'
import { authenticate } from './auth'
import { useUnlock } from './useUnlock'
import { allCredentials, importRows, useNeverList, useVaultMetas } from './vault'

const CSV_TYPES = ['text/csv', 'text/comma-separated-values', 'text/plain', 'application/vnd.ms-excel']

export function PasswordsScreen() {
  const t = useT()
  const router = useRouter()
  const offer = useStore((s) => s.settings.offerToSavePasswords)
  const setSettings = useStore((s) => s.setSettings)
  const { available, unlocked, unlock } = useUnlock(t('passwords.authOpen'))
  const metas = useVaultMetas()
  const never = useNeverList()
  const [query, setQuery] = useState('')
  const [confirmExport, setConfirmExport] = useState(false)

  const shown = useMemo(() => {
    const q = query.trim().toLowerCase()
    return [...metas]
      .filter((m) => !q || hostOf(m.origin).includes(q) || m.username.toLowerCase().includes(q))
      .sort((a, b) => hostOf(a.origin).localeCompare(hostOf(b.origin)) || a.username.localeCompare(b.username))
  }, [metas, query])

  const doImport = async () => {
    try {
      const picked = await File.pickFileAsync({ mimeTypes: CSV_TYPES })
      if (picked.canceled) return
      const rows = parsePasswordCsv(await picked.result.text())
      if (rows === null) return void toast.show({ message: t('passwords.importFormat') })
      if (rows.length === 0) return void toast.show({ message: t('passwords.importNone') })
      const { added, existing } = await importRows(rows)
      toast.show({
        message: existing ? t('passwords.importDoneExisting', { count: added, existing }) : t('passwords.importDone', { count: added }),
        duration: 6000
      })
    } catch {
      toast.show({ message: t('passwords.importFailed') })
    }
  }

  const doExport = async () => {
    setConfirmExport(false)
    let file: File | null = null
    try {
      if (!(await authenticate(t('passwords.authExport'), { fresh: true }))) return
      if (!(await Sharing.isAvailableAsync())) return void toast.show({ message: t('transfer.shareUnavailable') })
      file = new File(Paths.cache, 'Signet-passwords.csv')
      file.create({ overwrite: true })
      await file.write(exportPasswordCsv(await allCredentials()))
      await Sharing.shareAsync(file.uri, { mimeType: 'text/csv', UTI: 'public.comma-separated-values-text', dialogTitle: t('passwords.export') })
    } catch {
      toast.show({ message: t('transfer.exportFailed') })
    } finally {
      // The file contains passwords in clear text: it does not stay in the cache.
      try {
        file?.delete()
      } catch {
        /* already deleted */
      }
    }
  }

  return (
    <Screen scroll contentStyle={{ paddingTop: space.lg, paddingBottom: space.huge }}>
      <ListSection footer={t('passwords.offerFooter')}>
        <SwitchRow
          title={t('passwords.offer')}
          icon={icons.Key}
          iconColor="#f59e0b"
          value={offer}
          onValueChange={(offerToSavePasswords) => setSettings({ offerToSavePasswords })}
        />
      </ListSection>

      {available === false ? (
        <EmptyState icon={icons.Lock} title={t('passwords.unavailableTitle')} message={t('passwords.unavailable')} />
      ) : !unlocked ? (
        <EmptyState
          icon={icons.Lock}
          title={t('passwords.lockedTitle')}
          message={t('passwords.lockedMessage')}
          action={{ label: t('passwords.unlock'), onPress: () => void unlock() }}
        />
      ) : (
        <>
          {metas.length > 0 ? (
            <View style={{ paddingHorizontal: gutter, paddingBottom: space.lg }}>
              <SearchField
                value={query}
                onChangeText={setQuery}
                clearLabel={t('browser.clear')}
                placeholder={t('passwords.search')}
                accessibilityLabel={t('passwords.search')}
              />
            </View>
          ) : null}

          <ListSection footer={metas.length === 0 ? t('passwords.emptyFooter') : undefined}>
            <ListRow
              title={t('passwords.add')}
              icon={icons.Plus}
              onPress={() => router.push({ pathname: '/settings/passwords-detail', params: { id: 'new' } })}
            />
            {shown.map((m) => (
              <ListRow
                key={m.id}
                title={hostOf(m.origin)}
                subtitle={m.username || t('passwords.noUsername')}
                left={<Favicon url={m.origin} size={29} />}
                onPress={() => router.push({ pathname: '/settings/passwords-detail', params: { id: m.id } })}
              />
            ))}
          </ListSection>
          {metas.length > 0 && shown.length === 0 ? <EmptyState title={t('passwords.noResults')} /> : null}

          <ListSection>
            <ListRow
              title={t('passwords.never')}
              icon={icons.ShieldOff}
              iconColor="#8e8e93"
              value={never.length ? String(never.length) : undefined}
              onPress={() => router.push('/settings/passwords-never')}
            />
          </ListSection>

          <ListSection title={t('passwords.transfer')} footer={t('passwords.transferFooter')}>
            <ListRow
              title={t('passwords.import')}
              subtitle={t('passwords.importHint')}
              icon={icons.Upload}
              iconColor="#34a56f"
              onPress={() => void doImport()}
            />
            <ListRow
              title={t('passwords.export')}
              subtitle={t('passwords.exportHint')}
              icon={icons.Download}
              iconColor="#3a82f7"
              disabled={metas.length === 0}
              onPress={() => setConfirmExport(true)}
            />
          </ListSection>
        </>
      )}

      <ActionSheet
        visible={confirmExport}
        onClose={() => setConfirmExport(false)}
        title={t('passwords.exportWarningTitle')}
        message={t('passwords.exportWarning')}
        cancelLabel={t('common.cancel')}
        actions={[{ label: t('passwords.exportAction'), destructive: true, onPress: () => void doExport() }]}
      />
    </Screen>
  )
}

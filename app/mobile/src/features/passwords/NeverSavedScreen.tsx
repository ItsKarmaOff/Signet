// Sites for which saving a password is never offered.
import { hostOf } from '@signet/shared/mobile/passwords'
import { useT } from '@/lib/i18n'
import { EmptyState, Favicon, IconButton, ListRow, ListSection, Screen, icons, space } from '@/ui'
import { allowSavingFor, useNeverList } from './vault'

export function NeverSavedScreen() {
  const t = useT()
  const never = useNeverList()
  if (never.length === 0) {
    return (
      <Screen>
        <EmptyState icon={icons.ShieldOff} title={t('passwords.neverEmptyTitle')} message={t('passwords.neverEmpty')} />
      </Screen>
    )
  }
  return (
    <Screen scroll contentStyle={{ paddingTop: space.lg, paddingBottom: space.huge }}>
      <ListSection footer={t('passwords.neverFooter')}>
        {[...never].sort().map((origin) => (
          <ListRow
            key={origin}
            title={hostOf(origin)}
            left={<Favicon url={origin} size={29} />}
            right={
              <IconButton
                icon={icons.X}
                size={18}
                tone="muted"
                label={t('passwords.neverRemove', { site: hostOf(origin) })}
                onPress={() => void allowSavingFor(origin)}
              />
            }
          />
        ))}
      </ListSection>
    </Screen>
  )
}

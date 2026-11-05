// Sites where saving addresses and cards is never offered.
import { hostOf } from '@signet/shared/mobile/passwords'
import { useT } from '@/lib/i18n'
import { EmptyState, Favicon, IconButton, ListRow, ListSection, Screen, icons, space } from '@/ui'
import { allowOfferFor, useAutofillNever } from './store'

export function AutofillNeverScreen() {
  const t = useT()
  const never = useAutofillNever()
  if (never.length === 0) {
    return (
      <Screen>
        <EmptyState icon={icons.ShieldOff} title={t('autofill.neverEmptyTitle')} message={t('autofill.neverEmpty')} />
      </Screen>
    )
  }
  return (
    <Screen scroll contentStyle={{ paddingTop: space.lg, paddingBottom: space.huge }}>
      <ListSection footer={t('autofill.neverFooter')}>
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
                label={t('autofill.neverRemove', { site: hostOf(origin) })}
                onPress={() => void allowOfferFor(origin)}
              />
            }
          />
        ))}
      </ListSection>
    </Screen>
  )
}

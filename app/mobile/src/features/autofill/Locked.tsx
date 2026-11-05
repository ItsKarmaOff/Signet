import { useT } from '@/lib/i18n'
import { EmptyState, icons } from '@/ui'
import type { AutofillUnlock } from './useAutofillUnlock'

/** Placeholder of a screen that is locked (Face ID pending) or whose storage is unavailable. */
export function Locked({ available, unlock }: Pick<AutofillUnlock, 'available' | 'unlock'>) {
  const t = useT()
  return available === false ? (
    <EmptyState icon={icons.Lock} title={t('autofill.unavailableTitle')} message={t('autofill.unavailable')} />
  ) : (
    <EmptyState
      icon={icons.Lock}
      title={t('autofill.lockedTitle')}
      message={t('autofill.lockedMessage')}
      action={{ label: t('autofill.unlock'), onPress: () => void unlock() }}
    />
  )
}

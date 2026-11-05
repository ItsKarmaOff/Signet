// Labels for saved items, shared by the autofill screens.
import { formatExpiry, isExpired, NETWORK_NAMES } from '@signet/shared/autofill/card'
import { fullName } from '@signet/shared/autofill/values'
import type { TFunction } from '@/lib/i18n'
import { AutofillError, type AutofillAddress, type AutofillCard } from './store'

/** "Visa •••• 4242", or "Card •••• 4242" when the network is unknown. */
export const cardLabel = (t: TFunction, c: Pick<AutofillCard, 'network' | 'last4'>): string =>
  c.network === 'unknown' ? t('autofill.cardLabelPlain', { last4: c.last4 }) : t('autofill.cardLabel', { network: NETWORK_NAMES[c.network], last4: c.last4 })

export const addressTitle = (t: TFunction, a: AutofillAddress): string => fullName(a) || a.street.split('\n')[0] || t('autofill.noName')

export const addressSubtitle = (a: AutofillAddress): string | undefined => (fullName(a) ? a.street.split('\n')[0] : a.city) || undefined

/** "Expires 08/27" or "Expired 08/27". */
export const expiryText = (t: TFunction, c: Pick<AutofillCard, 'expMonth' | 'expYear'>): string =>
  t(isExpired(c.expMonth, c.expYear) ? 'autofill.expired' : 'autofill.expires', { date: formatExpiry(c.expMonth, c.expYear) })

/** Translated message of a failed save. */
export const errorMessage = (t: TFunction, err: unknown): string => {
  const code = err instanceof AutofillError ? err.code : 'failed'
  switch (code) {
    case 'unavailable':
      return t('autofill.error.unavailable')
    case 'empty':
      return t('autofill.error.empty')
    case 'missing':
      return t('autofill.error.missing')
    case 'number':
      return t('autofill.error.number')
    case 'expiry':
      return t('autofill.error.expiry')
    case 'expired':
      return t('autofill.error.expired')
    case 'duplicateCard':
      return t('autofill.error.duplicateCard')
    case 'duplicateAddress':
      return t('autofill.error.duplicateAddress')
    default:
      return t('autofill.error.failed')
  }
}

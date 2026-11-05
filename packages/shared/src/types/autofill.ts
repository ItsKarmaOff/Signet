/** Card networks recognised from the number prefix. */
export type CardNetwork = 'visa' | 'mastercard' | 'amex' | 'discover' | 'diners' | 'jcb' | 'unionpay' | 'maestro' | 'unknown'

/** Saved address, as Settings and the page menu see it. */
export interface AutofillAddress {
  id: string
  /** First name; either part may be empty. */
  givenName: string
  familyName: string
  organization: string
  /** Street, one line per row. */
  street: string
  city: string
  postalCode: string
  /** Region or state (code or name as typed). */
  region: string
  /** ISO 3166-1 alpha-2 code, or empty. */
  country: string
  phone: string
  email: string
  createdAt: number
  updatedAt: number
  useCount: number
  lastUsedAt?: number
}

/** Fields of an address a form or Settings can set. */
export type AddressInput = Pick<
  AutofillAddress,
  'givenName' | 'familyName' | 'organization' | 'street' | 'city' | 'postalCode' | 'region' | 'country' | 'phone' | 'email'
>

/** Saved card, as Settings sees it: never the number nor the CVC. */
export interface AutofillCard {
  id: string
  holder: string
  /** Last four digits, kept in clear for display. */
  last4: string
  network: CardNetwork
  expMonth: number
  expYear: number
  nickname: string
  createdAt: number
  updatedAt: number
  useCount: number
  lastUsedAt?: number
}

/** Card as entered in Settings; `number` null = unchanged. */
export interface CardInput {
  holder: string
  number: string | null
  expMonth: number
  expYear: number
  nickname: string
}

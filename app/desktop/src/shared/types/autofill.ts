import type { AutofillAddress, AutofillCard } from '@signet/shared/types'

/** State of the Settings Autofill section (`autofill:list`). */
export interface AutofillInfo {
  addresses: AutofillAddress[]
  cards: AutofillCard[]
  /** False if the system cannot encrypt (no card can be saved). */
  encryption: boolean
}

/** Result of a save from Settings: error message, or null. */
export type AutofillSaveResult = string | null

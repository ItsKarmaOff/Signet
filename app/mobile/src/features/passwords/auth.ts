// Face ID / Touch ID / device passcode before showing, filling, copying or exporting a password.
import * as LocalAuthentication from 'expo-local-authentication'
import { t } from '@/lib/i18n'

/** A successful authentication is valid for the following minute (filling two fields, opening a detail). */
const GRACE_MS = 60_000
let until = 0

/**
 * Requests device authentication. True if it succeeds.
 * Device with no lock (neither Face ID nor passcode): there is nothing to ask, we let it through; the Keychain itself is not readable
 * outside the app, and the system setting is the user's responsibility.
 */
export async function authenticate(reason: string, { fresh = false }: { fresh?: boolean } = {}): Promise<boolean> {
  if (!fresh && Date.now() < until) return true
  try {
    const level = await LocalAuthentication.getEnrolledLevelAsync()
    if (level === LocalAuthentication.SecurityLevel.NONE) return true
    const res = await LocalAuthentication.authenticateAsync({ promptMessage: reason, cancelLabel: t('common.cancel') })
    if (res.success) until = Date.now() + GRACE_MS
    return res.success
  } catch {
    // Native module missing or failing: we refuse, a password is never revealed without verification.
    return false
  }
}

/** Forgets the recent authentication (for example when leaving the passwords screen). */
export const forgetAuthentication = (): void => {
  until = 0
}

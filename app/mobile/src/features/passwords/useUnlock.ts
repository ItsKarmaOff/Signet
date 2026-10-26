import { useCallback, useEffect, useState } from 'react'
import { AppState } from 'react-native'
import { authenticate, forgetAuthentication } from './auth'
import { loadVault, vaultAvailable } from './vault'

export interface Unlock {
  /** `null` while unknown whether the Keychain is usable. */
  available: boolean | null
  unlocked: boolean
  unlock: () => Promise<void>
}

/** Lock for a passwords screen: Face ID on open, and again after a trip to the background. */
export function useUnlock(reason: string): Unlock {
  const [available, setAvailable] = useState<boolean | null>(null)
  const [unlocked, setUnlocked] = useState(false)

  const unlock = useCallback(async () => {
    if (!(await vaultAvailable())) return setAvailable(false)
    setAvailable(true)
    if (await authenticate(reason)) {
      await loadVault()
      setUnlocked(true)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps -- the reason does not change the logic
  }, [])

  useEffect(() => {
    void unlock()
    const sub = AppState.addEventListener('change', (state) => {
      if (state === 'background') {
        forgetAuthentication()
        setUnlocked(false)
      }
    })
    return () => sub.remove()
  }, [unlock])

  return { available, unlocked, unlock }
}

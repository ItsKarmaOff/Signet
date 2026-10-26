import { useEffect, useState } from 'react'
import * as LocalAuthentication from 'expo-local-authentication'

/** Can the device lock with Face ID / Touch ID / passcode? `null` until we know. */
export function useBiometrics(): boolean | null {
  const [available, setAvailable] = useState<boolean | null>(null)
  useEffect(() => {
    let alive = true
    void (async () => {
      try {
        const ok = (await LocalAuthentication.hasHardwareAsync()) && (await LocalAuthentication.isEnrolledAsync())
        if (alive) setAvailable(ok)
      } catch {
        if (alive) setAvailable(false)
      }
    })()
    return () => {
      alive = false
    }
  }, [])
  return available
}

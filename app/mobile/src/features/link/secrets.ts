// Link secrets: each computer's token lives in the iOS Keychain / Android Keystore (expo-secure-store), never in
// AsyncStorage. The phone's identifier is not a secret: AsyncStorage.
import AsyncStorage from '@react-native-async-storage/async-storage'
import * as SecureStore from 'expo-secure-store'
import { LINK_TOKEN_KEY_PREFIX, PHONE_ID_KEY } from '@signet/shared/mobile/link'
import { newId } from '@signet/shared/ids'

const OPTIONS: SecureStore.SecureStoreOptions = { keychainAccessible: SecureStore.WHEN_UNLOCKED_THIS_DEVICE_ONLY }
const cache = new Map<string, string>()

export async function getToken(linkId: string): Promise<string | null> {
  const known = cache.get(linkId)
  if (known) return known
  try {
    const token = await SecureStore.getItemAsync(LINK_TOKEN_KEY_PREFIX + linkId, OPTIONS)
    if (token) cache.set(linkId, token)
    return token
  } catch {
    return null
  }
}

export async function setToken(linkId: string, token: string): Promise<void> {
  await SecureStore.setItemAsync(LINK_TOKEN_KEY_PREFIX + linkId, token, OPTIONS)
  cache.set(linkId, token)
}

export async function deleteToken(linkId: string): Promise<void> {
  cache.delete(linkId)
  await SecureStore.deleteItemAsync(LINK_TOKEN_KEY_PREFIX + linkId, OPTIONS).catch(() => undefined)
}

let phoneId: Promise<string> | null = null

/** Stable identifier of this installation, created at first pairing. */
export function getPhoneId(): Promise<string> {
  phoneId ??= (async () => {
    try {
      const stored = await AsyncStorage.getItem(PHONE_ID_KEY)
      if (stored) return stored
    } catch {
      // unreadable: create a new one
    }
    const id = newId()
    await AsyncStorage.setItem(PHONE_ID_KEY, id).catch(() => undefined)
    return id
  })()
  return phoneId
}

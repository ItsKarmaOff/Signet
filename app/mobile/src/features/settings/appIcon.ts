// App icon (device home screen). The native module `expo-alternate-app-icons` only exists in the installed app
// (native build): it is absent in Expo Go. We detect it without ever importing it cold (the import would throw).
import { requireOptionalNativeModule } from 'expo'
import { APP_ICON_IDS, DEFAULT_APP_ICON, type AppIconId } from '@signet/shared/mobile/settings'

type Native = typeof import('expo-alternate-app-icons')

/** `aurore` is the binary's default icon (`assets/icon.png`): it is not an alternate icon. */
const nativeName = (id: AppIconId): string | null => (id === DEFAULT_APP_ICON ? null : id.charAt(0).toUpperCase() + id.slice(1))
const idOf = (name: string | null): AppIconId => APP_ICON_IDS.find((id) => nativeName(id) === name) ?? DEFAULT_APP_ICON

let cached: Native | null | undefined
function native(): Native | null {
  if (cached !== undefined) return cached
  try {
    // Absent in Expo Go and on web: the package is only loaded if its native module is present.
    // eslint-disable-next-line @typescript-eslint/no-require-imports -- chargement conditionnel voulu
    cached = requireOptionalNativeModule('ExpoAlternateAppIcons') ? (require('expo-alternate-app-icons') as Native) : null
  } catch {
    cached = null
  }
  return cached
}

export type AppIconSupport = 'ready' | 'unsupported' | 'unavailable'

/** `unavailable`: module missing (Expo Go); `unsupported`: module present but the device refuses; `ready` otherwise. */
export function appIconSupport(): AppIconSupport {
  const m = native()
  if (!m) return 'unavailable'
  try {
    return m.supportsAlternateIcons ? 'ready' : 'unsupported'
  } catch {
    return 'unsupported'
  }
}

/** Icon actually displayed by the OS, or `null` if it cannot be read. */
export function currentAppIcon(): AppIconId | null {
  const m = native()
  if (!m) return null
  try {
    return idOf(m.getAppIconName())
  } catch {
    return null
  }
}

/** Changes the icon (iOS shows its own alert). Throws if the system refuses. */
export async function applyAppIcon(id: AppIconId): Promise<void> {
  const m = native()
  if (!m) throw new Error('Alternate app icons are unavailable')
  await m.setAlternateAppIcon(nativeName(id))
}

// "Passwords" feature: local vault (Keychain + Face ID), capture and filling in pages, management screens.
// WebView bridge: `usePasswordBridge` (used by BrowserView). Screens: Settings › Passwords (app/settings/passwords*.tsx).
export { usePasswordBridge } from './usePasswordBridge'
export { PasswordsScreen } from './PasswordsScreen'
export { PasswordDetailScreen } from './PasswordDetailScreen'
export { NeverSavedScreen } from './NeverSavedScreen'

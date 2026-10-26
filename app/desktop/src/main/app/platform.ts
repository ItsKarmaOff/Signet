import { app, nativeTheme, type BrowserWindowConstructorOptions } from 'electron'

/** Signet is designed for macOS; elsewhere (Linux), windows keep the system frame. */
export const IS_MAC = process.platform === 'darwin'

/**
 * Title bar of a window. Mac: hidden, traffic lights placed over the UI, translucent background (`vibrancy`
 * and transparent background). Elsewhere: the system frame and buttons, menu bar hidden (Alt shows it), an
 * opaque background in the theme (no translucency, see appearance.ts).
 */
export function windowChrome(options: {
  lights?: { x: number; y: number }
  /** Translucent background (Mac); otherwise `background` as is. */
  vibrancy?: boolean
  background?: string
}): Partial<BrowserWindowConstructorOptions> {
  if (IS_MAC)
    return {
      titleBarStyle: 'hidden',
      ...(options.lights && { trafficLightPosition: options.lights }),
      ...(options.vibrancy ? { vibrancy: 'sidebar', visualEffectState: 'active', backgroundColor: '#00000000' } : { backgroundColor: options.background })
    }
  return {
    autoHideMenuBar: true,
    backgroundColor: options.vibrancy ? (nativeTheme.shouldUseDarkColors ? '#1e1e20' : '#ececef') : options.background
  }
}

/** Makes the app the system's default browser (http and https links). */
export function setDefaultBrowser(): boolean {
  return app.setAsDefaultProtocolClient('http') && app.setAsDefaultProtocolClient('https')
}

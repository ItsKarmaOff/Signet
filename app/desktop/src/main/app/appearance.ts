import { nativeTheme, webContents } from 'electron'
import { IS_MAC } from './platform'
import type { Appearance, Settings } from '@shared/types'
import { isUiContents } from './i18n'
import { pushUi } from '../ipc/push'

/** Appearance settings read by the UI views; missing or unknown values = default. */
export function appearanceOf(s: Settings): Appearance {
  return {
    tint: s.tint === 'subtle' || s.tint === 'strong' ? s.tint : 'normal',
    density: s.density === 'compact' || s.density === 'roomy' ? s.density : 'normal',
    text: s.textSize === 'small' || s.textSize === 'large' ? s.textSize : 'normal',
    // Translucency: only on Mac (vibrancy); elsewhere backgrounds stay opaque.
    glass: s.transparency !== false && IS_MAC
  }
}

/** Forced light or dark theme, or macOS's: applies to the UI and to pages (prefers-color-scheme). */
export function applyTheme(s: Settings): void {
  const next = s.theme === 'light' || s.theme === 'dark' ? s.theme : 'system'
  if (nativeTheme.themeSource !== next) nativeTheme.themeSource = next
}

/** Pushes the appearance to all UI views on the `appearance` channel. */
export function broadcastAppearance(s: Settings): void {
  const a = appearanceOf(s)
  for (const wc of webContents.getAllWebContents()) if (isUiContents(wc)) pushUi(wc, 'appearance', a)
}

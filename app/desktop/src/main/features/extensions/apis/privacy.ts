import { webContents, type Session } from 'electron'
import { PRIVACY_SETTINGS } from '@shared/extension-apis'
import { ApiError } from './types'
import type { SettingDef } from './settings'

const WEBRTC_POLICIES = ['default', 'default_public_and_private_interfaces', 'default_public_interface_only', 'disable_non_proxied_udp'] as const
type WebRtcPolicy = (typeof WEBRTC_POLICIES)[number]

/** Chrome's policy -> Electron's `webContents.setWebRTCIPHandlingPolicy` (same four names). */
function applyWebRtc(ses: Session, value: unknown): void {
  for (const wc of webContents.getAllWebContents()) if (!wc.isDestroyed() && wc.session === ses) wc.setWebRTCIPHandlingPolicy(value as WebRtcPolicy)
}

/** Applies the policy to a webContents created after the setting (tabs opened later). */
export function applyWebRtcTo(wc: Electron.WebContents, value: unknown): void {
  if (!wc.isDestroyed()) wc.setWebRTCIPHandlingPolicy(value as WebRtcPolicy)
}

export const WEBRTC_KEY = 'privacy.network.webRTCIPHandlingPolicy'

/**
 * `chrome.privacy.*` settings. Only `network.webRTCIPHandlingPolicy` does something. The others (see PRIVACY_SETTINGS:
 * networkPredictionEnabled, the deprecated webRTC booleans, every `services.*` and `websites.*` switch) store and
 * return their value so an extension that reads or writes them keeps working; Chromium offers no hook to honour them.
 */
export function privacySettings(): SettingDef[] {
  const defs: SettingDef[] = []
  for (const [ns, list] of Object.entries(PRIVACY_SETTINGS)) {
    for (const [name, defaultValue] of Object.entries(list)) {
      const key = `privacy.${ns}.${name}`
      if (key === WEBRTC_KEY) {
        defs.push({
          key,
          permission: 'privacy',
          defaultValue,
          parse: (v) => {
            if (!WEBRTC_POLICIES.includes(v as WebRtcPolicy)) {
              throw new ApiError(`Invalid value for argument 1. Property 'value': Value must be one of: [${WEBRTC_POLICIES.join(', ')}].`)
            }
            return v
          },
          apply: applyWebRtc
        })
      } else {
        defs.push({
          key,
          permission: 'privacy',
          defaultValue,
          parse: (v) => {
            if (typeof v !== typeof defaultValue) throw new ApiError(`Invalid value for argument 1. Property 'value': Expected '${typeof defaultValue}'.`)
            return v
          }
        })
      }
    }
  }
  return defs
}

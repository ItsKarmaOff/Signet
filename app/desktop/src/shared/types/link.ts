import type { LinkDevice } from '@signet/shared/link/protocol'
import type { LinkStatus } from '@signet/shared/link/status'

/** A paired phone, as Settings and menus see it (never a key or token). */
export interface LinkDeviceInfo {
  linkId: string
  device: LinkDevice
  /** Switch for this device, on this computer. */
  enabled: boolean
  status: LinkStatus
  createdAt: number
  /** Last control connection (ms since 1970), or null. */
  lastSeen: number | null
}

/** Pairing QR code shown in Settings. */
export interface LinkPairing {
  /** SVG image of the QR code, as a data URL. */
  qr: string
  /** Expiry time (ms since 1970). */
  expiresAt: number
}

/** Everything the UI knows about the link between devices. Pushed on `link:state`. */
export interface LinkState {
  /** Master switch ("Sync"). */
  general: boolean
  /** Name of this computer, as phones see it. */
  name: string
  devices: LinkDeviceInfo[]
  /** Number of phones whose link is active on both sides (toolbar indicator). */
  active: number
  /** Current QR code, or null. */
  pairing: LinkPairing | null
  /** True if pairing failed because it could not listen on the network. */
  listenError: boolean
}

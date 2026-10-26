// Switch rules and displayed state of a link.
//
// Each side has a master switch ("Sync") and one switch per device. A link only works if all four
// are ON (master + device, on each side). Turning off never deletes the link: the keys stay.
// When the master switch is off, the device switches keep their position (shown greyed out) and resume it when it is back on.

export interface LinkToggles {
  /** Master switch of this device. */
  general: boolean
  /** Switch of this link, on this device. */
  device: boolean
}

/** This link is allowed on this side (both local switches ON). */
export const enabledHere = (t: LinkToggles): boolean => t.general && t.device

export type LinkStatus =
  | 'connected' // everything is ON and the control connection is open
  | 'connecting' // allowed here, searching or connecting
  | 'paused-here' // a switch on this device is OFF
  | 'paused-there' // the other device said it was paused (last known state)
  | 'offline' // allowed here, the other device cannot be found
  | 'invalid' // the other device no longer knows this link: delete it here too

export interface LinkStatusInput extends LinkToggles {
  /** The control connection is open (after `welcome`). */
  connected: boolean
  /** A search or a connection is in progress. */
  connecting?: boolean
  /** Last `paused` received from the other device (`welcome` or `status`), kept after disconnection. */
  peerPaused: boolean
  /** The other device answered `unknown-link` / `bad-token`, or sent `forget`. */
  invalid: boolean
}

export function linkStatus(s: LinkStatusInput): LinkStatus {
  if (s.invalid) return 'invalid'
  if (!enabledHere(s)) return 'paused-here'
  if (s.peerPaused) return 'paused-there'
  if (s.connected) return 'connected'
  return s.connecting ? 'connecting' : 'offline'
}

/** The link is usable now (localhost tunnel, "open on…", server list). */
export const linkActive = (s: LinkStatusInput): boolean => linkStatus(s) === 'connected'

/**
 * Computer that receives the phone's `localhost` addresses.
 * - aucun ordinateur actif : null ;
 * - the previous choice is still active: keep it;
 * - only one active: that one;
 * - several and no valid choice: 'ask' (the phone shows a window to choose).
 */
export function pickLocalhostRoute(activeLinkIds: readonly string[], chosen: string | null): string | 'ask' | null {
  if (chosen && activeLinkIds.includes(chosen)) return chosen
  if (activeLinkIds.length === 0) return null
  if (activeLinkIds.length === 1) return activeLinkIds[0]!
  return 'ask'
}

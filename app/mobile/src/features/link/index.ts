// "Link with computers" feature: pairing by QR code, connection on the local network, the computer's `localhost` on the
// phone, "open on…". Routes: app/settings/computers|pair|pair-confirm, app/link (QR code link). Pure logic:
// @signet/shared/mobile/link ; protocole : @signet/shared/link/*. Module natif : modules/signet-link.
export { ComputersScreen } from './ComputersScreen'
export { PairScannerScreen } from './PairScannerScreen'
export { PairConfirmScreen } from './PairConfirmScreen'
export { DevServers } from './DevServers'
export { LinkHost } from './LinkHost'
export { useLinkManager } from './useLinkManager'
export { useComputers, useActiveComputers, type Computer } from './hooks'
export { resolveLocalhostUrl, askActiveComputer, openDevServerUrl, useLocalhostAttach } from './localhost'
export { forgetLink, pairWithOffer, sendOpenToComputer } from './manager'
export { useLinkRuntime } from './runtime'
export { freezeLinkManager } from './manager'
export { patchRuntime } from './runtime'

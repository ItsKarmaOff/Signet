// Local `signet-link` module: transport for the link with the computer (pinned TLS sockets, mDNS, WebView localhost relay).
// Nothing crashes without the module (Expo Go): `SignetLink` is then `null`.
export {
  SignetLink,
  type SignetLinkEvents,
  type SignetLinkNative,
  type LocalhostRoute,
  type PinnedEndpoint,
  type ServiceFound,
  type SocketError
} from './src/SignetLinkModule'

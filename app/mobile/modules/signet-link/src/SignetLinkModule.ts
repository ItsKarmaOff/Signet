import { NativeModule, requireOptionalNativeModule } from 'expo'

// Native `SignetLink` module (iOS + Android): the transport for the link with the computer. The protocol (messages, QR code, switch
// rules) is in `@signet/shared/link/*` and runs in JavaScript; this module only does what JS cannot:
// pinned-certificate TLS sockets, mDNS discovery, localhost relay for the WebView. Detailed contract: ../README.md.

/** Where to reach the computer for a link, and how to verify it is the right one. */
export interface PinnedEndpoint {
  /** IP address (v4 or v6, possibly with `%interface`) or hostname. */
  host: string
  port: number
  /** Name sent in SNI: `serverNameForLink(linkId)`. */
  serverName: string
  /** SHA-256 of the expected DER certificate, base64url without `=` (43 characters). Any other certificate is rejected. */
  certSha256: string
}

/** Computer to which the WebView's `localhost` addresses are sent. */
export interface LocalhostRoute {
  /** Addresses to try in order (the first to answer wins, and is then used first). */
  endpoints: PinnedEndpoint[]
  linkId: string
  token: string
  /** Relayed ports: the module listens on the phone's 127.0.0.1 and ::1 at these ports, and passes each connection through a tunnel. */
  ports: number[]
}

/** Error codes of `open` and `onSocketClosed`. */
export type SocketError = 'timeout' | 'refused' | 'unreachable' | 'pin-mismatch' | 'tls' | 'closed' | 'line-too-long' | 'io'

export interface ServiceFound {
  /** mDNS instance name (the computer's name). */
  name: string
  /** TXT `id`: the computer's identifier (`LinkDevice.id`). */
  desktopId: string
  /** TXT `v`: protocol version. */
  version: number
  /** Resolved addresses (IPv4 first). */
  hosts: string[]
  port: number
}

export type SignetLinkEvents = {
  /** A received line (without the trailing `\n`). */
  onSocketLine: (e: { id: string; line: string }) => void
  /** The socket is closed (by the other side, by `close`, or on error). Sent only once per socket. */
  onSocketClosed: (e: { id: string; error?: SocketError }) => void
  onServiceFound: (e: ServiceFound) => void
  onServiceLost: (e: { name: string }) => void
  /** The relay could not open a tunnel (to show a message). `code` = protocol error code or SocketError. */
  onTunnelError: (e: { port: number; code: string }) => void
}

declare class SignetLinkModuleType extends NativeModule<SignetLinkEvents> {
  /** Opens a pinned TLS socket. Resolves once the TLS handshake is done and the certificate verified. Rejects with a SocketError. */
  open(id: string, endpoint: PinnedEndpoint, timeoutMs: number): Promise<void>
  /** Writes a line (the module appends `\n`). */
  writeLine(id: string, line: string): Promise<void>
  /** Closes the socket (without error). No effect if it is already closed. */
  close(id: string): void

  /** mDNS discovery of `_signet._tcp`. Services already seen are returned on each `startBrowsing`. */
  startBrowsing(): void
  stopBrowsing(): void

  /** True if `localhost` can go through the computer (iOS: always, mirror ports; Android: PROXY_OVERRIDE + REVERSE_BYPASS). */
  isLocalhostRoutingSupported(): boolean
  /** Sends `localhost` to this computer (null = stop: `localhost` becomes the phone again). Resolves false if unsupported. */
  setLocalhostRoute(route: LocalhostRoute | null): Promise<boolean>
  /** No effect today (iOS: mirror ports serve every WebView; Android: the proxy applies to the whole app). */
  attachToWebView(viewTag: number): Promise<boolean>

  /** Device name for the computer ("Christophe's iPhone", model name on Android). */
  deviceName(): string
}

/** Null in Expo Go, or if the native module is not compiled. */
export const SignetLink = requireOptionalNativeModule<SignetLinkModuleType>('SignetLink')
export type SignetLinkNative = SignetLinkModuleType

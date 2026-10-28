# signet-link (native module)

Transport for the phone ↔ computer link. Protocol and rules: `packages/shared/src/link/README.md`. JS interface:
`src/SignetLinkModule.ts` (this is the contract: iOS and Android implement it identically).

## Pinned TLS sockets (`open`, `writeLine`, `close`, `onSocketLine`, `onSocketClosed`)

- TLS 1.2+ to `host:port`, SNI = `serverName`. Standard verification is **replaced** by: SHA-256 of the leaf certificate (DER)
  in base64url == `certSha256`. Otherwise: rejected with `pin-mismatch`.
  - iOS: Network.framework, `NWProtocolTLS.Options` + `sec_protocol_options_set_verify_block` + `sec_protocol_options_set_tls_server_name`.
  - Android: `SSLSocket` (SSLContext with an X509TrustManager that compares the fingerprint), SNI via `SSLParameters.serverNames`.
- `open` resolves after the handshake; rejects (`timeout`, `refused`, `unreachable`, `pin-mismatch`, `tls`) otherwise.
- Reads line by line (`\n`), UTF-8; a line longer than 64 KiB closes the socket (`line-too-long`).
- `onSocketClosed` is emitted only once per socket, with `error` except on a normal close or `close()`.
- All networking off the main thread; events emitted to JS without blocking.

## mDNS (`startBrowsing`, `stopBrowsing`, `onServiceFound`, `onServiceLost`)

- Type `_signet._tcp`, domain `local.`. For each service: resolve addresses + port + TXT (`v`, `id`).
- iOS: `NWBrowser` (bonjourWithTXTRecord) then resolution (NWConnection to the service endpoint, or `NetService`), Info.plist
  `NSBonjourServices` and `NSLocalNetworkUsageDescription` (already in app.json). The first search triggers the "Local Network" alert.
- Android: `NsdManager` (discoverServices + resolveService / registerServiceInfoCallback on API 34+), `WifiManager.MulticastLock`
  during the search.

## Localhost relay (`setLocalhostRoute`, `attachToWebView`, `isLocalhostRoutingSupported`, `onTunnelError`)

Opening the tunnel (shared): pinned socket to the first endpoint of the route that answers (`endpoints`, in order; remember the one
that worked), line `{"type":"hello","v":1,"linkId","token","role":"tunnel","port":PORT}`, wait for one line. `{"type":"ok"}` →
relay the bytes. Otherwise, answer `HTTP/1.1 502 Bad Gateway` with a small text page, and emit `onTunnelError { port, code }`.

### iOS: mirror ports

**On a real iPhone, iOS never sends `localhost` to a proxy** (`WKWebsiteDataStore.proxyConfigurations` is ignored for it;
checked on an iPhone running iOS 27: `example.com` reaches the proxy, `localhost` never does). The simulator, which uses the Mac's network,
does it: so it proves nothing for this case.

So the module listens itself on the phone's localhost (`127.0.0.1` and `::1`), on the ports in `route.ports` (all the ports that
listen on the computer, plus those opened by hand), and passes each connection as is through a tunnel to the same port on
the computer. The WebView opens `http://localhost:5173` without knowing anything: `localhost` origin, secure context, WebSocket, calls to
other ports (API) included, private tabs too (`attachToWebView` does nothing). A port already taken on the phone is ignored.
The listeners are restarted when the app comes back to the foreground (iOS can reclaim them during sleep). In the simulator, nothing
is relayed: it shares the Mac's localhost (the relay would call itself) and `localhost` there already leads to the computer.
Log: subsystem `dev.signet.link`, or standard output with `SIGNET_LINK_DEBUG=1`
(`DEVICECTL_CHILD_SIGNET_LINK_DEBUG=1 xcrun devicectl device process launch --console --device <udid> dev.signet.mobile`).

### Android: mirror ports too (never tested on a device)

Same principle as iOS: a `ServerSocket` on `127.0.0.1` and `::1` for each port in `route.ports`, each connection relayed as
is through a tunnel. The Android emulator has its own localhost: the relay works there as on a phone. `http://` traffic
(localhost included) is allowed by `expo-build-properties` (`android.usesCleartextTraffic`).

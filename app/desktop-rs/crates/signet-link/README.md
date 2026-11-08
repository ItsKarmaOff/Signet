# signet-link

Phone link and passkeys over a phone, as a protocol layer with no UI and no CEF. Port of
`app/desktop/src/main/features/link`, `packages/shared/src/link`, `app/desktop/src/main/web/hybrid`,
`webauthn.ts`, `webauthn-request.ts` and the logic of `windows/passkey.ts`. The phone app (`app/mobile`) talks to this
crate exactly as it talks to the Electron app.

```
signet_link::link      phone link: protocol, store, TLS service, mDNS, dev servers, LinkManager (sync API)
signet_link::hybrid    passkeys: cbor, eid, qr, noise, tunnel, ctap, webauthn_request, ble, session (HybridManager)
signet_link::qrimage   QR code as a module matrix and SVG
```

Test with `CARGO_TARGET_DIR=target-link cargo test -p signet-link` (loopback only, no phone, no Bluetooth).

## 1. Phone link

### Protocol summary (unchanged from Electron)

- The computer announces `_signet._tcp` over mDNS (instance = computer name, TXT `v=1`, `id=<desktopId>`) and listens with
  TLS on all interfaces, port 47615 if free, else a free port. TLS 1.2+, one self-signed ECDSA P-256 certificate per link,
  plus a default certificate for unknown names. The certificate is chosen by SNI `<linkId>.link.signet.invalid`.
- The phone pins the SHA-256 of the DER certificate (base64url, 43 characters) received in the QR code. The
  QR text is `signet://link?d=v~linkId~certSha256~code~port~expiry36~hosts~nameB64`.
- One TLS connection = one role, JSON lines (`\n`, 64 KiB max). First line from the phone: `pair`, `hello` role `control`, or
  `hello` role `tunnel` (+ port). Replies: `paired` (with the 32-byte link token), `welcome`, `ok`, or `error {code}`.
  Control messages: `status`, `servers`, `open`, `forget`, `ping`/`pong`. A tunnel relays raw bytes to
  `127.0.0.1:port` then `[::1]:port`, never to the link's own port.
- Pairing code: 16 random bytes, single use, 5 minutes, 5 wrong attempts cancel it. The token is only kept as a SHA-256.
  Re-pairing the same phone replaces its link. The four switches (general and device, on both sides) all have to be ON.
- All of it is in `link::protocol` (messages, QR), `link::service` (server), `link::status` (switch rules).
  The wire JSON is the same as `packages/shared/src/link/protocol.ts` (field names `linkId`, `certSha256`, `fromTab`...).

### API (`LinkManager`, the thing the app uses)

```rust
let (link, events) = LinkManager::start(LinkConfig {
    data_dir,                 // folder the app passes in; link.json lives there
    vault: Some(Box::new(my_keychain)), // impl signet_link::Vault; None = keys stored as `raw:` text
    port: None,               // 47615 by default
    announce: true,           // mDNS
});
link.state() -> LinkState                  // general, name, devices[], active, pairing{url, expires_at, qr}, listen_error
link.set_general(bool); link.set_device(link_id, bool); link.forget(link_id);
link.start_pairing() -> bool;  link.cancel_pairing();   // state().pairing.qr is a QrMatrix (draw it) ; .url is the text
link.set_tabs(Vec<TabLike>);               // every open tab {url, title}: local ones are offered to phones
link.targets() -> Vec<(link_id, device_name)>          // phones a page can be sent to now
link.open_on_phone(&tab_url, Option<&link_id>) -> bool // http(s) only, 127.0.0.1/[::1] mapped to localhost
link.flush(); link.shutdown();
events: std::sync::mpsc::Receiver<LinkEvent>
  LinkEvent::Changed(Box<LinkState>)   // only when the state really differs
  LinkEvent::OpenRequested { url, link_id, device_name }  // open a tab (activate) and show the window
```

`LinkState`, `DeviceInfo` and `PairingInfo` derive `serde::Serialize` (camelCase, `pairing.qr` skipped) if the app wants JSON.

What the app must wire: call `start` once at launch; poll `events` (a thread forwarding into the UI loop is fine);
push `set_tabs` when tabs or their titles change; show Settings > Devices from `LinkState`; call `shutdown` on quit.
The computer name is read with `scutil --get ComputerName` on macOS, the hostname elsewhere.

### Threading

`LinkManager::start` spawns its own multi-thread tokio runtime (2 workers, thread name `signet-link`). Methods block the caller
briefly (they run a future on that runtime and wait) and must not be called from a thread of that runtime. Hooks inside the
service only send on channels. The dev server scan (`lsof`/`ss`/`netstat`, HTTP probes) runs every 5 s while a phone is
connected, as in Electron.

For tests or a custom host, `link::service::LinkService` is the same engine without the runtime and the events glue
(async API, `LinkHooks` trait, injectable clock and port).

### Persistence

`<data_dir>/link.json`, same format as Electron's (`version`, `desktopId`, `general`, `defaultCert`, `links[]` with
`linkId`, `peer`, `enabled`, `certPem`, `key`, `tokenHash`, `createdAt`, `lastSeen`, `peerPaused`). Private keys are sealed
by the `Vault` as `enc:...`, or stored `raw:...` without one. An unreadable file is never overwritten. Writes are atomic
(temp file then rename). Note: an Electron `enc:` key (safeStorage) cannot be read by another vault, so such a link is
ignored and the phone must pair again; use a separate `data_dir` (the Rust app has its own folder anyway).

## 2. Passkeys with a phone (hybrid, caBLE v2, CTAP2)

Flow: the page script asks, the browser side validates, a session shows a `FIDO:/` QR code, the phone scans it, advertises over
BLE (service data on 0xFFF9 / 0xFDE2, AES-ECB + HMAC with keys derived from the QR secret), the browser opens a WebSocket to the
tunnel server (`wss://<domain>/cable/connect/<routing id>/<tunnel id>`, subprotocol `fido.cable`), runs a Noise KNpsk0
(P-256, AES-GCM, SHA-256) handshake, reads the phone's getInfo, sends one CTAP2 command and gets the credential.

Differences with Electron: BLE scanning is `btleplug` on all systems (no Swift helper, no BlueZ D-Bus code), the WebSocket
client is `tokio-tungstenite` over `tokio-rustls`, the Noise and CBOR code is hand-written like the TS (Chromium's variant
is not standard Noise), and the scan starts as soon as the session starts (the app opens its window at the same time).

### What the CEF side exchanges (same JSON as `webauthn.ts`)

Page script to browser (`navigator.credentials.get/create` with `publicKey`), BufferSources as base64url strings, functions and
undefined dropped (`toPlain` in `app/desktop/src/preload/webauthn.ts`):

```json
{ "type": "get" | "create", "mediation": "conditional"?, "publicKey": { "challenge": "<b64url>", "rpId": "...", ... } }
```

The browser process adds what the page must not provide: the frame `origin`, the top `origin`, `is_main_frame`.

```rust
match webauthn_precheck(origin, top_origin, is_main_frame, &payload) {
    Precheck::Answer(a) => reply(a.to_json()),     // invalid request, insecure origin, conditional mediation...
    Precheck::Start(request) => { /* below */ }
}
let hybrid = HybridManager::new();                 // once; own runtime; real BLE + tunnel server
let handle = hybrid.start_hybrid(request);         // returns at once; ends the previous session (it gets NotAllowedError)
handle.qr            // "FIDO:/..." text of the QR code
handle.qr_matrix     // Option<QrMatrix> to draw (or qr_matrix.to_svg(2))
handle.request_type  // "get" | "create"; handle.rp_id: show "Sign in to <rp_id>"
handle.events        // std::sync::mpsc::Receiver<HybridEvent>
handle.cancel()      // Cancel button, Escape, window closed, page AbortSignal, tab navigated away or destroyed
```

`HybridEvent`:

- `Status(PasskeyStatus)`: `waiting | off | no-adapter | denied | error | unsupported | connecting | confirm | failed`
  (same strings as `PasskeyStatus` in the Electron app; serialize with serde, kebab-case). Deduplicated.
- `AdvertSeen { tunnel_domain }`: the phone's advert was found and decrypted.
- `Done(WebAuthnAnswer)`: always the last event, exactly once. `answer.to_json()` is the reply for the page script:
  - `{"error":{"name":"NotAllowedError|SecurityError|TypeError|InvalidStateError","message":"..."}}`: the page rejects
    with `new DOMException(message, name)` (`TypeError` is a real `TypeError`);
  - `{"credential":{...}}`: the `toJSON()` form (`id`, `rawId`, `type: "public-key"`,
    `authenticatorAttachment: "cross-platform"`, `response` with `clientDataJSON`, `authenticatorData` and either
    `attestationObject` + `transports` + `publicKey` + `publicKeyAlgorithm` or `signature` + `userHandle`,
    `clientExtensionResults`), all binary fields base64url. The existing `buildCredential` in the Electron preload turns
    it into a `PublicKeyCredential`-like object.

Timeouts: `publicKey.timeout` clamped to 10 s .. 10 min (5 min default). "failed" is shown for 3 s before the error is
delivered. A failed session never leaks details to the page (always `NotAllowedError`, except `InvalidStateError` for an
excluded credential).

`HybridManager::with_deps` / `with_handle` allow another scanner or tunnel connector (`BleScanner`, `Connector` traits),
used by the tests and by an app that wants a different Bluetooth backend. `HybridDeps::log` receives stage lines for a dev
console (never keys or data).

Not handled (same as Electron): conditional mediation (answered `NotAllowedError` at once by `webauthn_precheck`), linking
information, extensions other than `credProps`.

## 3. Platform notes

- **macOS**: Bluetooth needs `NSBluetoothAlwaysUsageDescription` in the app's `Info.plist` (without it macOS kills the
  process at the first scan). Put it in the bundle script of `signet-app`/`scripts/bundle-macos.sh`, for example "Signet looks
  for your phone to sign in with a passkey." When run from a terminal during development, the terminal app is the responsible
  process and must have the key itself. Hardened runtime / sandbox builds also need the entitlement
  `com.apple.security.device.bluetooth`. Local network access (mDNS and the LAN server) needs
  `NSLocalNetworkUsageDescription` and `NSBonjourServices` = `_signet._tcp` in the `Info.plist` on macOS 15+.
- **Linux**: BlueZ through D-Bus (`bluetoothd` running, user in the `bluetooth` group or a normal desktop session). Needs
  `libdbus` at build time for btleplug (`libdbus-1-dev`). The scan never powers the adapter on. mDNS is built in (no Avahi
  needed). No special capability for the link server.
- **Windows**: WinRT Bluetooth LE (Windows 10+). Unpackaged apps need nothing; a packaged (MSIX) app needs the `bluetooth`
  device capability. The first LAN listen triggers the Windows Firewall prompt; the user must allow private networks.
- Everywhere: the link server listens on all interfaces (TCP 47615 or a free port) and only after a phone is paired or a QR
  code is shown, and only while the general switch is ON.

## 4. Verification status

Unit tests cover everything that does not need a phone: wire formats, QR codes, store, TLS server on loopback (pairing, control,
tunnels, pause rules), dev server detection, CBOR, advert decryption, Noise and transport crypto (with the TS test vectors),
the tunnel against a fake phone and tunnel server, CTAP2 requests and responses, WebAuthn validation, and whole hybrid
sessions with a fake scanner and phone. Not verified without hardware or the internet: the real `btleplug` scan, the real
connection to `cable.ua5v.com`, mDNS discovery by a phone, and the TLS handshake with the mobile app's pinning code.

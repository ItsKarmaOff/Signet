# Link between devices (no account)

A phone pairs **once** with a computer by scanning a QR code. After that, when they are on the same network:

- `localhost:PORT` typed on the phone opens the computer's dev server. The origin stays `http://localhost:PORT`, so the page is a "secure context"; WebSockets (Vite/Next auto reload) and calls to other local ports work too;
- "Open on my phone" (computer → phone) and "Open on the computer" (phone → computer);
- the phone sees the list of the computer's dev servers (listening ports + open localhost tabs).

No account, no server, nothing goes through the internet. The shared code is here (`protocol.ts`, `status.ts`, `loopback.ts`); each app does its own transport.

## Switches (on both sides)

- A **general switch** ("Sync") and a **per-device switch**.
- A link only works if all **four** are ON. Turning one off does not delete the link (the keys stay).
- General switch OFF: the device switches are grayed out but keep their position.
- **Forget** on a device deletes the link (keys included); the other side is told (`forget`) if it is connected. Otherwise, it will find out
  at the next connection (`unknown-link`) and show "Link no longer valid" with a button to delete it on its side.
- Pairing again = new link = new keys (fresh certificate and token).
- Displayed state: `linkStatus()` in `status.ts` (Connected / Connecting… / Paused here / Paused on X / Not found / Link no longer valid).

## Security

- **One TLS certificate per link** (self-signed, ECDSA P-256, created by the computer at pairing). The phone pins it: SHA-256 of the
  DER certificate, received in the QR code. No other check (no authority, no host name): only the fingerprint counts.
- **SNI** = `serverNameForLink(linkId)`. The computer picks the link's certificate from this name. Unknown name → the installation's default
  certificate; the phone then sees a different fingerprint (`pin-mismatch`). If that same computer (same `desktopId` in
  mDNS) answers this way, the link is considered deleted on the other side ("Link no longer valid").
- **Link token**: 32 random bytes (base64url, 43 characters), created by the computer and given to the phone in `paired`.
  The phone keeps it in the Keychain / Keystore (expo-secure-store). The computer only keeps its SHA-256 and compares in constant time.
- **Pairing code**: 16 random bytes, single use, valid for 5 minutes (`PAIRING_TTL_MS`). Only one active QR code at a time.
- The computer only opens a tunnel to **127.0.0.1:port or [::1]:port**, never anywhere else, and never to its own link port.
- General switch OFF on the computer: it stops the mDNS announcement **and** closes its listening port. Open connections
  receive `{"type":"status","paused":true}` and are then closed.

## Discovery

- The computer announces itself over mDNS: type `_signet._tcp` (`LINK_SERVICE_TYPE`), instance name = computer name, TXT
  `v=1` and `id=<desktopId>`.
- It listens on all interfaces, port `LINK_DEFAULT_PORT` (47615) if it is free, otherwise a free port (announced everywhere).
- The phone tries in order: addresses seen over mDNS for this `desktopId`, then the last addresses that worked, then
  those from the QR code. Reconnects with a growing delay (1 s → 30 s), only while the app is in the foreground.

## Connections and messages

One TCP + TLS connection = one role. Each message is a JSON object on one line (`\n`), 64 KiB at most. Types and validation:
`protocol.ts` (`parseLinkMessage`, `encodeLinkMessage`). The phone speaks first.

### Pairing

1. The computer creates a pending link: `linkId` (8 random bytes in hexadecimal), certificate, code. It shows the QR code:
   `encodePairingUrl(offer)` → `signet://link?d=…` (the iOS camera also opens the app with this link).
2. The phone reads the QR code (`decodePairingUrl`), opens a pinned socket to `hosts[i]:port` and sends
   `{"type":"pair","v":1,"linkId","code","device"}`.
3. The computer checks the code (constant time, single use, not expired), creates the token, saves the link (ON by default) and answers
   `{"type":"paired","token","device"}` then closes. Errors: `bad-code`, `expired`, `unknown-link`, `version`.
4. The phone saves the link (ON by default) then opens the control connection.

### Control

- Phone: `{"type":"hello","v":1,"linkId","token","role":"control","device"}`.
- Computer: `{"type":"welcome","device","paused"}` (`paused` = this link is paused on the computer side), or `error`
  (`unknown-link`, `bad-token`, `version`) then close.
- Then, in both directions: `status`, `open`, `forget`, `ping`/`pong`. The computer sends `servers` right after `welcome` then on
  each change (at most once every 2 s), only if the link is active on both sides.
- `ping` every 20 s from the phone; with no answer within 10 s, it closes and reconnects.
- Pause on the computer side (device switch): it sends `status paused:true` and keeps the connection open (it only answers
  `ping` and no longer pushes anything). Resume: `status paused:false` then `servers`.
- Pause on the phone side (general or device): it sends `status paused:true` then closes, and no longer reconnects to that computer.
  Resume: it reconnects (the `hello` counts as a resume).
- `open` is only accepted if the link is active on both sides. http(s) URLs only.

### Tunnel (localhost)

- Phone: `{"type":"hello","v":1,"linkId","token","role":"tunnel","port":5173}`.
- Computer: checks the token and that the link is ON on its side, connects to `127.0.0.1:port` then `[::1]:port` (Vite often listens
  on `::1` only), answers `{"type":"ok"}` then relays the raw bytes in both directions until the connection closes.
  Errors: `paused`, `port-closed`, `port-refused`, `bad-token`, `unknown-link`.
- One TLS connection per tunnel (the WebView keeps its connections open, which is enough on a local network).

## How the phone sends `localhost` to the computer

`servers` also carries `ports`: all the ports that listen on the computer (pages and APIs, excluding system and ephemeral ports). The
phone itself listens on its localhost on these ports ("mirror ports") and relays each connection through a tunnel: a real iPhone
never sends `localhost` to a proxy, and Android does the same to keep things simple. Details: `app/mobile/modules/signet-link/README.md`.

## Choosing the computer for `localhost`

`pickLocalhostRoute(activeLinkIds, chosen)`: none → `localhost` stays the phone; just one → that one; several with no valid choice → the
phone shows a window to choose (when opening a `localhost` address), and keeps that choice as long as that computer is active.
On the phone, the addresses `127.0.0.1`, `[::1]` and `0.0.0.0` are mapped to `localhost` (`toLinkLocalhost`): only this name
goes through the computer.

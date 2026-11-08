# signet-scripts

Page scripts of the Rust port: the JavaScript injected in web tabs (port of `app/desktop/src/preload/*`, except
`index.ts`, the trusted interface preload replaced by GPUI, and `extension-apis*.ts`, ported elsewhere) and the Rust
side of their channels: message types, sender checks, validation.

Depends on `url`, `psl`, `serde`, `serde_json` only (no internal crate, no GPUI, no CEF).

```
js/                     the scripts, hand-written modern JS, no build step (include_str! in src/scripts.rs)
  bridge.js             shared runtime: request/response with ids and timeouts, events by channel
  identity.js swipe.js passwords.js autofill-core.js autofill.js notes.js gpc.js webauthn.js
src/
  bridge.rs             the two bridge function names (single place), JS to dispatch a message in a frame
  scripts.rs            PageScript, all_scripts(), Registry (which script for which frame), bundle()
  messages.rs           PageMessage (page to browser), Reply and PageEvent (browser to page), parse()
  validate.rs           validate(sender, message): the checks the browser side must apply
  webauthn_request.rs   validation of a passkey request (port of main/web/webauthn-request.ts)
  notes.rs              which document a notes page may touch, cleaning of what it saves
tests/js/               Node tests (node:test, small fake DOM), run by `cargo test` through tests/js_tests.rs
```

Run the tests: `CARGO_TARGET_DIR=target-scripts cargo test -p signet-scripts` (runs `node --test tests/js` too when
`node` is on the PATH). Without cargo: `node --test tests/js` from this folder (the bundle tests then skip).

## 1. The bridge contract

Two functions on `window`, named by two constants in `src/bridge.rs` (`POST_FN = "__signetPost"`,
`DISPATCH_FN = "__signetDispatch"`). The JS never hardcodes them: they are passed to `bridge.js` as arguments by
`scripts::bundle`, so changing the constants is enough.

| Direction | Function | Provided by | Notes |
| --- | --- | --- | --- |
| page to browser | `window.__signetPost(channel: string, json: string): void` | `signet-web`, in every frame, before the scripts run | The bridge captures it and deletes it from `window` (or redefines it as `undefined` if it is not configurable). |
| browser to page | `window.__signetDispatch(channel: string, json: string): void` | the bundle (`bridge.js`) | Non-enumerable, non-writable, non-configurable, displayed as a native function. `signet-web`/`signet-app` call it by executing the string from `bridge::dispatch_js`. |

### Message shapes

Page to browser: `json` is `{"rid"?: string, "data": any}`.

- `rid` is present when the page waits for an answer (a request), absent for fire and forget messages. It is 128 random
  bits in base 36 (from `crypto.getRandomValues`). `messages::parse` enforces "request if and only if the channel
  expects a reply".
- `data` is the channel's payload (see the table below), `null` when there is none.

Browser to page:

- Reply, channel `"reply"`: `{"rid": string, "ok": true, "data": any}` or `{"rid": string, "ok": false, "error": string}`.
  Build it with `Reply::ok` / `Reply::err`, then `Reply::to_js()` (or `to_dispatch()` for the pair `(channel, json)`).
- Event, any other channel: the JSON of the event's data (`PageEvent::Locale`, `PageEvent::Appearance`).
- A request that gets no answer in time completes with the error `"timeout"`. Timeouts: 30 s by default, swipe 2 s,
  passwords and autofill menus 5 s, fills 30 s, passkeys 10 min.

JS API of the bridge, given to each script as its first argument (frozen object):
`send(channel, data)`, `request(channel, data, cb(error, value), timeoutMs)` returning a cancel function,
`invoke(channel, data, timeoutMs)` returning a Promise (only for results that go to the page anyway: notes, passkeys),
`on(channel, cb)` returning an unsubscribe function, `available`. Credentials and card numbers use the callback flavour
so that a page that patches `Promise.prototype.then` cannot observe them.

### Injection

Every script is a function expression `(bridge, config, global) => void` (`autofill` gets its pure helpers inside its
composed source). `signet-web` does not inject them one by one: it asks `Registry::bundle_for(&FrameContext, RunAt)` and
injects the returned program (a single IIFE holding the bridge and all the scripts that apply, each in its own
try/catch) in the frame at document start, before any page script, in the page's main world. One program per frame is
required: the bridge captures and deletes `__signetPost` once, and all the scripts share its closure.

## 2. Scripts

| Script (`id`) | Electron source | Frames | When | Profiles | Role |
| --- | --- | --- | --- | --- | --- |
| `signet-identity` (`identity.js`) | `preload/identity.ts` | all | always (skips `chrome-extension:` itself) | all | Fills `window.chrome` (`loadTimes`, `csi`, `app`) like Chrome, otherwise Google refuses the sign-in. |
| `signet-webauthn` (`webauthn.js`) | `preload/webauthn.ts` | main | `Setting(WebAuthnIntercept)` | all | Replaces `navigator.credentials.get/create` for `publicKey` requests (forwarded as `webauthn:request`), and reports no platform authenticator. |
| `signet-swipe` (`swipe.js`) | `preload/swipe.ts` (SPEC v10) | main | always | all | Two-finger swipe back/forward (bubble in a closed shadow root); reports real form submissions (`page:submit`); hides the "switch browser" promo on the Chrome Web Store. |
| `signet-passwords` (`passwords.js`) | `preload/passwords.ts` (SPEC v17) | main | http(s) | all | Account list under sign-in fields, fills on click, reports submitted credentials. |
| `signet-autofill` (`autofill.js` + `autofill-core.js`) | `preload/autofill.ts` + `shared/autofill/{classify,card,values}.ts` | main | http(s) | all | Address and card dropdown, fill on click, reports submitted forms. The classifier and helpers are a pure module (tested). |
| `signet-notes` (`notes.js`) | `preload/notes.ts` | main | `AppPages` (plus the route in the hash, checked by the script) | not private | Defines `window.signet` (locale, appearance) and `window.signetNotes` (load, save, remove) on the Note and Easel pages. |
| `signet-gpc` (`gpc.js`) | `preload/gpc.ts` | all | `Setting(GlobalPrivacyControl)` | all | `navigator.globalPrivacyControl` is `true`. |

Registrations mirror `main/web/profiles.ts` (`configureSession`, `configurePrivateSession`) and `main/web/gpc.ts`.
In Electron swipe, passwords and autofill were registered for all frames and returned early outside the main frame;
here they are registered for the main frame only (same effect). `RunAt` is `DocumentStart` for all of them.

`FrameContext { url, is_main_frame, is_private, settings }` is what `signet-web` must know about the frame.
`ScriptSettings { global_privacy_control, webauthn_intercept }` follows the `globalPrivacyControl` setting and
`webauthn_intercept_enabled()` (Linux, or a non-packaged build with `SIGNET_WEBAUTHN_INTERCEPT=1`; `cfg!(debug_assertions)`
stands for "not packaged"). When a setting changes, new frames get the new bundle (existing pages keep what they have,
as with Electron's preload scripts).

`ScriptOptions` holds what may move: `notes_pages` (extra app pages that host Notes and Easels on a hash route
`#note/<uuid>` or `#easel/<uuid>`; default: none) and `swipe_diag`.
**Notes address decision**: the notes script (`signet-notes`) and `validate` accept the document address itself,
`signet://note/<uuid>` and `signet://easel/<uuid>`, which is what `signet-pages` serves and what Electron tabs store in
`state.json` (the Electron tab showed `signet://note/<id>` and loaded the UI on `#note/<id>`; see SPEC v22). Existing
note tabs thus keep working after the first-run import with no rewrite (`signet_core::urls::electron_note_url` exists
for stray real addresses). `notes_pages` is kept for a hash-routed layout and is empty by default.

## 3. Channels

All channels are untrusted. "Sender" is what the browser process knows (see `validate::Sender`), never the payload.
Every channel requires a tab (not an interface view, not an extension page) and its main frame.

| Channel | Dir | Type | Page payload (`data`) | Reply (`data`) | Validation (`validate::validate`) |
| --- | --- | --- | --- | --- | --- |
| `swipe:can` | page to browser | request | `null` | `{enabled, back, forward}` | tab, main frame. Browser: setting `swipeNavigation`, cooldown 120 ms, history state. Refused: all false. |
| `swipe:navigate` | page to browser | send | `"back"` or `"forward"` | | tab, main frame; the browser re-evaluates `can` before navigating. |
| `page:submit` | page to browser | send | `null` | | tab, main frame (`noteFormSubmit`). |
| `pw:accounts` | page to browser | request | `null` | `[{id, username}]` | tab, main frame, sender origin matches `^https?://[^/]+$`. Refused: `[]`. |
| `pw:fill` | page to browser | request | `{id}` | `{username, password}` or `null` | same, `id` non-empty and at most 200. Browser: the entry's origin must equal the sender origin. |
| `pw:submit` | page to browser | send | `{username, password}` | | same, password at most 1000 (UTF-16 units), username trimmed and cut to 1000. Empty password means the username step. |
| `af:menu` | page to browser | request | `{kind: "address" or "card"}` | `{kind, title, manage, items:[{id,label,detail,match}]}` or `null` | tab, main frame, http(s) origin, `can_fill` (cards: https or localhost only). At most 20 items (the page drops more). |
| `af:fill` | page to browser | request | `{kind, id}` | address: `{address:{...}}`; card: `{network, holder, number, expMonth, expYear}`; or `null` | same as `af:menu`, id at most 200. Browser: the id must belong to the menu this frame opened (10 min), a card needs the user's authentication. The security code is never sent. |
| `af:manage` | page to browser | send | `{kind}` | | tab, main frame, http(s) origin. Opens Settings. |
| `af:submit` | page to browser | send | `{address?, card?}` | | tab, main frame, http(s) origin, payload is a plain object; the feature then cleans it (`cleanAddress`, `parseCardSubmission`), applies `mayOffer` and the 3 s rate limit. |
| `webauthn:request` | page to browser | request | `{type: "get" or "create", mediation?, publicKey}` (BufferSource as base64url) | `{error:{name,message}}` or `{credential}` | tab; `webauthn_request::validate_webauthn_request` (secure origin, rpId against the public suffix list, frame rules, payload shape). Anything else gets `NotAllowedError`. |
| `webauthn:abort` | page to browser | send | `null` | | tab; only the frame that owns the session can end it. |
| `notes:locale` | page to browser | request | `null` | locale code | tab, main frame, not private, frame address is an app page on a document route. |
| `notes:appearance` | page to browser | request | `null` | appearance object | same. |
| `notes:load` | page to browser | request | `null` | `{kind, id, doc, hue}` | same; the document id comes from the frame address, never from the payload. |
| `notes:save` | page to browser | request | `{title, html}` or `{title, items, view}` | `true` or `false` | same, plus `notes::clean_patch`: title cut to 200, note HTML at most 2 MiB (UTF-16 units), Easel items bounded and filtered (types, numbers, colors, image data URLs), Easel JSON at most 30 MiB. Refused: `false`. |
| `notes:remove` | page to browser | request | `null` | `null` | same. |
| `locale`, `appearance` | browser to page | event | locale string, appearance object | | Pushed by the app to notes pages when they change. |
| `reply` | browser to page | | `{rid, ok, data}` or `{rid, ok:false, error}` | | |

`messages::parse(channel, json)` rejects unknown channels, oversized JSON (16 KiB by default, 64 KiB for `af:submit`,
1 MiB for `webauthn:request`, 40 MiB for `notes:save`), malformed envelopes, wrong payload types, and a request without
`rid` or a send with one. `validate::refusal_reply` gives the answer of a refused request, the same as the Electron
handlers returned.

Not covered here (it belongs to the feature that owns the data): password vault lookups and anti-spam
(`signet-features` passwords), autofill menu bookkeeping, `cleanAddress`, `parseCardSubmission`, `mayOffer`, Luhn
(`signet-features` autofill), the hybrid session behind `webauthn:request` (`signet-link`), swipe history state and
cooldown (`signet-app`).

## 4. What changes without isolated worlds

Electron ran the preload in an isolated world with `contextBridge`; CEF has none, so every script now runs in the page's
main world, next to untrusted code.

- **No private `ipcRenderer`**: the post function is captured at start and removed from `window`. `__signetDispatch`
  cannot be removed (the browser must reach it), so it is locked (non-writable, non-configurable, non-enumerable). A page
  can call it, but replies need the 128-bit `rid` of a pending request, and events only matter to notes pages.
- **The browser side stays the authority**: sender from the frame, never from the payload, sizes, rate limits, and the
  origin rule on every credential or card request.
- **No isolated copy of the DOM prototypes**: a hostile page could patch `Element.prototype`, `Event`,
  `HTMLInputElement.prototype.value` before our listeners run. The bridge captures `JSON`, `Map`, `Promise`, timers and
  `crypto.getRandomValues` once; `passwords.js` and `autofill.js` capture the native `value` setters at start. Credentials
  and card data come back through callbacks, not Promises. Filling still ends up in page-visible fields, as before.
- **Trusted Types**: pages enforcing Trusted Types would block `innerHTML`; the icons are now built with DOM calls.
- **Listeners are visible to the page** (focus, input, submit, wheel on `document` and `window`), and the page can call
  `stopPropagation` in the capture phase before ours, as it could in Electron. Only `isTrusted` events trigger fills.
- **`notes.js` defines `window.signet` and `window.signetNotes` directly** (frozen, non-configurable) instead of
  `contextBridge.exposeInMainWorld`, and only for app pages on a document route in the main frame.
- **`identity.js`, `gpc.js`, `webauthn.js`** already ran in the main world (`executeInMainWorld`); their code is unchanged
  except that `webauthn.js` talks to the bridge instead of receiving proxied functions.
- **`swipe.js`**: the Chrome Web Store promo hiding runs directly (it used `webFrame.executeJavaScript`).
- A bridge timeout replaces "the main process never answers": swipe treats a timeout as "this gesture only".
- Fixed on the way: `parseAutocomplete` used `token in AUTOCOMPLETE` (so `constructor` counted as a token); it now checks
  own properties.

## 5. What signet-web and signet-app must implement

`signet-web`:

1. For every frame (main and sub, normal and private profiles), before page scripts: define `window.__signetPost` (a native
   function bound to the frame, not enumerable), then evaluate `Registry::bundle_for(&ctx, RunAt::DocumentStart)` if it
   returns `Some`. A `DocumentEnd` bundle uses the same call with `RunAt::DocumentEnd` (nothing needs it today).
   The bundle must run exactly once per document, after `__signetPost` exists and before any page script.
2. On every `__signetPost(channel, json)`, forward to the app: view id, frame URL, `is_main_frame`, the top frame URL,
   `channel`, `json`. Do not interpret them. The origin must come from the browser process, not from the page.
3. Offer `execute_js(view, frame, code)` so that the app can run `Reply::to_js()` and `PageEvent::to_js()` in the right
   frame (the frame that posted a request, or the main frame of a notes tab for events).
4. Do not inject into extension pages, DevTools and interface views that are not tabs (the `Registry` already skips the
   password and autofill scripts on non-http(s) URLs, but identity and swipe apply to any URL).
5. Keep the list of tab views (to build `Sender::is_tab`) and which tabs are private.

`signet-app`:

1. Build a `Sender` (`is_tab`, `is_private`, `is_main_frame`, `frame_url`, `origin`, `top_origin`) from what `signet-web`
   reports (`validate::origin_of_url` for http(s)), then `messages::parse` and `validate::validate`. On `Err(refusal)`
   for a request, answer `validate::refusal_reply`; for a send, drop it.
2. Handle each `Accepted` with the owner of the data: swipe (history, setting, 120 ms cooldown), form submit flag for Peek,
   passwords (`pw:*`), autofill (`af:*`), passkeys (`webauthn:request` goes to `signet-link`, a conditional request is
   answered `NotAllowedError` without a session; `webauthn:abort` ends the session of that frame), notes (`NotesOp`).
3. Answer every request exactly once with `Reply::ok` or `Reply::err` (a missing answer is a timeout in the page).
4. Push `PageEvent::Locale` and `PageEvent::Appearance` to open notes tabs when settings change.
5. Pass the `ScriptSettings` (`globalPrivacyControl` setting, `webauthn_intercept_enabled()`) and the notes page address
   to the registry, and keep both in sync with `signet-pages`.

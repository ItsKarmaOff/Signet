# signet-extensions

Chrome extensions for Signet GPUI: manifest, install (Web Store CRX, unpacked), the `chrome.*` APIs Signet implements itself
(port of `app/desktop/src/main/features/extensions/**`), and the page-side JavaScript that builds the `chrome` object in
extension contexts. **No CEF call inside this crate**: the app drives an `ExtensionHost` (events in, decisions out).

```sh
cd app/desktop-rs
CARGO_TARGET_DIR=target-extensions CARGO_BUILD_JOBS=4 cargo test -p signet-extensions
```

(325 unit tests, one Node based test of the page scripts that is skipped when `node` is missing. No network: CRX fixtures
are checked in, `tests/fixtures/make_fixtures.py` rebuilds them with openssl.)

## 1. Research: Chrome extensions in CEF 156

Sources: the CEF 156.0.15 headers (`.cache/cef/156.0.15/.../include`), the `cef` crate 156.3 sources, CEF's architecture
page and issue #3048 (extensions in the Alloy runtime).

What the headers and the crate say:

- **There is no extension API in CEF 156.** The headers have no `CefExtension`, `CefExtensionHandler` or
  `CefRequestContext::LoadExtension` (removed with the Alloy bootstrap around M127/M128), and `cef` 156.3 has no binding
  for them. Only `IDC_MANAGE_EXTENSIONS`-style command ids (Chrome UI) and `chrome://` resources remain.
- **Two runtime styles per browser** (`internal/cef_types_runtime.h`), both running on the Chrome bootstrap since M125:
  - *Chrome style* is the Chrome UI layer (`Browser`, `TabStripModel`, toolbar, `chrome://extensions`, Web Store install
    bubble, real extension system: service workers, content scripts, `chrome.*`). cefclient lists extension loading as a
    Chrome style feature.
  - *Alloy style* is the content layer: no Chrome UI, no extension system. Issue #3048 and the CEF maintainers: Alloy only
    keeps the few extension pieces the PDF viewer needs and will get no more extension APIs.
- **Embedding rules** (same header, `internal/cef_types_mac.h`): "Alloy style will always be used if windowless rendering is
  enabled or if `parent_view` is provided". A Views window of Alloy style hosts only Alloy browser views; a Chrome style
  window hosts at most **one** Chrome style browser view (plus any number of Alloy ones). A Chrome style browser cannot be a
  child view inside a foreign native window (GPUI).

What this means for Signet (one GPUI window, many tabs as native child views):

| Feature | Chrome style | Alloy style child view (what Signet uses) |
| --- | --- | --- |
| `--load-extension`, `--disable-extensions-except` | Chromium's own switches: expected to work (not verified here, see below) | no extension system, ignored |
| Web Store "Add to Chrome", `chrome://extensions` | yes, with Chrome UI | no |
| Toolbar action popups, shortcuts, context menu items | Chrome toolbar only; not our GPUI toolbar | we draw them |
| Content scripts, `chrome.*`, service workers | yes, for tabs of that Chrome `Browser` | **none** |
| Many tabs inside one GPUI window | no (one Chrome browser view per window, no foreign parent) | yes |

To verify before relying on the "expected" cell: run cefclient/cefsimple with `--use-chrome-runtime --load-extension=<dir>`
and open `chrome://extensions`. It cannot help Signet's layout anyway (see the decision).

### Decision

Chromium's real extension system cannot serve Signet tabs. The Electron app had the same shape: its extension APIs came from
`electron-chrome-extensions` and Signet's own APIs (`apis/*`) covered the gaps. With CEF the library layer disappears, so
**Signet owns the whole extension runtime** and Chromium only provides what it provides to any embedder:

| Delegated to Chromium (CEF Alloy) | Reimplemented by Signet (this crate + signet-web) |
| --- | --- |
| Rendering, V8, network stack, cookies, storage per origin | Manifest parsing and validation (`manifest`) |
| Custom scheme plumbing (`CefRegisterSchemeHandlerFactory`) | `chrome-extension://<id>/` served from the extension folder, `web_accessible_resources` checks (web layer, with `manifest`, `urlmatch`) |
| `ExecuteJavaScript` in any frame, `CefRenderProcessHandler::OnContextCreated` | Content script matching and wrapping (`contentscripts`), injection by the web layer |
| Request hooks (`CefResourceRequestHandler`) | declarativeNetRequest engine (`dnr*`), webRequest auth (`auth`) |
| `SetPreference("proxy" / "webrtc.ip_handling_policy")` | `chrome.proxy`, `chrome.privacy` models (`settings`, `proxyconfig`) |
| Hidden Alloy browser for the background | Background as a generated page that loads the service worker script (web layer), lifecycle and event wake-up (`router`) |
| (nothing) | CRX download/verify/unzip, install layout, updates, id computation (`crx`, `id`, `registry`, `unpacked`) |
| (nothing) | `chrome.storage`, messaging and ports, alarms, permissions, commands, context menus, notifications, action state, tabs mapping, offscreen, contexts (all modules below) |
| (nothing) | The page side: `chrome` object, safety net for missing APIs, Web Store install bridge (`assets/*.js`) |

How Signet's APIs reach extension contexts: every extension page, background page and content script gets
`ExtensionHost::page_script(...)` evaluated first (before any page script). It sets the config, loads `bridge.js` (a promise
based request/response over `window.__signetPost` / `window.__signetDispatch`) and builds `chrome`/`browser`. Calls go to
`ExtensionHost::call`, events come back through `Effect`s.

Known limitations of this strategy (Electron had them partly, others are new):

- The background is a **page, not a service worker**: no `install`/`fetch` events, no `self.registration`, no 30 s idle stop
  (it stays alive; the wake logic only matters if the web layer stops it). `importScripts` and module workers must be
  emulated by the generated background page (`<script>` tags, `type=module` honoured).
- **No isolated world** for content scripts (CEF has none): each script group runs in its own function scope with a private
  `chrome`, but the page can read its globals and tamper with prototypes. `world: "MAIN"` runs without `chrome`.
- Strict page CSP does not block injection (the browser process executes the script), but extensions that inject `<script>`
  elements see the CSP.
- Not provided (placeholders that report "not supported in Signet", from `assets/api-catalog.json`): `sidePanel`, `debugger`,
  `tabCapture`, `userScripts`, `identity` flows, `nativeMessaging`, `declarativeContent`, `downloads`, `cookies`, `history`,
  `bookmarks`, `webRequest` listeners other than `onAuthRequired`, `scripting`, `webNavigation` events other than errors and
  fragment changes. `tabs` and `windows` cover query/get and delegate `create/update/remove/reload` to the browser (`TabAction`).
  The next batches in order of value: `scripting.executeScript`, `cookies`, `webNavigation` main events, `webRequest`
  (observation), `nativeMessaging`, `downloads`.
- Web Store: the `chrome.webstorePrivate` method names and result strings were written from memory of the Electron library;
  verify them against the live store (`assets/webstore.js`).

## 2. Module map

| Rust module | Port of (`app/desktop/src`) | Tests |
| --- | --- | --- |
| `common` | `main/features/extensions/apis/types.ts`, `core/atomic.ts` | via users |
| `urlmatch` | `apis/urlmatch.ts`, `patternCovers` of `apis/permissions.ts`, `compilePattern` of `main/web/network.ts` | 5 |
| `permissions` | `apis/permissions.ts` and the install dialog lines of `extensions.ts` | 15 |
| `commands` | `shared/extension-commands.ts`, `apis/commands.ts` | 21 |
| `catalog` + `assets/api-catalog.json` | `shared/extension-api-catalog.ts` (JSON generated from the TS) | 8 |
| `shared` | `shared/extension-apis.ts` | 5 |
| `reload` | `unpacked-reload.ts`, `action-relay.ts` | 14 |
| `unpacked` | `unpacked.ts` (folders, `notify` watcher, state for Settings) | 7 |
| `dnr_match` | `apis/dnr-match.ts` | 33 |
| `dnr` | `apis/dnr.ts`, `apis/lifecycle.ts` | 9 |
| `dnr_engine` | `apis/dnr-engine.ts` | 23 |
| `schedule` | `apis/schedule.ts`, `apis/alarms.ts` | 12 |
| `proxyconfig`, `settings` | `apis/proxyconfig.ts`, `proxy.ts`, `privacy.ts`, `settings.ts` | 8 + 9 |
| `contextmenus` | `apis/contextmenus.ts` | 15 |
| `contexts` | `apis/contexts.ts`, `stubs.ts`, `offscreen.ts` | 6 |
| `auth` | `apis/auth.ts` | 9 |
| `action` | `apis/action.ts` (+ badge text/title/popup model, new) | 9 |
| `notifications` | `apis/notifications.ts` | 9 |
| `navigation` | `apis/navigation.ts` | 7 |
| `tabs` | `apis/tabs.ts`, `TabInfo::to_chrome_tab` (new) | 12 |
| `router` | `apis/router.ts` (permission check, event wake/queue/backoff) | 12 |
| `assets` + `assets/*.js` | `preload/extension-apis.ts`, `extension-apis-install.ts` (rewritten, see below), `webstore.js` (new) | 1 + `tests/js_smoke.rs` |
| `manifest` | new (MV2/MV3 parsing and validation) | 9 |
| `id` | new (extension ids: public key, path) | 4 |
| `crx` | new (Web Store URLs, CRX2/CRX3 verify, unzip, install, updates) | 14 |
| `registry` | new (installed list, enabled, pinned/toolbar helpers, Settings list) | 7 |
| `storage` | new (`chrome.storage` areas, quotas, events) | 7 |
| `contentscripts` | new (matching, globs, JS/CSS to inject) | 6 |
| `messaging` | new (`sendMessage`, `tabs.sendMessage`, ports) | 10 |
| `host` | `extensions.ts` (+ `apis/index.ts`): the facade | 19 |

Not ported one to one: `extension-apis-install.test.ts` (262 lines) tested the Electron specific "frozen chrome" dance; the
new installer is covered by `tests/js_smoke.rs` (Node, real Rust generated config). `electron-chrome-extensions` behaviour
(tabs/windows/action buttons/context menu UI) is replaced by the new modules above.

### Page side (`assets/`)

| File | Role |
| --- | --- |
| `bridge.js` | Promise request/response helper over `window.__signetPost` / `window.__signetDispatch`; subscription for pushed events. |
| `extension-apis-install.js` | `installApis(config, bridge, target)`: builds `chrome`/`browser` from `config.implemented`, hand written members (runtime messaging, ports, i18n, contextMenus `onclick`, `action.setIcon` imageData, `onAuthRequired`, settings, DNR enums) and the catalog safety net. Testable in Node. |
| `extension-apis.js` | Entry point: context check (`chrome-extension://`, not sandboxed), install, report `__installed`. Content scripts get a private object in `window.__signetContent[id]`. |
| `webstore.js` | `chrome.webstorePrivate` for Web Store pages. |
| `api-catalog.json` | The API catalog (also read by Rust). |

Each file starts with a comment stating the bridge assumption.

## 3. The API (`host::ExtensionHost`)

```rust
let mut host = ExtensionHost::new(HostConfig::new(data_dir)); // data_dir = <config>/Signet GPUI
let effects = host.startup(unpacked_paths, pinned, order);    // settings.unpackedExtensions / pinnedExtensions / extensionOrder
```

Everything returns `Vec<Effect>`; the app carries them out. Main groups:

- **Calls**: `call(Call { id, extension_id, context, method, args }, &dyn TabHost)` -> `Effect::Reply`. Never on the UI thread
  (`TabHost` may block). Web pages of the Web Store use `webstore_call(call_id, page_url, method, args)` instead.
- **Install**: `install_from_store(&dyn Fetcher, id)` (blocking), `install_crx_bytes`, `update_all`, `remove_extension`,
  `set_enabled`, `describe_install(&Manifest)` (lines for the dialog), `install_decision`, `store_install_done`.
- **Unpacked**: `add_unpacked`, `reload_unpacked`, `remove_unpacked`, watcher through `tick()` (`Effect::ReloadExtension`).
- **Toolbar / Settings**: `infos()` (`ExtensionInfo`, same JSON as Electron), `toolbar()`, `with_button()`, `set_pinned`,
  `reorder`, `popup_url`, `action_clicked`, `button_state`, `action_counts`, `menu_items`, `button_menu_items`, `menu_clicked`.
- **Shortcuts**: `command_bindings()` (rebuild the menu on `Effect::CommandsChanged`), `run_command`.
- **Engine events in**: `worker_running/stopped`, `wake_result`, `context_opened/closed`, `tab_created/tab_gone`,
  `frame_navigated`, `navigated_in_page`, `load_failed`, `permission_answer`, `offscreen_loaded/failed/page_closed`,
  `tab_action_done`, `notification_clicked/action/closed`, `proxy_auth_challenge`.
- **Request hooks**: `before_request(&RequestDetails)` -> `Pass | Cancel | Redirect(url)`; `host.dnr.send_headers` /
  `received_headers` for header changes; `Effect::NetworkHooks(events)` says which hooks to wire.
- **Content scripts**: `injections(&Frame, Option<RunAt>)` -> CSS and JS strings per extension.
- **Page scripts**: `page_script(ext_id, kind, context_id, tab_id, frame_id)`; `assets::webstore_script()`.
- **Time**: `tick()` about every 250 ms (alarms, retries, DNR rebuild/counts, watchers, persistence), `next_wakeup()`, `flush()` at quit.
- **Effects**: `Reply`, `Router(DeliverRunning | StartWorker | DeliverToWorker)`, `ToContext`, `LoadExtension`,
  `UnloadExtension`, `ReloadExtension`, `PermissionPrompt`, `InstallPrompt`, `InstallFromStore`, `Notification`,
  `ApplySetting`, `SetActionIcon`, `ActionChanged`, `ActionStates`, `ActionCounts`, `NetworkHooks`, `CommandsChanged`, `OffscreenOpen/Close`,
  `TabAction`, `OpenOptions`, `RunAction`, `ExtensionsChanged`, `Unpacked/Pinned/OrderChanged`, `ProxyAuth`, `OpenUrl`, `Log`.

Persistence (same names as Electron, in the data folder): `extension-permissions.json`, `extension-alarms.json`,
`extension-settings.json`, `extension-dnr.json`, `extension-context-menus.json`, plus new `Extensions/<id>/<version>_0/`,
`extensions-state.json` and `Extension Storage/<id>/{local,sync}.json`. Pinned extensions, their order and the unpacked
folders stay in `state.json` settings (signet-core); the host returns the new lists as effects.

## 4. What signet-web must do

1. **Bridge**: expose `window.__signetPost(channel, json)` (page to app) and call `window.__signetDispatch(channel, json)` in
   the page (app to page) for extension contexts and Web Store pages. Channels: `signet-crx` (call), `signet-crx-reply`,
   `signet-crx-event`, `signet-crx-ctx`. The app maps `signet-crx` messages to `Call`s, **deriving `extension_id` and `context`
   from the sender** (the scheme/host of the frame), never from the message. Reject frames that are not
   `chrome-extension://` (sandboxed pages included) and send Web Store frames to `webstore_call`.
2. **`chrome-extension://` scheme**: register it standard + secure + CORS + fetch enabled, one origin per host (the id). Serve
   files from `Effect::LoadExtension.dir` (path traversal checks with `urlmatch::extension_file_path`). Requests from web pages
   only to `Manifest::web_accessible_resources`. Serve `_generated_background_page.html` for MV3 workers
   (`BackgroundEntry::ServiceWorker { path, module }` -> `<script src=path [type=module]>`), MV2 `Page`/`Scripts` likewise.
3. **Background**: for each `LoadExtension` with a background, open a hidden Alloy browser (windowless, 1x1, no throttling)
   on that page, run `page_script(.., "worker", ..)` first, report `worker_running/stopped`. Honour `StartWorker`
   (`wake_result`). Same for `OffscreenOpen` (a second hidden browser, `offscreen_loaded`).
4. **Extension pages** (popup, options, tabs on `chrome-extension://`): run `page_script(.., "page", ..)` before page scripts
   (render process `OnContextCreated` or `LoadStart`). Popups open under the button using `popup_url`.
5. **Content scripts**: on each frame at `document_start` / `end` / `idle` call `injections`, execute `css` then `js` with
   `ExecuteJavaScript`, preceded by `page_script(.., "content", ..)` once per frame. Run it for subframes (`all_frames`).
6. **Network**: wire `before_request` (and header hooks when `NetworkHooks` asks) in `CefResourceRequestHandler`; map CEF
   resource types to the names of `dnr_match::dnr_type`; call `tab_gone` on browser close. Provide the proxy 407 challenge to
   `proxy_auth_challenge`.
7. **Settings**: `ApplySetting` `proxy.settings` -> `CefRequestContext::SetPreference("proxy", EngineProxy::to_cef_preference())`;
   `privacy.network.webRTCIPHandlingPolicy` -> `webrtc.ip_handling_policy`.
8. **Web Store**: inject `assets::webstore_script()` into the main frame of `crx::is_store_url` pages; hide the "switch browser"
   banners with CSS. Implement `crx::Fetcher` (HTTP GET following redirects) and run `install_from_store` on a worker thread.
9. **Navigation events**: report `frame_navigated`, `navigated_in_page`, `load_failed` with frame ids as `webNavigation` names them.

## 5. What the app must do

- Create the host after `signet-core` has loaded settings; `startup(settings.unpacked, pinned, order)`; keep calling `tick()`.
- Implement `tabs::TabHost` over the browser model: tab ids are CEF browser ids (as `cef_browser.h` says, usable as `tabId`).
- Carry out effects: dialogs (`PermissionPrompt`, `InstallPrompt`, built with `permissions::PermissionLine` keys and the
  existing `dialog.extension.*` translations), notifications (`Notification`; macOS actions only), the toolbar (buttons from
  `toolbar()`, badge from `button_state`, counts, Extensions menu, right click menu: Options, Unpin, Manage, Remove), menus
  (`command_bindings`, context menu entries from `menu_items`), `TabAction`s (`tabs.create/update/remove/reload/...`),
  `OpenOptions`, storing `Unpacked/Pinned/OrderChanged` into settings.
- `reload::ActionRelay` is available for the "shortcut opens the popup under the toolbar button" handshake (`RunAction`).
- Persist: call `flush()` on quit.

## 6. Risks

- Biggest: no real service worker and no isolated world (above); some extensions (Bitwarden, uBlock Origin) lean on both.
  uBO Lite style DNR-only extensions are the best fit; password managers need `nativeMessaging`/`scripting` first.
- `chrome.tabs`/`scripting`/`cookies`/`webRequest` coverage is the main gap to close for popular extensions.
- CEF hooks for header rewriting are weaker than Electron's `session.webRequest`: request headers can be set, response header
  rewriting needs a resource filter; the DNR engine already produces the decisions.
- Extension id of unpacked folders hashes the canonical path (UTF-16 on Windows) like Chromium: unverified on Windows/Linux.
- `crx::unzip` caps the extracted size at 512 MB; the CRX3 verification checks every proof and the `crx_id` binding but not a
  publisher key allow list.
- The `notify` based watcher test is timing dependent on Linux inotify and Windows.

# signet-web

Generic CEF layer of the Rust port: startup, native child browser views, profiles, typed events,
page scripts with a page-to-app bridge, custom schemes. It depends on no other signet crate.
The optional `gpui` feature adds `gpui_pump`, which runs the CEF message loop on the GPUI main thread.

Crates: `signet-web` (this), `signet-helper` (subprocess binary, one line), `signet-app` (demo `main` for now).

## Build, bundle, run (macOS)

```sh
cd app/desktop-rs
./scripts/bundle-macos.sh [--release]   # builds, assembles dist/Signet.app, ad hoc signs it
./scripts/run-macos.sh [--release] [-- app args]   # bundle then run (stdout stays on the terminal)
```

- `cargo build -p signet-app -p signet-helper` also works, but the app only runs from the bundle
  (the CEF framework and the helper apps must sit in `Contents/Frameworks`).
- Requirements: Xcode, cmake, ninja, and the Metal toolchain (`xcodebuild -downloadComponent MetalToolchain`),
  which `gpui-pre-apple` needs to compile its shaders.
- The CEF distribution (about 500 MB) is downloaded once by `cef-dll-sys` into `$CEF_PATH`
  (default `app/desktop-rs/.cache/cef`, git-ignored through `.cache/.gitignore`). The scripts export `CEF_PATH`;
  when you call `cargo` yourself, export it too (`export CEF_PATH=$PWD/.cache/cef`) or a fresh download
  lands in each `target/` folder.
- Bundle layout: `Signet.app/Contents/MacOS/signet`, `Contents/Frameworks/Chromium Embedded Framework.framework`,
  and `Signet Helper.app`, `Signet Helper (GPU|Renderer|Plugin|Alerts).app` (ids `dev.signet.app.gpui.helper...`).
  The app id is `dev.signet.app.gpui`. Icon: `app/desktop/build/icon.icns`.

## Startup and shutdown

```rust
signet_web::set_scheme_handler(|url| ...);              // optional, before or after init
signet_web::init(Config::new(data_dir))?;               // after the GPUI App exists (NSApp must exist)
signet_web::set_page_scripts(&profile, scripts);        // per profile, applies to views created later
signet_web::set_key_filter(|e| ...);                    // app shortcuts before the page
signet_web::gpui_pump::install(cx, |event, async_cx| ...); // message loop + events + quit hook
```

`init` loads the framework (macOS), runs `execute_process`, calls `initialize` with: `root_cache_path = <data>/cef`,
`cache_path = <data>/cef/default`, `external_message_pump = true`, `browser_subprocess_path` = the bundle helper,
locale, `Accept-Language`, a Chrome-like user agent (`default_user_agent()`), `no_sandbox = true`, and then
pumps until `OnContextInitialized` (browsers cannot be created earlier).
`shutdown()` (called by the `gpui_pump` quit hook) closes every browser, pumps until they are gone (3 s max),
then calls `cef::shutdown`. Verified: quitting returns exit code 0 and leaves no helper process.

Sandbox: disabled (`no_sandbox`). The macOS sandbox needs `libcef_sandbox` linked into every helper
executable, which cef-rs does not offer for Rust helpers yet. Revisit before shipping.

## Threading model

Everything runs on the main thread. CEF runs with `multi_threaded_message_loop = false` and
`external_message_pump = true`, so all CEF UI-thread callbacks happen inside `do_message_loop_work`
on the GPUI main thread. Callbacks never call app code: they push `WebEvent`s into a queue.
`pump()` runs one loop iteration, `drain_events()` returns the queue.

`gpui_pump::install` spawns one foreground task. CEF calls `OnScheduleMessagePumpWork(delay)` from any thread;
the wake handler forwards it to an unbounded channel. The task sleeps until the requested deadline or
`FALLBACK` (100 ms), pumps, drains events and calls your handler with `&mut AsyncApp`. Idle CPU measured at 0.3 to 0.5 percent.
Only the key filter and the scheme handler run synchronously (they must answer inside the callback);
the scheme handler runs on a CEF IO thread, so it is `Send + Sync`.

## The NSApp trick (macOS)

CEF requires `NSApp` to conform to `CefAppProtocol` (`isHandlingSendEvent`, `setHandlingSendEvent:`) and
`-sendEvent:` to maintain that flag. GPUI creates its own `GPUIApplication` (a `ctor` registers the class,
then `sharedApplication` instantiates it), so we cannot subclass. `mac::install_cef_app_protocols()`
runs inside `init`, before `initialize`, and patches the class of the live `NSApp` with the objc runtime:
it registers the three protocols dynamically (`objc_allocateProtocol`), adds the two methods backed by an
atomic flag, adds a `sendEvent:` override that sets the flag around `NSApplication`'s original implementation,
and calls `class_addProtocol`. GPUI does not override `sendEvent:` today; if it does, the existing IMP is wrapped.

## WebView API

`WebView` is a `Copy` handle (`id() -> ViewId`). Create with `WebView::create(raw_window_handle, ViewOptions)`
(`window.window_handle()?.as_raw()` from GPUI). Bounds are window points with a top-left origin
(`Bounds { x, y, width, height }`); the macOS code reads `isFlipped` of the parent and converts, Windows scales
by `GetDpiForWindow`, X11 uses `GDK_SCALE`.

Navigation: `load_url, back, forward, reload, reload_ignore_cache, stop, url, can_go_back, can_go_forward, is_loading`.
Layout: `set_bounds, bounds, set_visible, is_visible, focus`. Page: `execute_js, set_zoom_level, zoom_level,
find, stop_find, print, print_to_pdf, view_source, set_audio_muted, is_audio_muted, edit(EditCommand),
start_download, replace_misspelling`. DevTools: see "Developer tools" below. `close()`.
Downloads: `cancel_download(id)`, `pause_download(id)`, `resume_download(id)`.
Many views per window and several windows work (each view stores its parent). Overlap caveat: native views sit above
GPUI content, so GPUI popups over a page are hidden behind it; hide or shrink the view while an overlay is open.

## Events

`WebEvent` (all carry `view`): `Created, Closed, TitleChanged, AddressChanged, FaviconUrls, LoadingState,
LoadProgress, LoadStart, LoadEnd, LoadError, NewWindowRequested{disposition}, FullscreenChanged, RenderProcessGone,
Console, PreKey, FocusGained, ContextMenu(ContextMenuInfo), StatusText, FindResult, Permission(PermissionRequest),
DownloadStarting(DownloadRequest), DownloadProgress, CertificateError, PageMessage`.
Added in phase 2: `RequestBlocked, MainFrameResponse, DevToolsEvent, MediaSession, JsDialogRequest, AuthRequest,
CertificateErrorPrompt`; `AudioPlayingChanged` is now emitted (section 10). `PageMessage` gained `frame_id` and `top_url`
(match it with `..`). Still not emitted: `CursorChanged`, `JsDialog` (replaced by `JsDialogRequest`).

- Popups are always cancelled in CEF and reported as `NewWindowRequested` (also for middle click and ctrl click via `OnOpenURLFromTab`).
- `set_key_filter` runs inside `OnPreKeyEvent`; returning `true` swallows key down and its key up. `PreKey` is queued for key downs.
- `ContextMenu` cancels the native menu; the app draws its own and acts with `edit`, `start_download`, `load_url`, and the app's `inspect_at`.
- `PermissionRequest` and `DownloadRequest` carry the CEF callback: answer with `allow()/deny()` and `save_to(path)/cancel()`; dropping denies or cancels.
- Certificate errors: the load is cancelled and CEF shows its error page; the event lets the app react.

## Profiles

`ViewOptions.profile` is `ProfileKind::Persistent(id)` (a `RequestContext` with cache path `<data>/cef/profile-<id>`) or
`ProfileKind::Private(session)` (in memory, empty cache path). Contexts are created on first use and cached.
The path is a direct child of the root cache because Chrome-style profiles must be (a nested `profiles/<id>` fails with
"Cannot create profile"). `drop_profile`, `clear_profile_cache` are provided.

## Custom schemes

`signet` and `signetui` are registered in browser and helper as standard + secure + CORS + fetch enabled
(`CUSTOM_SCHEMES`). `set_scheme_handler(|url| Option<SchemeResponse{mime, body, status}>)`; `None` gives a 404.
The factories are registered on the global context and on every profile context.

## Page scripts and the bridge

`set_page_scripts(&profile, Vec<PageScript{name, source, main_frame_only}>)` stores sources per profile. When a view is created,
the scripts travel to the renderer in the `extra_info` dictionary of `CreateBrowser`. The helper's render process handler
evaluates them in `OnContextCreated`, so they run at document start in each frame (or the main frame only).
The same hook defines `window.__signetPost(channel, jsonString)` (non enumerable, non writable, non configurable), which sends
the process message `signet.page` to the browser; it surfaces as `WebEvent::PageMessage { view, frame_url, is_main_frame, channel, payload }`.
`frame_url` and `is_main_frame` come from the browser-side `Frame`, not from the page. Pages are untrusted: validate `view`, the URL and the payload.
Reverse direction: `execute_js`. DevTools and other browsers not created by the app get no bridge.
Verified: a page script set a flag and the bridge delivered `{"flag":true,"enumerable":false}` from a `signet://` page.

## Phase 2 additions

Everything below keeps the earlier API unchanged. Events still arrive through `drain_events` on the main thread.
`drain_events` also runs the callbacks of finished CDP calls (never call CEF from inside a CEF callback: it is why).

### 1. Page-script bundle (`signet_scripts::Registry::bundle_for`)

Status: verified at runtime (webcheck: bundle in main frame and iframe, before page scripts, `PageMessage` with frame id and top url).

```rust
signet_web::set_frame_bundle(|f: &FrameInfo| registry.bundle_for(&ctx_of(f), RunAt::DocumentStart)); // Send + Sync
signet_web::clear_frame_bundle();
```
`FrameInfo { view, profile: ProfileKind, url, is_main_frame, is_private, frame_id }`. In the renderer, `OnContextCreated` of every
frame (main and iframes, app views only: no DevTools) defines `window.__signetPost` (locked), then evaluates the string
the closure returns, once per document, before any page script, then the legacy `set_page_scripts` sources.
CEF has no synchronous browser/renderer call, so the renderer asks over a loopback socket: `init` starts a server thread,
publishes `SIGNET_WEB_BUNDLE=port:token` (inherited by subprocesses) and the renderer blocks about 100 microseconds per
frame. The closure therefore runs on a helper thread of the browser process: keep it fast, use `Arc`/atomics for
settings (`globalPrivacyControl`, `webauthn_intercept`). Set `SIGNET_WEB_DEBUG=1` to log script errors and connection problems.
The view id, profile and legacy scripts reach every renderer through `extra_info` of `CreateBrowser` (CEF calls `OnBrowserCreated`
with it in each renderer process the browser gets, including after cross-process navigations: verified). If an entry is ever missing the
renderer asks the browser over the same socket (defensive fallback). The server address also travels as the child switch
`--signet-bundle=port:token` (set in `on_before_child_process_launch`), the env var `SIGNET_WEB_BUNDLE` is the fallback.
Why the loopback request and not a registry sent to the renderer: the closure depends on live app settings and returns a string computed per
URL by `signet-scripts`; shipping the registry would force a second implementation of the matching in the renderer. The cost is about 0.1 ms per
frame. With `SIGNET_WEB_DEBUG=1` the helpers log `[signet-web renderer <pid>]` lines (process start, `on_browser_created`, `on_context_created`
with the frame URL, bundle size fetched), which is the first thing to read when injection seems dead.

Root cause of the "injection does not run" bug (most likely, since the first run with a freshly built helper worked at once): the `signet-helper` binary inside the bundle's helper apps was stale (built before the render
process handler code existed), so no renderer ran it. Always rebuild `signet-helper` and re-copy it into the `Renderer` helper app
(`cargo build -p signet-helper` then bundle) whenever signet-web changes; the app bundling script must not cache it.

Replying to the page: `view.execute_js_in(&FrameTarget, code) -> usize` with `FrameTarget::{Main, Focused, Id(frame_id), Url(url), All}`;
use `FrameTarget::Id(frame_id)` from `PageMessage` with `signet_scripts::bridge::dispatch_js` / `Reply::to_js()`. `view.frames()`
lists `FrameDescription { id, url, name, is_main }`. `PageMessage { view, frame_url, is_main_frame, channel, payload, frame_id, top_url }`:
`frame_url` and `top_url` come from the browser process.

### 2. Network hooks

```rust
signet_web::set_request_filter(|r: &RequestInfo| -> RequestDecision { .. }); // Send + Sync, IO thread
signet_web::set_response_hook(|r: &ResponseInfo| -> ResponseDecision { .. }); // Send + Sync, IO thread
view.insert_css(&FrameTarget::Main, "key", css); view.remove_css(&FrameTarget::Main, "key");
```
`RequestInfo { view, url, method, resource_type: ResourceType, first_party_url (top frame URL), is_main_frame, initiator }`;
`ResourceType::dnr_name()` gives the declarativeNetRequest name. `RequestDecision::{Allow, Block, Redirect(url), AddHeaders(Vec<(name, value)>)}`
(headers overwrite same-named ones: `Sec-GPC: 1`; redirect is an internal redirect, used for HTTPS upgrades). Blocked requests emit
`WebEvent::RequestBlocked` (counters). `ResponseInfo { view, url, status, mime_type, headers, is_main_frame }` is called for main and sub
frame responses; `ResponseDecision::{Continue, Redirect(url)}` restarts the request on another URL (JSON viewer). Every main frame
response also emits `WebEvent::MainFrameResponse { view, url, status, mime_type }`, which needs no hook. `insert_css` uses a
constructed stylesheet (not blocked by page CSP), falling back to `<style>`; for CSS before first paint return the same code from the bundle hook.
Not covered: the filter sees the requests of app views only (DevTools and the `signet://` handler of CEF internal pages are included
if they go through the network stack; filter on `url`). Request body modification and response header rewriting are not offered.

### 3. DevTools protocol

```rust
view.execute_devtools_method("Page.captureScreenshot", r#"{"format":"png"}"#, |r: Result<String /*result JSON*/, String>| ..);
view.devtools_subscribe(&["Media.playerPropertiesChanged"]); // then WebEvent::DevToolsEvent { method, params }
view.capture_screenshot(&ScreenshotOptions { clip: Some(Bounds), format: ImageFormat::Png, scale, capture_beyond_viewport }, |r: Result<Vec<u8>, String>| ..);
view.set_device_emulation(Some(&DeviceEmulation { width, height, device_scale_factor, mobile, user_agent, touch })); // None clears
view.set_user_agent_override(Some("..")); // None resets
view.save_page_mhtml(path, done); view.save_page_html(path, done); view.clear_site_data(origin, done);
```
One observer per view stays registered for the view's life (keeps `Emulation` overrides). Callbacks run on the main thread from `drain_events`.
A hidden view (`set_visible(false)`) produces no frames, so screenshots of background tabs hang or fail: capture the visible tab just before
switching away and cache the image. `mobile: true` without a viewport meta lays the page out at 980 px like a phone.

### 4. Audio state and media session (no silencing)

Status: verified at runtime (webcheck: `AudioPlayingChanged` true/false, `MediaSession` with actions, `media_control(Next)`).

CEF only exposes audio through a capturing `AudioHandler` (it redirects the sound: the page becomes mute), and CDP `Media` has no
audible flag. So `media.js` (embedded, runs in every frame of app views, own V8 function, nothing exposed to the page except the
inert `__signetMediaCtl`) tracks `<audio>`/`<video>` elements (playing, not muted, volume above 0, has audio) and wraps
`navigator.mediaSession` (`metadata`, `playbackState`, `setActionHandler`). `window.MediaSession` does not exist yet when CEF calls `OnContextCreated`, so the wrapper is installed lazily, on the first read of `navigator` by the page (or at the first flush); it is therefore in place before any page script. Events: `AudioPlayingChanged { view, playing }` (any frame; reset on
navigation, crash, close) and `MediaSession { view, info: MediaSessionInfo { title, artist, album, artwork[{src,sizes,mime}], playback_state, playing,
actions, frame_id, duration, position } }` (sent when title/artist/artwork/state/actions change). Control: `view.media_control(MediaCommand::{Play,
Pause,Toggle,Next,Previous,SeekBackward,SeekForward})` calls the page's registered handler, else acts on the media elements. WebAudio-only pages
(no media element) are not detected. `view.set_audio_muted` still works.

### 5. Overlay windows (`signet_web::overlay`)

```rust
overlay::attach_child_window(parent: RawWindowHandle, child: RawWindowHandle) -> Result<(), String>
overlay::detach_child_window(parent, child); overlay::set_click_through(window, on); overlay::configure_overlay_window(window);
overlay::raise_window(window); overlay::focus_parent_window(parent)
```
macOS (`mac::*`, same names): `addChildWindow:ordered:NSWindowAbove` (verified: above the native CEF views, follows the parent), `setIgnoresMouseEvents:`,
transparent background and no shadow. Windows: owner window (`GWLP_HWNDPARENT`), click-through with `WS_EX_LAYERED|WS_EX_TRANSPARENT`. Linux: `XSetTransientForHint`,
click-through with an empty XFixes input shape; Wayland handles are refused (run under XWayland like CEF). Windows and Linux code is compiled by design only.
Create the GPUI window with `WindowBackgroundAppearance::Transparent`, `titlebar: None`, `focus: false`, attach it after creation, and keep its bounds aligned with the parent yourself.

### 6. Dialogs, auth, certificates, close

```rust
signet_web::set_js_dialog_mode(JsDialogMode::Events); // default Native (platform dialogs)
WebEvent::JsDialogRequest { view, request } // request.kind (Alert/Confirm/Prompt/BeforeUnload), origin, message, default_prompt, is_reload
request.accept(Some(text)) / request.dismiss()   // dropping dismisses
WebEvent::AuthRequest { view, request }          // origin, host, port, realm, scheme, is_proxy; request.submit(user, pass) / request.cancel()
signet_web::set_certificate_handler(|e: &CertificateErrorInfo| CertificateDecision::{Cancel, Proceed, Ask});
WebEvent::CertificateErrorPrompt { view, request } // request.info {code,url,cert_status,subject,issuer}; request.proceed() / request.cancel()
view.request_close()  // runs beforeunload; Closed follows when allowed. close() still forces.
```
Without a certificate handler the old behavior stays (cancel + `CertificateError` event). `Proceed` is per request: remember accepted hosts in the app.
`beforeunload` only fires for pages that had a user gesture (Chromium rule).

### 7. History, user agent, zoom, spellcheck, data

```rust
view.navigation_entries() -> Vec<HistoryEntry { index, url, display_url, title, current, http_status }> // oldest first
view.go_to_history_index(index)
view.zoom_percent(), set_zoom_percent(p), zoom_step(up), zoom_reset(); zoom_level_to_percent, percent_to_zoom_level, next_zoom_percent, ZOOM_PRESETS
signet_web::set_spellcheck(&profile, enabled, &["en-US", "fr"]) -> Result<(), String>; view.add_word_to_dictionary(w); view.replace_misspelling(w)
signet_web::clear_profile_data(&profile, ClearData { cookies, cache, http_auth, certificate_exceptions, connections }, done)
signet_web::delete_cookies(&profile, Some(url), Some(name)) -> bool
```
Chromium keeps zoom per host and profile: every tab of that site follows `set_zoom_percent`. Web storage (localStorage, IndexedDB...) has no profile-wide
call: `view.clear_site_data(origin, done)` per origin, or close the views and `drop_profile` for a private session.
"Save page as": `save_page_mhtml` (single file with resources) and `save_page_html` (DOM only). "View source" is `view_source()`.

### 8. Picture in picture

CEF 156 with the Alloy runtime (the only one that embeds as a child view) has no Picture-in-Picture UI: the browser-side PiP window manager belongs to
Chrome's UI layer, so `video.requestPictureInPicture()` is not backed by a window (do not rely on it; Document PiP likewise). CEF offers no PiP API. The custom mini
player of the Electron app is the way: an overlay window (section 5) showing the title, artwork and buttons from `MediaSession` and driven with `media_control`
(play, pause, next, previous, seek). It cannot show the video pixels. If video in the mini window is wanted, the tab's view can be reparented into a small
overlay window (`WebView::create` in the overlay window, same URL and profile, `execute_js` to seek) at the price of reloading the page; not implemented.

### 9. Keyboard focus between GPUI and CEF

* GPUI to CEF: `view.focus()` (makes the CEF view first responder). A click on the page does it natively and emits `FocusGained`: blur the focused GPUI input then.
* CEF to GPUI: on macOS the patched `-[NSApplication sendEvent:]` gives the window content view the first responder on every mouse down that lands outside the CEF views while a
  CEF view holds the focus (verified, see below). For keyboard-initiated hand-offs (a shortcut that opens the command bar) call `view.blur()` or `overlay::focus_parent_window(parent)`.
  Windows and Linux: call `view.blur()` from the GPUI mouse down handlers of the non-web areas.
* Shortcuts while a page has focus: `set_key_filter` runs in `OnPreKeyEvent` before the page and before the macOS menu. Return `true` for Cmd+Q, Cmd+W, Cmd+L (and every app shortcut): the key is
  swallowed (down and up) and `PreKey { handled: true }` tells the app to run the action. Do not rely on GPUI key bindings or the menu for those keys while a CEF view has focus.
  Overlay windows are separate key windows: a click in one makes it key, so its inputs get the keys; call `view.focus()` when it closes.

## Platforms

- macOS: built and run here (NSView parent).
- Windows: `HWND` parent, `SetWindowPos`/`ShowWindow`, DPI via `GetDpiForWindow`. Compiles by design only, untested.
- Linux: X11 window id parent (`XMoveResizeWindow`, `XMapWindow` through `x11-dl`). Wayland has no child windows for CEF:
  run under XWayland (the Wayland handle is rejected with a clear error). Untested.

## Known limitations

- `examples/webcheck.rs` (run from a bundle, see its header) covers phase 2. Last run: everything passes (bundle and `PageMessage`, media, filters, CDP, history, zoom, CSS, JS dialogs, beforeunload, focus, key filter, overlays) except `set_spellcheck` (preference `spellcheck.dictionaries` refused) and `AuthRequest`: CEF 156 (Alloy) never calls `RequestHandler::get_auth_credentials` for a 401 Basic main frame (load fails with `ERR_INVALID_AUTH_CREDENTIALS`, -338) although the handler is installed and wired; the event path itself now goes through the IO queue (the callback is documented as IO thread) but is unverified. Open.

- No screenshot was possible on the build machine (screen capture permission), so the visual result was verified through
  native view frames (`debug_native_frame`: frame `(240,0,960,801)` inside a 1200x801 parent), load events and titles, not by eye.
- Live window resize and focus hand-off between GPUI and CEF views were not exercised interactively.
- `use-mock-keychain` is passed on macOS (no keychain prompt, cookies not protected by the login keychain).
- `CursorChanged`, `JsDialog` not emitted; fullscreen is reported but the app must resize the view.
- Alloy runtime style is requested for child views (Chrome style does not embed natively); it is deprecated upstream.

## Extension support (additive)

- `chrome-extension` is a fifth custom scheme (standard, secure, CORS, fetch enabled; one origin per host). `set_extension_handler(|req: &ExtensionRequest| Option<ExtensionResponse>)` serves it (IO thread, `Send + Sync`): `ExtensionRequest { url, referrer, method }` lets the app apply `web_accessible_resources` to requests made by web pages (empty or `chrome-extension://` referrer: the extension's own); `ExtensionResponse { mime, body, status, headers }`. Registered with the other schemes on the global context and on every profile context. Unverified: whether Chromium in the Alloy runtime accepts a second registration of this name.
- `WebView::create_background(parent, profile, url)`: a 2x2 view parked outside the visible area (not hidden, so not throttled), for extension backgrounds and offscreen documents. Main thread.
- `set_preference_json(name, &serde_json::Value)`: sets a Chromium preference (`proxy`, `webrtc.ip_handling_policy`) on every profile now and on profiles created later. Main thread.

## Developer tools (remote debugging frontend)

`CefBrowserHost::ShowDevTools` aborts the browser process in this setup (CHECK failure with the default window info,
"Alloy style is not supported for this browser" with the Alloy style), so there is no `show_devtools`. The tools are an
ordinary view that loads the DevTools frontend bundled in CEF from Chromium's remote debugging server:

```rust
let port = signet_web::pick_loopback_port();            // free random port on 127.0.0.1
config.remote_debugging_port = port.map_or(0, i32::from); // before init; 0 = off (the tools are then unavailable)
WebView::devtools_available();                           // true when the server runs
page.devtools_frontend_url(|r: Result<String, String>| ..); // CDP Target.getTargetInfo on the page's own session -> targetId
// -> http://127.0.0.1:<port>/devtools/devtools_app.html?ws=127.0.0.1:<port>/devtools/page/<targetId>
signet_web::is_frontend_url(url);                        // only this origin and path prefix may load in a tools view
```

The app creates the tools view with `WebView::create` (in-memory profile) and places it like a tab (docked) or under
the header of the `#devtools` window. `devtools_app.html` is used rather than `inspector.html`, which adds a
screencast of the page on the left. The frontend opens a WebSocket from its own origin, so `--remote-allow-origins=
http://127.0.0.1:<port>` is added to the command line; Chromium binds the server to the loopback address only (log line
`DevTools listening on ws://127.0.0.1:<port>/...`). A `panel=console` parameter on the frontend address opens it on
the console; selecting an element is done by running a script in the frontend (dynamic import of its own modules,
`DOMModel.nodeForLocation` then `Revealer.reveal`) because the frontend's CDP session is not reachable from CEF.

Security: while the server runs, any local process that finds the port (a scan of loopback ports is enough) controls the whole browser through the DevTools protocol: pages, cookies, and the
profiles of every view. Mitigations in place: random port per launch, loopback only, a frontend origin that is
restricted to the exact `127.0.0.1:<port>/devtools/` prefix (the tools view navigates to `about:blank` on anything
else), `remote-allow-origins` limited to that origin. Not possible with CEF: opening or closing the port at runtime
(it is a startup setting), so the server runs for the whole session unless the app starts with `SIGNET_NO_DEVTOOLS=1`
(the app then leaves `remote_debugging_port` at 0 and the tools stay closed).

## Page view size on macOS

Chromium creates a new `RenderWidgetHostViewCocoa` on the first navigation (and on every renderer swap) with a small
default frame (560x342) and sizes it a few frames later: the page showed small for a frame, then jumped to the pane
size (the "blink"). `mac::install_page_view_fill` forces that view to the bounds of its `WebContentsViewCocoa` when it
is attached and on every `setFrame:`. `SIGNET_TRACE_VIEWS=1` logs every change of the native view tree (frames, layer
scales) to stderr; `SIGNET_NO_PAGE_FILL=1` turns the fix off to compare. On X11 and Win32 the browser is created at
its pixel size (`platform::initial_size`).

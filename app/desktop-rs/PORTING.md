# Signet in Rust (GPUI + CEF): porting plan

This folder is a full rewrite of the desktop app (`app/desktop`, Electron) in Rust.

- The **interface** is drawn with [GPUI](https://www.gpui.rs) (`gpui-pre` 0.3.11, the version `gpui-component` 0.9 pins) and [`gpui-component`](https://github.com/longbridge/gpui-component).
- The **web pages** are rendered by Chromium through CEF ([`cef`](https://github.com/tauri-apps/cef-rs) 156). Each tab is a native CEF child view placed inside the GPUI window, like the `WebContentsView`s of the Electron app.
- The **behavior** is the one described in [docs/SPEC.md](../../docs/SPEC.md). The Electron code (`app/desktop/src`) and the shared package (`packages/shared/src`) are the reference: port their rules, do not reinvent them.
- The app must run on **macOS, Linux and Windows**. Only macOS is built and run on this machine. Platform code goes behind `#[cfg(target_os = "...")]`, and every platform gets a code path, even if untested.

The Electron app stays untouched. Nothing under `app/desktop`, `app/mobile` or `packages/shared` is edited by this port.

## 1. Crates

```
app/desktop-rs/
  Cargo.toml            workspace
  crates/
    signet-core/        state, model rules, persistence, shared utilities, i18n, settings, shortcuts. No GPUI, no CEF.
    signet-features/    data features: passwords, autofill, bookmarks, importers, backup, notes, live folders, boosts, mods, web apps, ad blocker, zap.
    signet-link/        phone link and passkeys over the phone (hybrid/caBLE, CTAP2): protocol layer, no UI.
    signet-extensions/  Chrome extensions: manifest, install (Web Store CRX, unpacked), the Signet extension APIs logic (DNR engine, url match, commands...).
    signet-scripts/     page scripts (port of app/desktop/src/preload/*): the JS injected in tabs, and the Rust side of their channels (message types, sender checks).
    signet-pages/       internal web pages rendered by CEF: Note, Easel, JSON viewer, responsive mode frame; served through the signet:// scheme.
    signet-web/         the CEF layer: startup, browser views, profiles, events, page scripts, schemes, downloads, permissions, find, zoom, devtools.
    signet-ui/          GPUI: theme, icons, widgets, then every screen (sidebar, tabs, command bar, overlays, settings, library, windows).
    signet-app/         the `signet` binary: composition root (the `Browser`), windows, menus, shortcuts, wiring of the other crates.
    signet-helper/      the CEF subprocess binary (renderer, GPU, utility processes).
  scripts/              bundling (macOS .app with CEF framework and helper apps), run helpers.
```

Who may depend on whom:

```
signet-app --> signet-ui, signet-web, signet-features, signet-link, signet-core
signet-ui --> signet-core
signet-features --> signet-core
signet-web --> (nothing internal; generic CEF wrapper)
signet-link --> (nothing internal)
signet-extensions, signet-scripts, signet-pages --> signet-core (types only), nothing else internal
signet-helper --> signet-web (only its subprocess entry point)
```

`signet-ui` never talks to CEF or to the disk. It reads state and calls actions through the `SignetApi` trait (port of `app/desktop/src/shared/types/api.ts`, lives in `signet-core::api`), implemented by `signet-app`. This replaces the Electron IPC contract.

## 2. Mapping from the Electron app

| Electron                                         | Rust                                                     |
| ------------------------------------------------ | -------------------------------------------------------- |
| `packages/shared/src`                            | `signet-core` (and `signet-features` for autofill, passwords, bookmarks, feeds helpers it owns) |
| `app/desktop/src/shared` (types, shortcuts)      | `signet-core` (`types`, `api`, `shortcuts`)              |
| `main/core`, `main/model`                        | `signet-core` (`store`, `history`, `model`)              |
| `main/features/*`                                | `signet-features` (data and rules) + `signet-app` (wiring) + `signet-ui` (screens) |
| `main/features/link`, `main/web/hybrid`          | `signet-link`                                            |
| `main/web/*`, `preload/*` (page scripts)         | `signet-web` (CEF) ; page scripts are injected JS + CEF process messages |
| `main/browser/*`, `main/windows/*`, `main/app/*` | `signet-app`                                             |
| `renderer/src/*` (React)                         | `signet-ui` (GPUI views)                                 |
| IPC contract (`ipc/`, `preload/index.ts`)        | `SignetApi` trait + `UiEvent` enum (`signet-core::api`)  |
| `state.json`, `bookmarks.json`...                | same JSON formats, read and written with serde           |

## 3. Data

- Data folder: `<config dir>/Signet GPUI/` (macOS: `~/Library/Application Support/Signet GPUI/`). The CEF root cache is `<data>/cef/`, each persistent profile is `<data>/cef/profile-<id>` (CEF requires direct children of the root cache).
- The JSON files keep the **same format** as the Electron app (`state.json`, `history.json`, `bookmarks.json`, `autofill.json`...), so serde types must accept the existing files (`#[serde(default)]`, `rename_all = "camelCase"`, unknown fields kept or ignored).
- First launch: if the Rust data folder is empty and `<config dir>/Signet/` exists, its JSON files are **copied** (never moved, never written) to import the user's spaces, tabs, bookmarks, history and settings.
- macOS bundle identifier: `dev.signet.app.gpui` (distinct from the Electron `dev.signet.app`, so both can be installed).

## 4. Rules for everyone working here

- Language: code, comments and docs in **English**. Never use em or en dashes, anywhere: use real punctuation or `-`.
- Comments: short and useful, no talking to yourself.
- Stay in your crate. Only edit files of the crate (or the module) you were given. Read anything else freely. Add dependencies in **your crate's** `Cargo.toml` with explicit versions; do not edit the workspace `Cargo.toml` (ask through your report instead).
- Never edit `app/desktop`, `app/mobile`, `packages/shared`.
- Never commit, never create branches, never run `git` commands that change anything (`status`, `diff`, `log` are fine).
- Tests: port the Electron unit tests of what you port (`*.test.ts` next to the file) as Rust `#[cfg(test)]` tests. `cargo test -p <crate>` must pass.
- No `unwrap()` on user data or I/O: return errors, or fall back like the Electron code does.
- Builds share 8 GB of RAM: use `CARGO_BUILD_JOBS=4`. Crates without GPUI/CEF use their own target folder so they do not wait on the GPUI build:
  - `signet-core`: `CARGO_TARGET_DIR=target-core`
  - `signet-features`: `CARGO_TARGET_DIR=target-features`
  - `signet-link`: `CARGO_TARGET_DIR=target-link`
  - `signet-extensions`: `CARGO_TARGET_DIR=target-extensions`
  - `signet-scripts`: `CARGO_TARGET_DIR=target-scripts`
  - `signet-pages`: `CARGO_TARGET_DIR=target-pages`
  - `signet-web`, `signet-ui`, `signet-app`, `signet-helper`: the default `target/`
- Running the app opens real windows on the user's screen. Launch it only when you need to see a result, always with a time limit, and kill every process you started before you finish (`pkill -f "Signet GPUI"`, `pkill -f signet-helper`). Never leave the app running. Never loop on launches.

## 5. Build and run (macOS)

```sh
cd app/desktop-rs
./scripts/run-macos.sh          # builds, bundles dist/Signet.app (CEF framework + helpers), runs it
./scripts/bundle-macos.sh       # bundles only
cargo test -p signet-core       # unit tests (with CARGO_TARGET_DIR=target-core)
```

(The scripts are written by the `signet-web` foundation work; see section 6.)

## 6. Status

| Area                                                        | Crate             | State |
| ----------------------------------------------------------- | ----------------- | ----- |
| CEF foundation: startup, child views, bundle, message loop  | signet-web        | done (phase 1) |
| Types, store, history, shared utilities, i18n, model rules  | signet-core       | done (phase 1, 272 tests) |
| Theme, icons, widgets                                       | signet-ui         | done (phase 1) |
| Phone link, hybrid passkeys protocol                        | signet-link       | done (phase 1, 204 tests) |
| Data features                                               | signet-features   | done (phase 1, 227 tests) |
| Page scripts                                                | signet-scripts    | done (phase 1) |
| Internal pages (Note, Easel, JSON viewer)                   | signet-pages      | done (phase 1) |
| Extensions logic                                            | signet-extensions | in progress |
| Screens                                                     | signet-ui         | phase 2 |
| Composition root, windows, menus, shortcuts                 | signet-app        | phase 2 |
| Web layer additions (request hooks, overlays, CDP, ...)     | signet-web        | phase 2 |
| Wiring of features, scripts, pages, link, extensions        | signet-app        | phase 3 |

Each crate has a README with its API: read the README of every crate you use before writing code against it.

## 7. Integration notes (to resolve when wiring signet-app)

- Notes and Easel addresses (resolved): `signet://note/<id>` and `signet://easel/<id>` everywhere, like the Electron tabs store them. signet-app must leave `ScriptOptions::default().notes_pages` empty.
- Bridge names: `__signetPost` (page to browser, defined by signet-web before the bundle runs) and `__signetDispatch` (browser to page, defined by the bundle). signet-web injects `signet_scripts::Registry::bundle_for(...)` once per document; it must not inject scripts one by one.
- `signet-app` needs `gpui_platform` (workspace dependency) to create the `Application`, with `SignetAssets` from signet-ui.
- Screenshots (`screencapture`) do not work from the agents' sandbox: visual checks are left to the user.
- Info.plist keys needed by signet-link (bundler): `NSBluetoothAlwaysUsageDescription`, `NSLocalNetworkUsageDescription`, `NSBonjourServices` = [`_signet._tcp`]; entitlement `com.apple.security.device.bluetooth` if hardened runtime is on.
- Passkeys page flow: `signet_scripts` channel `webauthn:request` goes to `signet_link::hybrid::webauthn_precheck` then `HybridManager::start_hybrid`; the QR window (port of `windows/passkey.ts` UI) is drawn by signet-ui.

- Build env: `export CEF_PATH=$PWD/.cache/cef CARGO_BUILD_JOBS=4` from `app/desktop-rs` (the CEF distribution is cached in `.cache/cef`). Always run cargo from `app/desktop-rs`, never from inside a crate (that creates stray target folders).
- Native CEF views are drawn ABOVE all GPUI content of their window. Anything that must float over a page (command bar, popovers, Peek, find bar, toasts, hover preview, floating sidebar/toolbar, mini player) lives in its own transparent GPUI window placed over the main window, exactly like the Electron app used separate `WebContentsView`s (`UiView`).

## 8. Screens (signet-ui) and the app (signet-app)

How a screen is built:

- Screens live in `crates/signet-ui/src/screens/<module>/`. Each module is owned by one agent; `screens/mod.rs` already declares them all.
- Port the React component faithfully: same structure, same states, same gestures and shortcuts, same sizes and colors (read its CSS in `renderer/src/styles/`). Use `signet_ui::theme` tokens and `signet_ui::widgets`; never hard-code colors.
- State and actions come from `signet_ui::app_state`: `api(cx)` returns the `Rc<dyn SignetApi>` (call actions like `api(cx).close_tab(&id)`), `ui_state(cx)` is the shared `Entity<UiState>` (snapshot, locale, appearance); observe it to re-render, subscribe to it to receive one-shot `UiEvent`s (`RenameTab`, `EditSpace`, `OverlayShow`...). Never call CEF or the disk from a screen.
- Texts: `app_state::tr("key")` / `tr_args("key", &[("name", value)])` with the same keys the React code passes to `t(...)` (catalog `packages/shared/src/locales/*/ui.json`).
- Each screen exposes a root view type with `pub fn new(window: &mut Window, cx: &mut App) -> Entity<Self>` (plus parameters if needed). The app creates the windows and wraps each root in `gpui_component::Root`.
- Surfaces map 1:1 to the Electron routes (`main.tsx`): main window `''` (`main_window::MainWindow`), `#overlay` (`overlay::OverlayRoot`: command bar, popovers, Peek frame, capture, password prompt, Boost, zoom bubble), `#sidebar` (floating sidebar), `#toolbar` (floating compact toolbar), `#find`, `#toast`, `#preview`, `#pip`, `#quit`, and the separate windows `#little`, `#private`, `#settings`, `#library`, `#webapp`, `#devtools`, `#welcome`, passkey. The app decides window kind, size and position like the Electron main process does.
- Web areas: the app computes where web views go (port of `browser/layout.ts`) and publishes them in `Snapshot.panes` and `Snapshot.dividers`. The main window draws the pane chrome (pane bar, card corners, dividers, drop zones) around those rectangles and leaves the web area itself empty.
- Verification: `cargo check -p signet-ui` and `cargo test -p signet-ui`. Screenshots do not work from the agents' sandbox, so do not try to look at windows.

The app (`signet-app`) implements `SignetApi`, owns the `Store`, the `Model`, the CEF views, the windows and all the wiring (phase 2: core browser; phase 3: features).

## 9. Phase 3 backlog (collected from reports)

- Data folder: signet-app uses `dirs::data_dir()/Signet GPUI`, signet-core `paths` uses `dirs::config_dir()/Signet GPUI` (same on macOS and Windows, different on Linux). Use `signet_core::paths::data_dir()` everywhere.
- Windows exe icon: add a `build.rs` with `winresource` in signet-app (cfg windows).
- Linux/Windows helper: `default_helper_path` returns the main exe; either ship and use `signet-helper` or drop it from the bundles.
- `signet_core::core::https_only::HttpsOnly` (port of browser/https-only.ts) is ready: wire it to the signet-web request filter and error pages.
- Extension toolbar buttons: signet-ui exposes `screens::toolbar::BrowserActions` (state/activate/subscribe per partition) and `install_browser_actions(Rc<dyn BrowserActions>, cx)`; signet-app implements it over `signet_extensions::ExtensionHost` (toolbar(), button_state(), action_clicked()). No button is drawn until it is installed.
- Main window chrome on macOS: hidden title bar, `cx.signet().window_background()`, traffic lights from `screens::main_window::traffic_lights(horizontal, sidebar_right, compact)`, re-applied on layout change.
- Settings window: the app calls `SettingsWindow::open_section(key, window, cx)` when something asks for a section (no UiEvent for it); it sets the window hue from `SettingsInfo.hue`; `set_shortcut_recording(true)` must suspend menu accelerators; `clean_settings` must accept every camelCase key the settings UI sends (check against `screens/settings`).
- Overlay windows: `OverlayRoot` (full main window, or 348x200 password / 296x100 zoom rects per overlay.ts), `FindBar` 360x48, `ToastView` 384x72, `QuitHud` 320x120, `BoostWindow` normal window. They call `overlay_ready()`: the app then sends the pending payload. Only Peek leaves a rect empty for a native view (`card`).
- `zoom_page(tab, delta: i32)`: the overlay sends +-1; React sent +-0.5. Treat the delta as one step of the zoom table in the app.
- `palette_suggest` is synchronous on the UI thread: the app must answer from cache and fetch network suggestions in the background, then push a fresh `OverlayShow`/state to refresh.
- Windows native view rects: Little `x=6,y=46,w=W-12,h=H-52`; WebApp `y=38` (bar 38); DevTools window `y=36`; Private: tabs on top `x=6,y=84,w=W-12,h=H-90`, sidebar `x=side,y=46,w=W-side-6,h=H-52`. Pip: transparent window over the video view; implement `screens::windows::pip::PipHost` + `install_pip_host` for move/resize.
- Per-window hue: GPUI theme is global; the app sets the theme hues for the focused window (Little/WebApp/Library/Welcome use their site hue, Private uses 285).
- Recovery: GPUI has no error boundary; catch panics in the app if a fallback is wanted.
- Final integration backlog (core files, not owned by a phase 3 module): Peek (open links in Peek per settings, place the native view in the overlay `card` rect), site panel data in `browser/overlay.rs`, network suggestions for the command bar (background fetch, cache), sidebar show/hide slide (`UiEvent::SidebarSlide` steps), status URL bubble, closing the main window should not quit on macOS (Electron behavior check), shortcut rebinding without restart, `open_settings(section)` must call `SettingsWindow::open_section`, the 3 phase 3 lines each agent asked to add in files it does not own.
- `browser/layout.rs` (~line 159): set `d.toast` only when `!self.sidebar_on()` (Electron `undo.ts`); the sidebar embeds its own archive toast.

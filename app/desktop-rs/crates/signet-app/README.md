# signet-app

The `signet` binary: composition root of the Rust port (port of `app/desktop/src/main`). It owns the store, the
history, the CEF views and the windows, implements `signet_core::api::SignetApi` for the screens of `signet-ui`, and
wires `signet-web` events into the state.

## Module map

| Rust module | TypeScript source |
| --- | --- |
| `main.rs` | `main/index.ts` (startup, single instance, store ticks, quit flush, `SIGNET_AUTOQUIT`) |
| `app/single_instance.rs` | `app.requestSingleInstanceLock` in `index.ts` |
| `app/platform.rs` | `app/platform.ts` (window chrome), clipboard/open/reveal helpers |
| `app/menu.rs` | `app/menu.ts` (menu bar, accelerators from the shortcut table, checkmarks) |
| `browser/mod.rs` (`Core`, `Browser`) | `browser/browser.ts` (composition root), `model` wiring (`Env`) |
| `browser/bus.rs` | `ipc/push.ts` (`UiEvent` fan-out to `SignetApi::subscribe` listeners) |
| `browser/views.rs` | `browser/views.ts` (one CEF `WebView` per loaded tab, per-profile) |
| `browser/layout.rs` | `browser/layout.ts` (panes, dividers, compact, tabs on top, fullscreen, floating rects) |
| `browser/mod.rs` (`changed`, `apply`, `patch_tab`, `snapshot`) | `browser/broadcast.ts` |
| `browser/tabs.rs` | `browser/tab-actions.ts`, `typed-nav.ts` |
| `browser/spaces.rs` | `browser/space-actions.ts` |
| `browser/folders.rs` | `folder-actions.ts`, `group-actions.ts`, `arrange.ts`, bookmark gestures |
| `browser/splitview.rs` | `splitview.ts`, `drag.ts` |
| `browser/undo.rs` | `undo.ts` |
| `browser/shell.rs` | `shell.ts`, hover parts of `floating.ts` and `compact.ts` |
| `browser/sleep.rs` | `sleep.ts` |
| `browser/find.rs` | `find.ts` |
| `browser/overlay.rs` | `overlay.ts` (palette, popovers, zoom bubble; Peek and site panel are PHASE3) |
| `browser/palette.rs` | `palette.ts` |
| `browser/downloads.rs` | `web/downloads.ts` wiring |
| `browser/events.rs` | `createTabView` listeners of `browser/tabs.ts` (title, address, favicon, load, popups, crash, permissions) |
| `browser/context_menu.rs` | context menu of `browser/tabs.ts`, tab/space/folder/group/bookmark menus of `ipc/tabs.ts` |
| `browser/commands.rs` | click handlers of `app/menu.ts`; key table of the page key filter |
| `browser/settings.rs` | `windows/settings.ts` (`update`, `info`), appearance and language broadcasts |
| `browser/win.rs` | `browser/window.ts` (main window events), `app/quit.ts` (Cmd+Q confirmation) |
| `browser/api.rs` | `main/ipc/*.ts` (the `SignetApi` implementation) |
| `windows/` | `windows/*.ts`, `browser/uiview.ts` (GPUI windows, floating surfaces) |
| `scheme.rs` | `web/internal.ts` `attachInternal` (`signet://` handler) |
| `theme.rs` | `broadcastAppearance` (keeps `signet_ui` theme in step with settings and active space) |
| `mac.rs` | `window.ts`/`uiview.ts` native parts: child windows, frames, traffic lights, popup menus |
| `dev/` | `main/dev/{index,seed,capture,probes}.ts` (SIGNET_* sample state and screenshots, `scripts/capture.sh`; only with `SIGNET_DATA_DIR`) |
| `phase3/` | features wired in phase 3 (see below) |

## Startup sequence (`main.rs`)

1. `signet_core::paths::prepare_data_dir()` (first run: copy of the Electron JSON files into `Signet GPUI/`).
2. Single instance: a lock port in `<data>/instance.port`. A second launch sends its URL arguments to the first one
   (JSON line over loopback) and exits; the first one opens them (`Cmd::Urls`).
3. `Store::open` (also initializes the interface language) and `History::new`.
4. `gpui_platform::application().with_assets(SignetAssets).run(...)`, then inside:
   `signet_web::init` (scheme handler set first; the helper is found in the bundle), the key filter, `Core`/`Browser`,
   `gpui_pump::install` (CEF events go to `Core::on_web_event`), `signet_ui::init` with the theme computed from the
   snapshot, `app_state::install(Rc<dyn SignetApi>)`, menus, the window manager loop, the store/history tick,
   forwarded links, and the first `Cmd::ShowMain`.
5. Store and history writes follow deadlines: a foreground task sleeps until `min(store.next_deadline(),
   history.next_deadline())` (clamped to 50 ms to 5 s) and calls `tick`. `prepare_quit` flushes (also registered with
   `on_app_quit`, so every exit path writes).

## Windows and floating surfaces

`Core` never touches GPUI. It sends `Cmd`s (`windows/mod.rs`) to a loop on the GPUI foreground executor
(`windows::run`), which owns the `Manager` (`windows/manager.rs`).

- Main window: `MainHost` (`windows/host.rs`) wraps the `MainWindow` screen and reports size and screen position on
  each render (`Core::on_main_resized`). The CEF views are native children of this window; their rectangles come from
  `layout_update` and are also published in `Snapshot.panes` and `dividers` for the pane chrome.
- Floating surfaces (overlay, floating sidebar, floating toolbar, find bar, toast, tab preview, quit HUD) are
  transparent borderless GPUI windows over the main window, because native views are drawn above all GPUI content
  (PORTING.md section 7). `layout_update` computes the wanted rectangles (`Desired`, window coordinates); each change
  is sent as `Cmd::Sync` and the manager opens, moves (macOS: `setFrame:`) and closes windows. On macOS they are
  children of the main window (`addChildWindow:`), without shadow or traffic lights.
- Secondary windows (Settings, Library, Welcome) are opened once and focused when asked again (`Cmd::OpenWin`).
- The overlay is subscribed to `UiEvent::OverlayShow`; `overlay_ready` re-sends the pending payload because the
  overlay window is created after the event.
- Native menus: the menu bar is GPUI's (`app/menu.rs`, one `RunCommand { id }` action). Context and tab menus are
  native `NSMenu` popups on macOS (`mac::popup`), shown by `Cmd::Popup`; the callback gets the chosen id.

## Data flow

- Gesture: screen -> `SignetApi` method on `Browser` -> `Core` method -> `Model` rules -> `changed()` -> `apply()`:
  group invariant, `layout_update`, snapshot or `StatePatch` on the bus, menu rebuild if the menu key changed,
  `Cmd::Sync` for surfaces, `store.schedule`.
- Engine event: `WebEvent` -> `Core::on_web_event` -> `patch_tab` (batched over 32 ms by `Cmd::SoftApply`, sent as a
  `StatePatch` when only already-sent tab fields changed).
- App shortcuts while a page has focus: the key filter checks a table (`commands::build_key_table`) rebuilt when
  shortcuts change; a swallowed key arrives as `PreKey { handled: true }` and runs `Core::run_command(id)`.

## PHASE3 stubs (`src/phase3/*.rs`)

Each method logs `PHASE3: name` with `SIGNET_DEBUG=1` and returns a neutral value.

| file | API methods |
| --- | --- |
| `passwords.rs` | list_passwords, reveal_password, copy_password, save_password, delete_password, import_passwords, export_passwords, install_password_extension, answer_password_prompt, open_password_key |
| `autofill.rs` | list_autofill, save_address, delete_address, save_card, delete_card, reveal_card |
| `boosts.rs` | save_boost, start_zap, stop_zap, unzap, delete_boost, open_boost |
| `mods.rs` | list_mods, save_mod, delete_mod, set_mod_enabled, move_mod, set_mods_disabled, import_mod, export_mod |
| `live_folders.rs` | save_live_folder, refresh_live_folder, has_github_token, set_github_token |
| `backup.rs` | get_backup_info, export_backup, open_backup, open_safety_backup, apply_backup, reveal_safety_backups |
| `import.rs` | run_import (and `SettingsInfo.import_sources`, empty) |
| `extensions.rs` | open_extension_store, remove_extension, load/reload/remove_unpacked_extension, show_extension_menu, show_extensions_menu, get_extension_action_counts, get_extension_action_states, ack_extension_action |
| `link.rs` | get_link_state, set_link_general, set_link_device, forget_link, start_link_pairing, cancel_link_pairing, get_passkey_prompt, cancel_passkey_prompt |
| `webapps.rs` | install_web_app, list_web_apps, open_web_app, rename_web_app, reveal_web_app, remove_web_app, web_app_ready/home/open_in_main/copy_url |
| `devtools.rs` | set_dev_tools_dock, set_dev_tools_size, show_dev_tools_menu, show_develop_menu, set_responsive, set_responsive_dragging, dev_tools_window_ready (toggle/close/inspect included). The tools are a view loading the DevTools frontend from the loopback remote debugging port (see signet-web README): docked in the pane's `PaneDevtools` rect, or in the `Win::Devtools` window (`windows/host.rs` `ToolsHost` reports its size) under its 36 px header. `SIGNET_NO_DEVTOOLS=1` turns the port, and the tools, off. |
| `page_tools.rs` | take_capture, toggle_reader, toggle_translation, media_toggle, media_skip, pip_command, open_pip, clear_site_data, page commands (capture, reader, translate, save page, PDF, install app, customize site, responsive) |
| `windows.rs` | Little Signet and private windows (open_little_window, open_private_window, little_*, private_*), create_note |
| `system.rs` | set_default_browser, check_for_updates, relaunch_app, choose_download_dir (proxy settings and browsing data are done) |
| `pages.rs` | `on_page_message`: router of `window.__signetPost` channels |

Also left for later and marked in the code: tab hover preview (`show_tab_preview`), Peek (`expand_peek`, external
links of pinned tabs), site panel, `fallback_favicon`, network suggestions in the command bar, favicon download,
navigation history copy (`ModelEnv::save_history`), sidebar slide animation, adblock hook.

## Run

```sh
cd app/desktop-rs
export CEF_PATH=$PWD/.cache/cef CARGO_BUILD_JOBS=4
./scripts/bundle-macos.sh                       # builds and assembles dist/Signet.app
SIGNET_DEBUG=1 SIGNET_AUTOQUIT=12 dist/Signet.app/Contents/MacOS/signet
pkill -f "Signet.app"; pkill -f "Signet Helper"  # always, if a run was interrupted
```

| variable | effect |
| --- | --- |
| `SIGNET_DEBUG=1` | logs startup, views, panes, surfaces, page keys, downloads, load errors, PHASE3 stub calls; prints a `[dump]` of window, views and floating surfaces just before an autoquit |
| `SIGNET_AUTOQUIT=<seconds>` | quits cleanly after that delay (`1` means 8 seconds); the quit path flushes the store and shuts CEF down |

Command line arguments that are not flags are opened as URLs (routed space, else a new tab); a second launch forwards
them to the running instance.

## Known limitations

- After a shortcut customization the menu and key table are rebuilt, but a GPUI key binding bound earlier stays active
  until restart (GPUI cannot unbind without clearing the whole keymap).
- Closing the main window quits the application.
- Linux and Windows: native popup menus return nothing yet (`Manager::popup`), moved floating surfaces are resized
  instead of moved.

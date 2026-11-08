# signet-features

Data and rules of the browser features, with no UI and no CEF. Each feature is a plain Rust module: a struct with
methods, pure functions, or a small state machine whose decisions the app (`signet-app`) turns into actions. Files
keep the **same JSON formats** as the Electron app (in the data folder the app passes in). Page scripts stay JavaScript
and live in `src/assets/`. Each injected JS asset has one owner: the scripts that run in every page (swipe, passwords,
autofill, GPC, identity, WebAuthn, notes) belong to `signet-scripts`; this crate only owns the ones tied to its own
features (reader, translate, zap, emoji, web app page). Types shared with the UI (`WebApp`, `LiveFolder`, `ReaderPrefs`,
`AddressInput`, `AutofillAddress`, `AutofillCard`, `AutofillInfo`, `DownloadInfo`) come from `signet-core::types` and are
re-exported here. Only `autofill::CardInput` stays local (it holds raw `i64` values to validate, core's has `u32`); the
card year is stored as `u32` in core's `AutofillCard` and converted at the boundary.

Conventions:

- Texts shown to the user are never built here: functions take a `Translate` callback
  (`&dyn Fn(&str, &[(&str, String)]) -> String`, backed by `signet-core::i18n`) or return an error key
  (`passwords.error.site`, `backup.error.nothing`...) for the app to translate.
- Network goes through the `http::Http` trait (`UreqHttp` in the app, fakes in tests). Keychain goes through
  `secrets::SecretStore` (`KeyringStore`, `MemoryStore`). No test touches the network or a keychain.
- Blocking calls: run the ones that fetch (feeds, ad lists, updates, translation, icons) off the main thread.
- Tests: `CARGO_TARGET_DIR=target-features cargo test -p signet-features`.

## Passwords (`passwords.rs`, `osauth.rs`)

- TS: `main/features/passwords/passwords.ts`, `main/app/osauth.ts`.
- API: `Passwords::new(data_dir, Arc<dyn Cipher>)` with `list`, `reveal`, `upsert`, `remove`, `add_many`, `import_csv`,
  `export_csv`, `accounts`, `fill`, `submitted` (returns a `PromptRequest`), `answer`, `key_state`, `open_key`,
  `on_navigate`, `flush`. Free functions `normalize_origin`, `parse_csv`, `is_manager`.
  `KeychainCipher::new(&dyn SecretStore)` is AES-256-GCM with a random key in the OS keychain (service
  `Signet GPUI Safe Storage`, account `vault-key`). `import_electron_vault(file, electron_data_dir, &dyn KeyProvider)`
  decrypts the Electron vault (Chromium os_crypt `v10`/`v11`, service `Signet Safe Storage`); the OS prompts at that moment.
  `osauth::AuthGate::new(Box<dyn Authenticator>)` with `authenticate(reason, confirm_fallback)` and the 60 s grace.
  `SystemAuth` runs JXA/LocalAuthentication on macOS and Windows Hello through PowerShell; Linux returns `Unavailable`.
- Files: `<data>/passwords.json` (`{ version: 1, logins: [{ id, origin, username, password (base64), createdAt, updatedAt, lastUsedAt }] }`).
  Rust-written passwords start with the `sg1` marker and cannot be read by Electron (and the reverse), by design.
- Wire: the page script is owned by `signet-scripts` (`js/passwords.js`); it calls `accounts`/`fill`/`submitted` with the
  origin taken from the sending frame; the save prompt UI and
  the key button; the authentication dialog; clipboard clearing after `CLIPBOARD_TTL_SECS`; settings
  (`passwordManager`, `offerSavePasswords`, `neverSavePasswords`).

## Autofill (`autofill/`)

- TS: `main/features/autofill/*`, `shared/autofill/{card,values,classify}.ts`.
- API: `Autofill::new(data_dir, Arc<dyn Cipher>)` with `list`, `upsert_address`, `upsert_card`, `reveal_card`, `menu`,
  `fill`, `submitted`, `next_offer`, `answer`, `on_navigate`. Pure modules: `classify` (field classifier, EN and FR),
  `card` (Luhn, networks, expiry), `values` (names, countries EN/FR table, select matching), `rules`.
- Files: `<data>/autofill.json` (`{ version: 1, addresses, cards }`, card numbers encrypted base64).
- Wire: the page scripts are owned by `signet-scripts` (`js/autofill.js`, `js/autofill-core.js`; the classifier exists
  in both languages on purpose, the JS one runs in the page), dropdown UI, prompt UI, `osauth::AuthGate` passed to `fill`/`upsert_card`.
- Deviation: country names only in English and French (no `Intl.DisplayNames`); extra locales are ignored.

## Bookmarks (`bookmarks.rs`)

- TS: `main/features/bookmarks.ts`, `packages/shared/src/mobile/bookmarks-html.ts`.
- API: tree operations on a `Vec<Bookmark>` (`toggle`, `add`, `move_to`, `rename`, `remove`, `new_folder`, `find`,
  `find_url`, `open_all_urls`), `prune_bookmarks` for what is read from disk, `export_bookmarks_html`,
  `parse_bookmarks_html`. Types `Bookmark` and `BookmarkTarget` come from `signet-core`.
- Files: `bookmarks.json` is owned by the core store.

## Importers (`importers/`)

- TS: `main/features/importers/*`, `FORMATS.md`.
- API: `sources` (per platform locations, `available_sources`, `kinds_of`, injectable `Env`), `arc::parse_spaces_file`,
  `bookmarks::{parse_chrome_bookmarks, parse_safari_bookmarks, parse_xml_plist}`, `chromium::{read_history,
  extension_ids, read_passwords, profile_has}` (SQLite copied to a temp folder first; keys through a `KeyProvider`,
  `SystemKeys` asks the OS), `apply::{apply_bookmarks, plan_spaces}` (a `SpacesPlan` the app turns into core state),
  `read_safari` (macOS `plutil`), `merge_history`.
- Wire: dialogs and permission hints (`is_permission_error`), history store, `Passwords::add_many`, extensions install.

## Backup (`backup.rs`)

- TS: `main/features/backup.ts`.
- API: `.signetbackup` is gzip JSON; `parse_bytes`/`parse_file`, `write_file`, `seal`/`unseal` (scrypt N=2^15 + AES-256-GCM,
  compatible with Electron), `collect`, `counts`, `info`, `export_to`, `prepare` (decrypts passwords), `import`
  (safety copy then `apply_sections`), `write_safety`, `safety_list`, `open_safety`, `take_pending_extensions`.
  Works on the profile folder as JSON values; app services come in through the `Hooks` trait (flush, extensions,
  vault encryption, settings and theme validation from signet-core).
- Wire: file dialogs, authentication before exporting passwords, confirmation, relaunch right after `import`.

## Notes and Easels (`notes.rs`)

- TS: `main/features/notes/notes.ts`, `shared/notes.ts`. `Notes::{read, save, create, remove, list}`, address helpers
  (`note_address`, `parse_note_address`, `resolve_note_address`, `note_of_app_url`), `clean_items`, size limits.
- Files: `<data>/notes/<id>.json`. Wire: page IPC (`load`, `save`, `remove`) answered only for the document named by the
  sender frame's address.
- Address decision: a note tab keeps `signet://note/<id>` (or `easel`) both as displayed and as real address, because
  `signet-pages` serves that address itself (Electron displayed it too and loaded the UI on `#note/<id>`). Tabs imported
  from `state.json` therefore work unchanged. `note_app_url`/`resolve_note_address` (UI base + hash) only matter if a
  hash-routed UI page is configured; the Rust app does not need them. `signet_core::urls::electron_note_url` maps a
  stray Electron real address (`signetui://app/index.html#note/<id>`) to `signet://note/<id>`.

## Live folders (`feeds.rs`, `livefolders.rs`)

- TS: `packages/shared/src/feeds.ts`, `main/features/livefolders/livefolders.ts`.
- API: `parse_feed`, `discover_feed`, `looks_like_html`, `clean_live_source`, `clean_live_folder`; `LiveFolders {http,
  secrets, t}` with `fetch_source`, `normalize`, `set_token`, `has_token`; `select_entries`, `next_seen`, `mark_seen`.
- GitHub token: OS keychain (service `Signet GPUI Safe Storage`, account `github-token`).
- Wire: the folder tree (one tab per entry) with signet-core, refresh timer (`FIRST_DELAY`, `REFRESH_EVERY`).

## Boosts and Mods (`boost.rs`, `mods.rs`, `modscss.rs`)

- TS: `main/features/boost/{boost,emoji}.ts`, `features/zap.ts`, `features/mods/*`.
- API: `Boosts` (the `state.boosts` map: `save`, `zap`, `unzap`, `delete`), `css_of`, `applied_css`, scripts `ZAP_SCRIPT`,
  `EMOJI_SCRIPT`, `EMOJI_CSS` (assets `zap.js`, `emoji.js`); `Mods::{load, save, remove, set_enabled, move_by, set_disabled,
  sheet, import_css}`, `sanitize_mod_css` (no remote loads).
- Wire: insert `applied_css` per tab on load, run the zap script in an isolated world (`WORLD`), edit window, mods sheet
  injected in every interface view.

## Web apps (`webapps.rs`)

- TS: `main/features/webapps.ts`. API: `WebApps` registry (`webapps.json`), `app_for`, `inspect` (manifest and icon choice
  through `Http`), launcher data: `plist`, `mac_script`, `desktop_entry` (Linux), `windows_url_shortcut`, `parse_launch`.
- Wire: windows, install overlay, writing launchers (icns via `iconutil` on macOS), `PAGE_SCRIPT` in an isolated world.
  Bundle id prefix is `dev.signet.app.gpui.webapp.`.

## Ad blocker (`adblock.rs`)

- TS: `main/web/adblock.ts` (Ghostery). Here: Brave's `adblock` crate. Lists: EasyList, EasyPrivacy, uBlock filters,
  privacy, unbreak, badware, quick-fixes, resource-abuse, Peter Lowe: `LIST_URLS` is exactly Ghostery's
  `adsAndTrackingLists` (`fromPrebuiltAdsAndTracking`, @ghostery/adblocker 2.20.3, 14 lists), plus Brave's
  redirect/scriptlet resources.
- API: `Adblock::new(data_dir)`, `load(&dyn Http)` (cache `adblock.dat` for 7 days, falls back to the old cache offline),
  `check_request(url, source_url, resource_type, tab) -> Verdict::{Allow, Block, Redirect(body)}`,
  `cosmetic(page_url) -> Cosmetic { css, script }`, `generic_hide_css`, `set_allowed`/`is_allowed` (per-site allowlist, new),
  `blocked_count`/`reset_blocked` (site panel counter).
- Wire: CEF request hook (before request, resource types mapped by `map_resource_type`), inject cosmetic CSS and the
  scriptlet script at document start, reset the counter on main frame navigation.

## Smaller ones

- `languages.rs` (`main/web/languages.ts`): `expand_languages`, `normalize_languages`, `accept_language`.
- `identity.rs` (`web/identity.ts`): `reduced_user_agent`, `Identity` (native client hints, `READ_HINTS_JS`).
- `gpc.rs` (`web/gpc.ts`): `header_for`, `apply_header`. The page script (`navigator.globalPrivacyControl`) is owned by `signet-scripts`.
- `network.rs` (`web/network.ts`): `order::*`, Chrome match `Pattern`/`Filter`. The Electron hub itself is not ported (CEF
  has its own request handler); the app keeps consumers sorted by `order`.
- `permissions.rs` (`web/permissions.ts`): `Permissions`/`Table` (per origin decisions, `media` grouping, private windows
  inherit denials), `verdict`, `check`, `origin_for`, `external_allowed`. Dialogs and queuing are the app's.
- `downloads.rs` (`web/downloads.ts`): `DownloadList` model, `unique_path`, `safe_file_name`, `progress`, `download_dir`.
- `favicons.rs` (`web/favicons.ts`): `FaviconCache` (400 entries, 32 MB, 60 s retry), `to_data_url`, `fetch`.
- `errorpage.rs` (`web/errorpage.ts`): `load_error_text`, `injection_script`, `insecure_site_script`, `crash_document`.
- `internal.rs` (`web/internal.ts`): `display_url`, `resolve_internal`, `guard_navigation` (returns a `Guard`), shortcuts,
  suggestions, `urls_page`, `version_page`, `not_found_page`, `page_for`, `CSP`.
- `engine.rs` (`features/engine.ts`): secure DNS validation (`secure_dns`), proxy settings opener.
- `updates.rs` (`features/updates.ts`): GitHub releases check (`check`), `is_newer`, `Tracker`, `can_install`.
- `reader.rs` (`features/reader.ts`): preferences, script builders (`open_script`, `probe_script`), re-probe timing.
  `@mozilla/readability` 0.6.0 is embedded (`assets/readability/`, Apache 2.0 `LICENSE.md`): `READABILITY_JS`,
  `READERABLE_JS`, `open_script_default`, `probe_script_default`.
- `translate.rs` (`features/translate.ts`): `service_lang`, `translate_batch` (retries), `install_script`, `Runs`.

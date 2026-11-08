# signet-core

Pure Rust core of the Signet port: state, model rules, persistence, shared utilities, i18n, shortcuts and the API
contract between the interface and the app. No GPUI, no CEF, no platform UI. It reads and writes the exact JSON of the
Electron app (`state.json`, `history.json`, `bookmarks.json`...).

Test: `CARGO_TARGET_DIR=target-core CARGO_BUILD_JOBS=4 cargo test -p signet-core` (from `app/desktop-rs`).

## Module map

| Rust module | TypeScript source |
| --- | --- |
| `types::*` | `packages/shared/src/types/*.ts`, `app/desktop/src/shared/types/*.ts` (all but `api.ts`) |
| `api` (`SignetApi`, `UiEvent`) | `app/desktop/src/shared/types/api.ts` (IPC validation layer not ported) |
| `ids`, `site`, `routes`, `urls`, `searchengines` | `packages/shared/src/{ids,site,routes,urls,searchengines}.ts`, engine table of `mobile/search.ts` |
| `split`, `tree`, `tab_list`, `factory` | `packages/shared/src/{split,tree,tab-list,model}.ts` (`factory` = shared `model.ts`) |
| `i18n` | `packages/shared/src/i18n.ts` + catalogs `packages/shared/src/locales/{en,fr}/{main,ui}.json` |
| `core::store` | `main/core/store.ts` |
| `core::history`, `core::rank` | `main/core/history.ts`, `rank.ts` |
| `core::atomic`, `core::https_only`, `core::dev_url` | `main/core/atomic.ts`, `https-only.ts` (+ `HttpsOnly` state of `main/browser/https-only.ts`, no Electron session: sessions are string keys, the app feeds `set_config` and its request hook calls `upgrade_request`), `dev-url.ts` |
| `urls::SearchState`, `urls::is_app_url`, `urls::electron_note_url` | non-Electron parts of `main/core/urls.ts`; `electron_note_url` maps a stray Electron note address (`signetui://app/index.html#note/<id>`) to `signet://note/<id>` |
| `model::*` | `main/model/*.ts` (`Model` = activation, opening, removal, lists, splits, closed, archive, folders, groups, spaces) |
| `shortcuts` | `app/desktop/src/shared/shortcuts.ts` |
| `devhosts`, `pagesettings`, `slide`, `languages` | `shared/devhosts.ts`, `shared/pagesettings.ts`, `shared/slide.ts`, pure parts of `main/web/languages.ts` |
| `paths` | new: data folders and first-run import from the Electron folder |

Not ported: `ipc/validate.ts` and `contract.ts` (Rust types replace them), `feeds.ts` (belongs to `signet-features`;
core only has `tree::clean_live_source` / `clean_live_folder` which the store needs), `link/` (belongs to `signet-link`;
core has its own small `LinkDevice`/`LinkStatus` copies in `types::link`), `mobile/`.

## Store and write timing

`Store::open(dir, StoreOptions)` loads `state.json`, `bookmarks.json` and `tab-history.json` (migrations and repairs
as in the TS: unknown tabs dropped, split views pruned, groups synced, bad settings reset, corrupt files copied aside
as `*.corrupt-<ts>`). It never fails and never spawns a thread.

Writes are driven by the app (deadline model):

- `store.schedule(SAVE_DELAY)` after a state change (500 ms), `store.schedule(LAZY_DELAY)` for title/icon changes
  (60 s), `store.schedule_bookmarks()` (500 ms), `store.schedule_history()` (5 s, tab histories).
- Each call only records a deadline; an earlier deadline is never pushed back.
- The app loop asks `store.next_deadline()` (ms since epoch) to know when to wake up, then calls
  `store.tick(now_ms)` (writes what is due) or checks `store.due(now_ms)`.
- `store.flush()` writes everything now (shutdown). Writes are synchronous, atomic (tmp + fsync + rename) and skipped
  when the content did not change. `History` has the same `schedule` / `due` / `tick` / `flush`.

`Store` derefs to `ModelData { state, histories, closed, detached }`, which is what the model works on:
`store.state.spaces`, `store.histories`... The store does not watch `histories`: call `schedule_history()` after
changing them.

## Model

```rust
let mut env = MyEnv::new();               // implements ModelEnv: new_id, now, names, destroy_view, save_history...
let mut m = Model::new(&mut store.data, &mut env);
let id = m.open("https://example.com", &OpenOptions { activate: true, ..Default::default() });
m.close(&id);
m.reopen();
store.schedule(SAVE_DELAY);
```

`Model` only borrows, create it per operation. Rules that can be undone return data (`Removed`, `ClearedToday`,
`SpaceRemoved`) to hand back to `restore_removed`, `restore_archive` or `restore_space`. Lists are addressed by
`ListRef` (space index, zone, owning folder id) instead of JS references.

## i18n

`i18n::t("key")`, `t_args("key", &[("name", "x")])` and `tn(Namespace::Ui, ...)` read the bundled catalogs
(`include_str!`). Semantics of i18next as used by the app: nested keys with dots, `{{var}}` interpolation without
escaping, plurals (`key_one` / `key_other` chosen from a `count` argument; French treats 0 and 1 as singular), OS
context keys (`key_windows`, `key_linux`), fallback to English, Mac-style shortcut glyphs rewritten to `Ctrl+Shift+T`
outside macOS. Pick the language with `set_locale(Locale)` or `init_locale(pref, &system_languages)`
(`resolve_locale` accepts `fr-CA`, `FR_fr`, `fr_FR.UTF-8`). `t` looks in `main` then `ui`; use `tn` when a key exists in
both. `I18n::new` builds an independent translator from JSON text (tests, extra catalogs).

## Implementing `SignetApi`

`signet_core::api::SignetApi` has one synchronous method per method of the TS `SignetApi` (snake_case, value returned
directly, `()` for fire-and-forget). Everything the main process pushed (`onX` subscriptions) is a `UiEvent` delivered
to listeners registered with `subscribe(Box<dyn Fn(UiEvent) + Send + Sync>) -> Subscription` (dropping the
subscription unsubscribes). Listeners run on the producing thread: hand the event to the UI thread yourself. The app
implements the trait on its `Browser` (or a handle to it) and gives `Arc<dyn SignetApi>` to the UI; the UI never talks
to CEF or the disk.

Settings updates use `SettingsPatch` (a JSON object of changed camelCase fields) validated by
`core::store::clean_settings`, like the TS.

## Paths

`paths::data_dir()` is `<config dir>/Signet GPUI`, `paths::electron_dir()` is `<config dir>/Signet`.
`paths::prepare_data_dir()` resolves the data folder and COPIES the Electron JSON files and `notes/` into it when it
has no `state.json` yet (see `IMPORT_FILES`). The Electron folder is never written.

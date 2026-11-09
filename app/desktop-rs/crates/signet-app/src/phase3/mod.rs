//! PHASE3 stubs: features wired in phase 3. Each file is one feature; every method logs `PHASE3: <name>` (with
//! `SIGNET_DEBUG=1`) and returns a neutral value, so the interface works without it. A phase 3 agent fills in a file
//! without touching the core modules: the signatures are what `browser/api.rs` calls.
//!
//! | file | feature | API methods |
//! | --- | --- | --- |
//! | `passwords` | passwords | list/reveal/copy/save/delete/import/export_passwords, install_password_extension, answer_password_prompt, open_password_key |
//! | `autofill` | autofill | list_autofill, save/delete_address, save/delete_card, reveal_card |
//! | `boosts` | boosts, zap | save_boost, start/stop_zap, unzap, delete_boost, open_boost |
//! | `mods` | UI mods | list/save/delete/move/import/export_mod, set_mod_enabled, set_mods_disabled |
//! | `live_folders` | live folders | save_live_folder, refresh_live_folder, has/set_github_token |
//! | `backup` | backup | get_backup_info, export/open/apply_backup, open_safety_backup, reveal_safety_backups |
//! | `import` | importers | run_import |
//! | `extensions` | Chrome extensions | open_extension_store, remove_extension, load/reload/remove_unpacked_extension, show_extension(s)_menu, get_extension_action_counts/states, ack_extension_action |
//! | `link` | phone link, passkeys | get_link_state, set_link_general/device, forget_link, start/cancel_link_pairing, get/cancel_passkey_prompt |
//! | `webapps` | installed sites | install/list/open/rename/reveal/remove_web_app, web_app_* |
//! | `devtools` | devtools, develop | toggle_dev_tools, inspect_at, set_dev_tools_*, show_dev_tools_menu, show_develop_menu, set_responsive*, show_console |
//! | `page_tools` | site panel, per-site data, menu command router | permissions list, clear_site_data, phase3_command |
//! | `reader` | reader mode, translation | toggle_reader, toggle_translation |
//! | `media` | capture, previews, media, mini player, audio | take_capture, media_*, pip_command, open_pip |
//! | `net` | ad blocker, HTTPS-only, GPC, permission prompts | request hooks |
//! | `web_pages` | favicons, error pages, internal pages, JSON viewer | response hooks, scheme |
//! | `notes` | notes and Easels | notes channels, create_note |
//! | `data` | history copy, bookmarks import/export | |
//! | `windows` | Little, Private, notes | open_little_window, open_private_window, little_*, private_*, create_note |
//! | `system` | updates, default browser, proxy, relaunch | set_default_browser, check_for_updates, relaunch_app, open_proxy_settings, choose_download_dir, clear_browsing_data |
//! | `pages` | page script messages | on_page_message |

//!
//! Each file has a `State` (a field of `Phase3`, reached with `core.phase3.<module>`) and four hooks called for
//! every module: `startup`, `web_event` (every CEF event, before the core handles it), `tick`, `quit`. Global
//! synchronous hooks (request filter, response hook, frame bundle) are registries in `hooks`: a module registers
//! its closures at `startup`; `hooks::install()` (called once in `main.rs`) chains them.

mod autofill;
mod backup;
mod boosts;
pub mod data;
mod devtools;
mod extensions;
mod import;
mod link;
mod live_folders;
mod media;
mod mods;
mod net;
mod notes;
mod page_tools;
mod pages;
mod passwords;
mod reader;
mod system;
mod web_pages;
mod webapps;
mod windows;

pub mod hooks;

pub use extensions::install_ui as install_extension_ui;
pub use media::{fit_ratio, initial_bounds, pip_host, release_target, resized_bounds, PipHostLink, PipInput, PipLink, PipWindowOp};
pub use system::is_default_browser;

use signet_core::types::MediaInfo;

use crate::browser::Core;

/// State owned by phase 3 features: one field per module.
#[derive(Default)]
pub struct Phase3 {
    pub autofill: autofill::State,
    pub backup: backup::State,
    pub boosts: boosts::State,
    pub data: data::State,
    pub devtools: devtools::State,
    pub extensions: extensions::State,
    pub import: import::State,
    pub link: link::State,
    pub live_folders: live_folders::State,
    pub media: media::State,
    pub mods: mods::State,
    pub net: net::State,
    pub notes: notes::State,
    pub page_tools: page_tools::State,
    pub pages: pages::State,
    pub passwords: passwords::State,
    pub reader: reader::State,
    pub system: system::State,
    pub web_pages: web_pages::State,
    pub webapps: webapps::State,
    pub windows: windows::State,
}

impl Phase3 {
    /// Most recent media of a tab that is not displayed (filled by `media`).
    pub fn media_for(&self, displayed: &[String]) -> Option<MediaInfo> {
        media::media_for(&self.media, displayed)
    }

    /// True if the tab must not be put to sleep (mini player, media, devices in use, devtools).
    pub fn keeps_awake(&self, tab_id: &str) -> bool {
        media::keeps_awake(&self.media, tab_id) || self.devtools.is_open(tab_id)
    }
}

impl Core {
    /// Runs every module's `startup` hook. Called once from `main.rs`, after `signet_web::init`.
    pub fn phase3_startup(&mut self) {
        // Each hook is timed by `SIGNET_TRACE_STARTUP` (see `signet_web::startup`).
        macro_rules! timed {
            ($($m:ident),* $(,)?) => {
                $({
                    let _span = signet_web::startup::span(concat!("phase3.", stringify!($m)));
                    $m::startup(self);
                })*
            };
        }
        timed!(
            autofill, backup, boosts, data, devtools, extensions, import, link, live_folders, media, mods, net, notes, page_tools, pages,
            passwords, reader, system, web_pages, webapps, windows,
        );
    }

    /// Runs every module's `web_event` hook, before the core handles the event.
    pub fn phase3_web_event(&mut self, ev: &signet_web::WebEvent) {
        autofill::web_event(self, ev);
        backup::web_event(self, ev);
        boosts::web_event(self, ev);
        data::web_event(self, ev);
        devtools::web_event(self, ev);
        extensions::web_event(self, ev);
        import::web_event(self, ev);
        link::web_event(self, ev);
        live_folders::web_event(self, ev);
        media::web_event(self, ev);
        mods::web_event(self, ev);
        net::web_event(self, ev);
        notes::web_event(self, ev);
        page_tools::web_event(self, ev);
        pages::web_event(self, ev);
        passwords::web_event(self, ev);
        reader::web_event(self, ev);
        system::web_event(self, ev);
        web_pages::web_event(self, ev);
        webapps::web_event(self, ev);
        windows::web_event(self, ev);
    }

    /// Runs every module's `tick` hook.
    pub fn phase3_tick(&mut self) {
        autofill::tick(self);
        backup::tick(self);
        boosts::tick(self);
        data::tick(self);
        devtools::tick(self);
        extensions::tick(self);
        import::tick(self);
        link::tick(self);
        live_folders::tick(self);
        media::tick(self);
        mods::tick(self);
        net::tick(self);
        notes::tick(self);
        page_tools::tick(self);
        pages::tick(self);
        passwords::tick(self);
        reader::tick(self);
        system::tick(self);
        web_pages::tick(self);
        webapps::tick(self);
        windows::tick(self);
    }

    /// Runs every module's `quit` hook.
    pub fn phase3_quit(&mut self) {
        let debug = self.debug;
        let mut step = |name: &str, t: std::time::Instant| {
            if debug {
                println!("[quit] {name}: {} ms", t.elapsed().as_millis());
            }
        };
        let t = std::time::Instant::now();
        autofill::quit(self);
        step("autofill", t);
        let t = std::time::Instant::now();
        backup::quit(self);
        step("backup", t);
        let t = std::time::Instant::now();
        boosts::quit(self);
        step("boosts", t);
        let t = std::time::Instant::now();
        data::quit(self);
        step("data", t);
        let t = std::time::Instant::now();
        devtools::quit(self);
        step("devtools", t);
        let t = std::time::Instant::now();
        extensions::quit(self);
        step("extensions", t);
        let t = std::time::Instant::now();
        import::quit(self);
        step("import", t);
        let t = std::time::Instant::now();
        link::quit(self);
        step("link", t);
        let t = std::time::Instant::now();
        live_folders::quit(self);
        step("live_folders", t);
        let t = std::time::Instant::now();
        media::quit(self);
        step("media", t);
        let t = std::time::Instant::now();
        mods::quit(self);
        step("mods", t);
        let t = std::time::Instant::now();
        net::quit(self);
        step("net", t);
        let t = std::time::Instant::now();
        notes::quit(self);
        step("notes", t);
        let t = std::time::Instant::now();
        page_tools::quit(self);
        step("page_tools", t);
        let t = std::time::Instant::now();
        pages::quit(self);
        step("pages", t);
        let t = std::time::Instant::now();
        passwords::quit(self);
        step("passwords", t);
        let t = std::time::Instant::now();
        reader::quit(self);
        step("reader", t);
        let t = std::time::Instant::now();
        system::quit(self);
        step("system", t);
        let t = std::time::Instant::now();
        web_pages::quit(self);
        step("web_pages", t);
        let t = std::time::Instant::now();
        webapps::quit(self);
        step("webapps", t);
        let t = std::time::Instant::now();
        windows::quit(self);
        step("windows", t);
    }
}

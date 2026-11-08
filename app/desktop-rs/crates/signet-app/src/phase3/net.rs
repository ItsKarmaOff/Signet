//! Network policies: ad blocker (`signet_features::adblock`, port of `web/adblock.ts`), HTTPS-only
//! (`signet_core::core::https_only::HttpsOnly`, port of `browser/https-only.ts`), Global Privacy Control header and
//! the site permission prompts (port of `web/permissions.ts`).
//!
//! Three request hooks run on CEF IO threads and share one `Shared` (atomics for the settings, a mutex for the HTTPS-only
//! state): HTTPS upgrade (priority 50, like `NetworkOrder.httpsOnly`), ad blocker (100) and `Sec-GPC: 1` (300). The
//! cosmetic filters and scriptlets are a bundle part evaluated at the start of every http(s) document. Everything that
//! needs the `Core` (settings sync, warning page, permission prompts) runs from `web_event` and `tick`.
//!
//! Permission prompt: Electron used a native message box. Here it is the native popup menu of the window manager
//! (`Core::popup`): title row, "Allow", "Block", "Not now". Dismissing the menu or "Not now" denies this request only.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use signet_core::core::https_only::{CONTINUE_TITLE, HttpsOnly};
use signet_core::ids::now_ms;
use signet_core::types::PermissionDecision;
use signet_features::adblock::{Adblock, Verdict};
use signet_features::errorpage;
use signet_features::gpc;
use signet_features::http::UreqHttp;
use signet_features::network::order;
use signet_features::permissions as perm;
use signet_web::{Permission, PermissionRequest, RequestDecision, RequestInfo, ResourceType, ViewId, WebEvent};

use super::hooks;
use crate::browser::Core;
use crate::windows::Entry;

/// All regular tabs share one session key for the "allowed through the warning" hosts.
const SESSION: &str = "default";
/// The engine cache is checked for staleness this often.
const REFRESH_CHECK_MS: i64 = 3_600_000;
/// A prompt whose menu was dismissed is forgotten after this delay.
const ASK_STALE: Duration = Duration::from_secs(3);

// ------------------------------------------------------------------------------------------------ shared state

/// Data shared with the CEF threads.
pub struct Shared {
    adblock_on: AtomicBool,
    gpc_on: AtomicBool,
    loading: AtomicBool,
    pub adblock: Adblock,
    https: Mutex<HttpsOnly>,
}

impl Shared {
    fn new(data_dir: &Path) -> Shared {
        Shared {
            adblock_on: AtomicBool::new(false),
            gpc_on: AtomicBool::new(false),
            loading: AtomicBool::new(false),
            adblock: Adblock::new(data_dir),
            https: Mutex::new(HttpsOnly::new()),
        }
    }
}

fn view_key(view: ViewId) -> String {
    format!("v{}", view.0)
}

fn is_http(url: &str) -> bool {
    let l = url.trim_start();
    l.get(..7).is_some_and(|p| p.eq_ignore_ascii_case("http://")) || l.get(..8).is_some_and(|p| p.eq_ignore_ascii_case("https://"))
}

/// Name the ad blocker's request type mapping understands.
fn type_name(t: ResourceType) -> &'static str {
    match t {
        ResourceType::MainFrame => "main_frame",
        ResourceType::SubFrame => "sub_frame",
        ResourceType::Stylesheet => "stylesheet",
        ResourceType::Script => "script",
        ResourceType::Image | ResourceType::Favicon => "image",
        ResourceType::Font => "font",
        ResourceType::Object | ResourceType::PluginResource => "object",
        ResourceType::Media => "media",
        ResourceType::Xhr => "xhr",
        ResourceType::Ping => "ping",
        ResourceType::CspReport => "csp_report",
        _ => "other",
    }
}

// ------------------------------------------------------------------------------------------------ request hooks

/// HTTPS-only: main-frame `http://` requests are redirected to `https://` (frames and subresources are left to Chromium).
fn https_decision(sh: &Shared, r: &RequestInfo, now: i64) -> Option<RequestDecision> {
    if !r.is_main_frame || r.resource_type != ResourceType::MainFrame || !r.url.trim_start().get(..5).is_some_and(|p| p.eq_ignore_ascii_case("http:")) {
        return None;
    }
    let mut g = sh.https.lock().ok()?;
    g.upgrade_request(Some(&view_key(r.view)), SESSION, &r.url, now).map(RequestDecision::Redirect)
}

/// Ad blocker: block or redirect, counting per view. A main-frame request restarts the counter of its view.
fn adblock_decision(sh: &Shared, r: &RequestInfo) -> Option<RequestDecision> {
    if r.is_main_frame && r.resource_type == ResourceType::MainFrame {
        sh.adblock.reset_blocked(r.view.0);
        return None;
    }
    // Pages of the app and extensions are never filtered; neither is anything that is not a web request.
    if !sh.adblock_on.load(Ordering::Relaxed) || !is_http(&r.url) || !is_http(&r.first_party_url) {
        return None;
    }
    match sh.adblock.check_request(&r.url, &r.first_party_url, type_name(r.resource_type), Some(r.view.0)) {
        Verdict::Allow => None,
        Verdict::Block => Some(RequestDecision::Block),
        Verdict::Redirect(body) => Some(RequestDecision::Redirect(body)),
    }
}

/// `Sec-GPC: 1` on web requests while the setting is on.
fn gpc_decision(sh: &Shared, r: &RequestInfo) -> Option<RequestDecision> {
    let (k, v) = gpc::header_for(sh.gpc_on.load(Ordering::Relaxed), &r.url)?;
    Some(RequestDecision::AddHeaders(vec![(k.to_string(), v.to_string())]))
}

/// JS evaluated at the start of a document: scriptlets (each in its own block) then the hiding stylesheet.
fn cosmetic_js(sh: &Shared, url: &str) -> Option<String> {
    if !sh.adblock_on.load(Ordering::Relaxed) || !is_http(url) {
        return None;
    }
    let c = sh.adblock.cosmetic(url);
    if c.css.is_empty() && c.script.is_empty() {
        return None;
    }
    let mut js = String::new();
    if !c.script.is_empty() {
        js.push_str("try {\n");
        js.push_str(&c.script);
        js.push_str("\n} catch (e) {}\n");
    }
    if !c.css.is_empty() {
        let css = serde_json::to_string(&c.css).unwrap_or_else(|_| "\"\"".into());
        js.push_str(&format!(
            "(function (css) {{ try {{ var s = new CSSStyleSheet(); s.replaceSync(css); document.adoptedStyleSheets = document.adoptedStyleSheets.concat([s]); }} catch (e) {{ var add = function () {{ var n = document.createElement('style'); n.textContent = css; (document.head || document.documentElement).appendChild(n); }}; if (document.documentElement) add(); else document.addEventListener('DOMContentLoaded', add); }} }})({css});\n"
        ));
    }
    Some(js)
}

fn spawn_load(sh: &Arc<Shared>) {
    if sh.loading.swap(true, Ordering::SeqCst) {
        return;
    }
    let sh = sh.clone();
    let spawned = std::thread::Builder::new().name("adblock-load".into()).spawn({
        let sh = sh.clone();
        move || {
            if let Err(e) = sh.adblock.load(&UreqHttp) {
                eprintln!("[adblock] engine unavailable: {e}");
            }
            sh.loading.store(false, Ordering::SeqCst);
            // The user may have switched it off while loading.
            if !sh.adblock_on.load(Ordering::SeqCst) {
                sh.adblock.unload();
            }
        }
    });
    if spawned.is_err() {
        sh.loading.store(false, Ordering::SeqCst);
    }
}

// ------------------------------------------------------------------------------------------------ permissions policy

/// How a CEF permission is treated (port of `FREE` / `ASKABLE` of `permissions.ts`).
#[derive(Debug, PartialEq, Eq)]
enum Class {
    Free,
    Ask(&'static str),
    Deny,
}

fn classify(p: Permission) -> Class {
    match p {
        Permission::Camera => Class::Ask("camera"),
        Permission::Microphone => Class::Ask("microphone"),
        Permission::ScreenCapture | Permission::TabCapture => Class::Ask("display-capture"),
        Permission::Geolocation => Class::Ask("geolocation"),
        Permission::Notifications => Class::Ask("notifications"),
        Permission::ClipboardRead => Class::Ask("clipboard-read"),
        Permission::Midi => Class::Ask("midi"),
        // Nothing to ask the user about.
        Permission::MultipleDownloads | Permission::LocalFonts | Permission::ProtectedMedia => Class::Free,
        // Electron refused everything it did not list as askable.
        Permission::MidiSysex | Permission::StorageAccess | Permission::IdleDetection | Permission::WindowManagement | Permission::Other(_) => Class::Deny,
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Plan {
    Allow,
    Deny,
    /// Unknown decision for these permissions: ask the user.
    Ask(Vec<String>),
}

type SitePerms = indexmap::IndexMap<String, indexmap::IndexMap<String, PermissionDecision>>;

/// Decision for a request: any refused part denies, every part known and allowed grants, else ask.
fn plan(table: &SitePerms, origin: &str, perms: &[Permission]) -> Plan {
    let mut names: Vec<String> = Vec::new();
    for p in perms {
        match classify(*p) {
            Class::Deny => return Plan::Deny,
            Class::Free => {}
            Class::Ask(n) => {
                if !names.iter().any(|x| x == n) {
                    names.push(n.to_string());
                }
            }
        }
    }
    if names.is_empty() {
        return Plan::Allow;
    }
    let known: Vec<Option<PermissionDecision>> = names.iter().map(|n| table.get(origin).and_then(|m| m.get(n)).copied()).collect();
    if known.contains(&Some(PermissionDecision::Deny)) {
        Plan::Deny
    } else if known.iter().all(|k| *k == Some(PermissionDecision::Allow)) {
        Plan::Allow
    } else {
        Plan::Ask(names)
    }
}

fn ask_key(origin: &str, names: &[String]) -> String {
    format!("{origin}|{}", names.join("+"))
}

// ------------------------------------------------------------------------------------------------ module state

struct Warning {
    failed_url: String,
    token: String,
    /// Set by `LoadEnd` when the warning was injected: when, and how many times (the tick repeats it a few times).
    injected: Option<(Instant, u8)>,
}

/// Injection attempts of a warning (the first one is made at `LoadEnd`), and the delay between two.
const WARN_TRIES: u8 = 3;
const WARN_EVERY_MS: u128 = 900;

/// Does the HTTPS-only warning of `failed_url` survive a main-frame load start of `load_url`? Only the engine's error
/// page of that failure and a reload of the warned URL keep it; any other page makes it stale.
fn warning_survives_load_start(load_url: &str, failed_url: &str) -> bool {
    load_url.starts_with("chrome-error:") || load_url == failed_url
}

fn inject_warning(core: &Core, view: ViewId, w: &Warning) {
    if let Some(v) = core.views.tab_of(view).and_then(|t| core.views.view_of(t)) {
        let tr = super::web_pages::translate;
        let text = errorpage::insecure_site_text(&tr, &w.failed_url);
        v.execute_js(&errorpage::insecure_site_script(&text, errorpage::ERROR_CSS, &w.token, CONTINUE_TITLE, v.can_go_back()));
    }
}

struct Asking {
    since: Instant,
    waiters: Vec<PermissionRequest>,
}

/// State owned by this feature (a field of `Phase3`).
#[derive(Default)]
pub struct State {
    shared: Option<Arc<Shared>>,
    /// Last `(httpsOnly, alias targets)` pushed to `HttpsOnly`.
    https_cfg: Option<(bool, Vec<String>)>,
    last_refresh_check: i64,
    /// HTTPS-only warnings waiting for their error page to be there, per view.
    warnings: HashMap<ViewId, Warning>,
    /// Prompts shown, with the requests that joined them (one prompt per `origin|permissions`).
    asking: HashMap<String, Asking>,
    /// Requests of tabs that are not displayed yet: asked when the tab comes to the foreground.
    waiting: Vec<(String, PermissionRequest)>,
}

impl State {
    pub(super) fn has_warning(&self, view: ViewId) -> bool {
        self.warnings.contains_key(&view)
    }
}

// ------------------------------------------------------------------------------------------------ hooks

/// Called once after the browser core exists and the web layer is initialized.
pub(super) fn startup(core: &mut Core) {
    let sh = Arc::new(Shared::new(&core.data_dir));
    core.phase3.net.shared = Some(sh.clone());
    sync_settings(core);

    let s = sh.clone();
    hooks::add_request_hook(order::HTTPS_ONLY, move |r| https_decision(&s, r, now_ms()));
    let s = sh.clone();
    hooks::add_request_hook(order::ADBLOCK, move |r| adblock_decision(&s, r));
    let s = sh.clone();
    hooks::add_request_hook(order::HEADERS, move |r| gpc_decision(&s, r));
    let s = sh;
    hooks::add_bundle_part(100, move |f| cosmetic_js(&s, &f.url));
}

/// Copies the settings the CEF threads read, and starts or stops the engine load.
fn sync_settings(core: &mut Core) {
    let Some(sh) = core.phase3.net.shared.clone() else { return };
    let st = &core.store.data.state.settings;
    let (adblock, gpc_on, https_only) = (st.adblock, st.global_privacy_control == Some(true), st.https_only == Some(true));
    let aliases: Vec<String> = st.dev_aliases.as_deref().unwrap_or(&[]).iter().map(|a| a.target.clone()).collect();

    sh.gpc_on.store(gpc_on, Ordering::Relaxed);
    let was = sh.adblock_on.swap(adblock, Ordering::SeqCst);
    if adblock && !sh.adblock.is_loaded() {
        spawn_load(&sh);
    } else if !adblock && was && !sh.loading.load(Ordering::SeqCst) {
        sh.adblock.unload();
    }
    let cfg = (https_only, aliases);
    if core.phase3.net.https_cfg.as_ref() != Some(&cfg)
        && let Ok(mut g) = sh.https.lock()
    {
        g.set_config(cfg.0, cfg.1.clone());
        core.phase3.net.https_cfg = Some(cfg);
    }
}

/// Called for every CEF event, before the core handles it.
pub(super) fn web_event(core: &mut Core, ev: &WebEvent) {
    let Some(sh) = core.phase3.net.shared.clone() else { return };
    sync_settings(core);
    let view = ev.view();
    let tab = core.views.tab_of(view).map(str::to_string);
    let key = view_key(view);
    match ev {
        WebEvent::LoadStart { is_main_frame: true, url, .. } => {
            // Another page starts loading: the warning (its host and Continue token) no longer applies to the view.
            if core.phase3.net.warnings.get(&view).is_some_and(|w| !warning_survives_load_start(url, &w.failed_url)) {
                core.phase3.net.warnings.remove(&view);
            }
            // Typed address promoted to https://: its http:// version is the fallback, and a failure shows the warning.
            if let Some(pending) = tab.as_ref().and_then(|t| core.upgraded.get(t)).cloned()
                && let Ok(mut g) = sh.https.lock()
            {
                g.note_typed(&key, &pending, now_ms());
            }
        }
        WebEvent::AddressChanged { is_main_frame: true, url, .. } => {
            if let Ok(mut g) = sh.https.lock() {
                g.on_committed(&key, url);
            }
        }
        WebEvent::LoadError { is_main_frame: true, code, url, .. } if *code != -3 => {
            let http = sh.https.lock().ok().and_then(|g| g.insecure_failure(&key, url, *code));
            if let (Some(http), Some(tab)) = (http, tab.as_ref()) {
                // No silent http fallback of the typed input: the warning replaces the error page.
                core.upgraded.remove(tab);
                let token = sh.https.lock().map(|mut g| g.warn(&key, &http)).unwrap_or_default();
                core.phase3.net.warnings.insert(view, Warning { failed_url: url.clone(), token, injected: None });
            }
        }
        WebEvent::LoadEnd { is_main_frame: true, url, .. } => {
            if let Some(w) = core.phase3.net.warnings.get(&view) {
                // The warning replaces the engine's error page; its script checks it is in `chrome-error:`.
                if warning_survives_load_start(url, &w.failed_url) {
                    inject_warning(core, view, w);
                    if let Some(w) = core.phase3.net.warnings.get_mut(&view) {
                        w.injected = Some((Instant::now(), 1));
                    }
                } else {
                    core.phase3.net.warnings.remove(&view);
                }
            }
        }
        WebEvent::TitleChanged { title, .. } if title.starts_with(CONTINUE_TITLE) => {
            // "Continue to site" on the warning page: the token proves the click comes from that page.
            let http = sh.https.lock().ok().and_then(|mut g| g.continue_from(&key, SESSION, title));
            if let (Some(http), Some(tab)) = (http, tab) {
                core.phase3.net.warnings.remove(&view);
                core.load_url(&tab, &http, false);
            }
        }
        WebEvent::Closed { .. } => {
            if let Ok(mut g) = sh.https.lock() {
                g.forget(&key);
            }
            sh.adblock.reset_blocked(view.0);
            core.phase3.net.warnings.remove(&view);
        }
        _ => {}
    }
}

/// Called on every core tick (about once per second or at the next store deadline).
pub(super) fn tick(core: &mut Core) {
    sync_settings(core);
    let Some(sh) = core.phase3.net.shared.clone() else { return };
    let now = now_ms();
    // Lists are refreshed when the cache is older than a week (the old cache is kept when offline).
    if now - core.phase3.net.last_refresh_check > REFRESH_CHECK_MS {
        core.phase3.net.last_refresh_check = now;
        if sh.adblock_on.load(Ordering::Relaxed) && sh.adblock.is_loaded() && sh.adblock.cache_is_stale() {
            spawn_load(&sh);
        }
    }
    // Prompts whose menu was dismissed leave no trace but a stale entry: their requests are denied by dropping them.
    core.phase3.net.asking.retain(|_, a| a.since.elapsed() < ASK_STALE);
    // The warning script is idempotent: repeat it a few times in case the engine's page was not ready at `LoadEnd`.
    let due: Vec<ViewId> = core.phase3.net.warnings.iter().filter(|(_, w)| w.injected.is_some_and(|(at, n)| n < WARN_TRIES && at.elapsed().as_millis() >= WARN_EVERY_MS)).map(|(v, _)| *v).collect();
    for view in due {
        if let Some(w) = core.phase3.net.warnings.get(&view) {
            inject_warning(core, view, w);
        }
        if let Some(w) = core.phase3.net.warnings.get_mut(&view) {
            w.injected = w.injected.map(|(_, n)| (Instant::now(), n + 1));
        }
    }
    let waiting = std::mem::take(&mut core.phase3.net.waiting);
    for (tab, req) in waiting {
        if !core.state().tabs.contains_key(&tab) {
            continue; // closed: denied without remembering
        }
        if core.shown_tabs.contains(&tab) {
            core.on_permission_request(&tab, req);
        } else {
            core.phase3.net.waiting.push((tab, req));
        }
    }
}

/// Called before quitting (flush files, stop threads).
pub(super) fn quit(core: &mut Core) {
    core.phase3.net.waiting.clear();
    core.phase3.net.asking.clear();
}

// ------------------------------------------------------------------------------------------------ Core API

impl Core {
    /// Requests blocked by the ad blocker on the page of this tab (site panel `SiteInfo.blocked`, toolbar badge).
    pub fn adblock_blocked(&self, tab_id: &str) -> u32 {
        let (Some(sh), Some(v)) = (self.phase3.net.shared.as_ref(), self.views.view_of(tab_id)) else { return 0 };
        sh.adblock.blocked_count(v.id().0)
    }

    /// Turns the ad blocker off or on for a host and its subdomains (in memory: there is no persisted allowlist).
    pub fn adblock_allow_host(&mut self, host: &str, allowed: bool) {
        if let Some(sh) = self.phase3.net.shared.as_ref() {
            sh.adblock.set_allowed(host, allowed);
        }
    }

    /// True if the ad blocker is off for this page.
    pub fn adblock_is_allowed(&self, url: &str) -> bool {
        self.phase3.net.shared.as_ref().is_some_and(|sh| sh.adblock.is_allowed(url))
    }

    /// Permission request of a page (arm `WebEvent::Permission` of `browser/events.rs`).
    pub fn on_permission_request(&mut self, tab: &str, req: PermissionRequest) {
        let top = self.state().tabs.get(tab).map(|t| t.url.clone());
        let Some(origin) = perm::origin_for(&[&req.origin], top.as_deref()) else {
            self.log(format!("permission {} -> deny (no web origin)", req.origin));
            return req.deny();
        };
        match plan(&self.state().site_permissions, &origin, &req.permissions) {
            Plan::Allow => {
                self.log(format!("permission {origin} {:?} -> allow", req.permissions));
                req.allow();
            }
            Plan::Deny => {
                self.log(format!("permission {origin} {:?} -> deny", req.permissions));
                req.deny();
            }
            Plan::Ask(names) => {
                // A background tab asks when it is displayed.
                if !self.shown_tabs.iter().any(|t| t == tab) {
                    self.phase3.net.waiting.push((tab.to_string(), req));
                    return;
                }
                let key = ask_key(&origin, &names);
                if let Some(a) = self.phase3.net.asking.get_mut(&key) {
                    a.waiters.push(req); // same question already on screen
                    return;
                }
                self.phase3.net.asking.insert(key.clone(), Asking { since: Instant::now(), waiters: vec![req] });
                self.show_permission_prompt(key, origin, names);
            }
        }
    }

    fn show_permission_prompt(&mut self, key: String, origin: String, names: Vec<String>) {
        let t = signet_core::i18n::t;
        let shown_origin = origin.split_once("://").map_or(origin.as_str(), |(_, rest)| rest).to_string();
        let message = signet_core::i18n::t_args(&format!("dialog.permission.{}", perm::Permissions::dialog_key(&names)), &[("origin", &shown_origin)]);
        let item = |label: String, id: &str| Entry::Item { label, id: id.to_string(), enabled: true, checked: false };
        let entries = vec![
            Entry::Item { label: message, id: "title".into(), enabled: false, checked: false },
            Entry::Item { label: t("dialog.permission.detail"), id: "detail".into(), enabled: false, checked: false },
            Entry::Sep,
            item(t("dialog.permission.allow"), "allow"),
            item(t("dialog.permission.deny"), "deny"),
            item(t("dialog.permission.later"), "later"),
        ];
        let at = self.win.as_ref().map_or((200.0, 80.0), |w| ((w.size.0 / 2.0 - 140.0).max(20.0), 80.0));
        self.popup(entries, at, move |core, choice| {
            let waiters = core.phase3.net.asking.remove(&key).map(|a| a.waiters).unwrap_or_default();
            // "Not now" denies this request only: nothing is remembered.
            let decision = match choice {
                "allow" => Some(PermissionDecision::Allow),
                "deny" => Some(PermissionDecision::Deny),
                _ => None,
            };
            if let Some(d) = decision {
                let table = core.store.data.state.site_permissions.entry(origin.clone()).or_default();
                for n in &names {
                    table.insert(n.clone(), d);
                }
                core.changed();
            }
            core.log(format!("permission {origin} {names:?} -> {choice}"));
            for w in waiters {
                if decision == Some(PermissionDecision::Allow) { w.allow() } else { w.deny() }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use indexmap::IndexMap;

    fn req(url: &str, first: &str, t: ResourceType, main: bool) -> RequestInfo {
        RequestInfo { view: ViewId(7), url: url.into(), method: "GET".into(), resource_type: t, first_party_url: first.into(), is_main_frame: main, initiator: String::new() }
    }

    fn shared() -> ((), Shared) {
        // Nothing is written: the engine is built from text.
        let s = Shared::new(&std::env::temp_dir().join("signet-net-tests"));
        s.adblock.load_from_text(&["||ads.example.com^\nexample.org##.banner".to_string()]);
        ((), s)
    }

    #[test]
    fn warning_only_survives_its_own_error_page_and_reload() {
        assert!(warning_survives_load_start("chrome-error://chromewebdata/", "http://a.test/"));
        assert!(warning_survives_load_start("http://a.test/", "http://a.test/"));
        assert!(!warning_survives_load_start("https://b.test/", "http://a.test/"));
    }

    #[test]
    fn adblock_blocks_counts_and_resets() {
        let (_d, s) = shared();
        s.adblock_on.store(true, Ordering::Relaxed);
        let ad = req("https://ads.example.com/a.js", "https://site.com/", ResourceType::Script, false);
        assert_eq!(adblock_decision(&s, &ad), Some(RequestDecision::Block));
        assert_eq!(adblock_decision(&s, &req("https://good.com/a.js", "https://site.com/", ResourceType::Script, false)), None);
        assert_eq!(s.adblock.blocked_count(7), 1);
        // A main-frame request restarts the counter and is never blocked.
        assert_eq!(adblock_decision(&s, &req("https://ads.example.com/", "https://ads.example.com/", ResourceType::MainFrame, true)), None);
        assert_eq!(s.adblock.blocked_count(7), 0);
    }

    #[test]
    fn adblock_off_allowlist_and_app_pages() {
        let (_d, s) = shared();
        let ad = req("https://ads.example.com/a.js", "https://site.com/", ResourceType::Script, false);
        assert_eq!(adblock_decision(&s, &ad), None, "setting off");
        s.adblock_on.store(true, Ordering::Relaxed);
        s.adblock.set_allowed("site.com", true);
        assert_eq!(adblock_decision(&s, &ad), None, "allowlisted site");
        s.adblock.set_allowed("site.com", false);
        let from_app = req("https://ads.example.com/a.js", "signet://pages/x", ResourceType::Script, false);
        assert_eq!(adblock_decision(&s, &from_app), None, "app page");
        assert_eq!(adblock_decision(&s, &ad), Some(RequestDecision::Block));
    }

    #[test]
    fn gpc_header_follows_the_setting() {
        let (_d, s) = shared();
        let r = req("https://a.com/", "https://a.com/", ResourceType::Xhr, false);
        assert_eq!(gpc_decision(&s, &r), None);
        s.gpc_on.store(true, Ordering::Relaxed);
        assert_eq!(gpc_decision(&s, &r), Some(RequestDecision::AddHeaders(vec![("Sec-GPC".into(), "1".into())])));
        assert_eq!(gpc_decision(&s, &req("file:///a", "file:///a", ResourceType::Xhr, false)), None);
    }

    #[test]
    fn https_upgrade_loop_and_subresources() {
        let (_d, s) = shared();
        let main = req("http://example.com/x", "http://example.com/x", ResourceType::MainFrame, true);
        assert_eq!(https_decision(&s, &main, 0), None, "mode off");
        s.https.lock().unwrap().set_config(true, vec![]);
        assert_eq!(https_decision(&s, &main, 0), Some(RequestDecision::Redirect("https://example.com/x".into())));
        assert_eq!(https_decision(&s, &req("http://example.com/i.png", "http://example.com/", ResourceType::Image, false), 0), None, "subresource");
        assert_eq!(https_decision(&s, &req("http://localhost:3000/", "http://localhost:3000/", ResourceType::MainFrame, true), 0), None, "exempt host");
        // The site sends back to http each time: after 3 upgrades within 10 s the page loads over http.
        assert!(https_decision(&s, &main, 1).is_some());
        assert!(https_decision(&s, &main, 2).is_some());
        assert_eq!(https_decision(&s, &main, 3), None);
    }

    #[test]
    fn https_warning_and_continue_token() {
        let (_d, s) = shared();
        let mut g = s.https.lock().unwrap();
        g.set_config(true, vec![]);
        g.note_typed("v7", "https://example.com/", 0);
        assert_eq!(g.insecure_failure("v7", "https://example.com/", -200).as_deref(), Some("http://example.com/"));
        assert_eq!(g.insecure_failure("v7", "https://example.com/", -105), None, "DNS failure keeps the normal error page");
        let token = g.warn("v7", "http://example.com/");
        assert_eq!(g.continue_from("v7", SESSION, &format!("{CONTINUE_TITLE}wrong")), None);
        assert_eq!(g.continue_from("v7", SESSION, &format!("{CONTINUE_TITLE}{token}")).as_deref(), Some("http://example.com/"));
        // The host is now allowed for the session.
        assert_eq!(g.resolve_for("v7", SESSION, "http://example.com/", 5), None);
    }

    #[test]
    fn cosmetic_injection() {
        let (_d, s) = shared();
        assert_eq!(cosmetic_js(&s, "https://example.org/"), None, "setting off");
        s.adblock_on.store(true, Ordering::Relaxed);
        let js = cosmetic_js(&s, "https://example.org/").unwrap();
        assert!(js.contains("CSSStyleSheet") && js.contains(".banner"));
        assert_eq!(cosmetic_js(&s, "https://other.org/"), None);
        assert_eq!(cosmetic_js(&s, "signet://urls"), None);
    }

    fn table(entries: &[(&str, &str, PermissionDecision)]) -> SitePerms {
        let mut t: SitePerms = IndexMap::new();
        for (o, p, d) in entries {
            t.entry(o.to_string()).or_default().insert(p.to_string(), *d);
        }
        t
    }

    #[test]
    fn permission_plan() {
        let o = "https://meet.example.com";
        let none = table(&[]);
        assert_eq!(plan(&none, o, &[Permission::Geolocation]), Plan::Ask(vec!["geolocation".into()]));
        assert_eq!(plan(&none, o, &[Permission::Camera, Permission::Microphone]), Plan::Ask(vec!["camera".into(), "microphone".into()]));
        assert_eq!(plan(&none, o, &[Permission::MultipleDownloads]), Plan::Allow);
        assert_eq!(plan(&none, o, &[Permission::StorageAccess]), Plan::Deny);
        assert_eq!(plan(&none, o, &[Permission::Camera, Permission::IdleDetection]), Plan::Deny);
        let cam = table(&[(o, "camera", PermissionDecision::Allow)]);
        assert_eq!(plan(&cam, o, &[Permission::Camera]), Plan::Allow);
        assert_eq!(plan(&cam, o, &[Permission::Camera, Permission::Microphone]), Plan::Ask(vec!["camera".into(), "microphone".into()]));
        let denied = table(&[(o, "camera", PermissionDecision::Allow), (o, "microphone", PermissionDecision::Deny)]);
        assert_eq!(plan(&denied, o, &[Permission::Camera, Permission::Microphone]), Plan::Deny);
        assert_eq!(plan(&cam, "https://other.com", &[Permission::Camera]), Plan::Ask(vec!["camera".into()]));
    }

    #[test]
    fn ask_key_groups_the_same_question() {
        assert_eq!(ask_key("https://a.com", &["camera".into(), "microphone".into()]), "https://a.com|camera+microphone");
        assert_eq!(classify(Permission::TabCapture), Class::Ask("display-capture"));
    }
}

//! Ad and tracker blocker (port of the logic of `main/web/adblock.ts`) on Brave's `adblock` engine.
//!
//! The TS app uses Ghostery's prebuilt ads and tracking lists. Here the same family of lists is fetched
//! (EasyList, EasyPrivacy, uBlock Origin filters) and compiled into one engine, serialized on disk and refreshed
//! after 7 days (the old cache is kept when offline).
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use std::time::{Duration, SystemTime};

use adblock::lists::{FilterSet, ParseOptions};
use adblock::request::Request;
use adblock::resources::Resource;
use adblock::Engine;

use crate::http::Http;
use crate::util::{err, write_atomic, Result};

pub const MAX_AGE: Duration = Duration::from_secs(7 * 24 * 3600);
pub const CACHE_FILE: &str = "adblock.dat";

/// Ads and tracking lists compiled into the engine: exactly `adsAndTrackingLists` of `@ghostery/adblocker` 2.20.3
/// (`fromPrebuiltAdsAndTracking`, what `main/web/adblock.ts` uses), from Ghostery's own copies of the upstream lists.
pub const LIST_URLS: &[&str] = &[
    "https://raw.githubusercontent.com/ghostery/adblocker/master/packages/adblocker/assets/easylist/easylist.txt",
    "https://raw.githubusercontent.com/ghostery/adblocker/master/packages/adblocker/assets/peter-lowe/serverlist.txt",
    "https://raw.githubusercontent.com/ghostery/adblocker/master/packages/adblocker/assets/ublock-origin/badware.txt",
    "https://raw.githubusercontent.com/ghostery/adblocker/master/packages/adblocker/assets/ublock-origin/filters-2020.txt",
    "https://raw.githubusercontent.com/ghostery/adblocker/master/packages/adblocker/assets/ublock-origin/filters-2021.txt",
    "https://raw.githubusercontent.com/ghostery/adblocker/master/packages/adblocker/assets/ublock-origin/filters-2022.txt",
    "https://raw.githubusercontent.com/ghostery/adblocker/master/packages/adblocker/assets/ublock-origin/filters-2023.txt",
    "https://raw.githubusercontent.com/ghostery/adblocker/master/packages/adblocker/assets/ublock-origin/filters-2024.txt",
    "https://raw.githubusercontent.com/ghostery/adblocker/master/packages/adblocker/assets/ublock-origin/filters.txt",
    "https://raw.githubusercontent.com/ghostery/adblocker/master/packages/adblocker/assets/ublock-origin/quick-fixes.txt",
    "https://raw.githubusercontent.com/ghostery/adblocker/master/packages/adblocker/assets/ublock-origin/resource-abuse.txt",
    "https://raw.githubusercontent.com/ghostery/adblocker/master/packages/adblocker/assets/ublock-origin/unbreak.txt",
    "https://raw.githubusercontent.com/ghostery/adblocker/master/packages/adblocker/assets/easylist/easyprivacy.txt",
    "https://raw.githubusercontent.com/ghostery/adblocker/master/packages/adblocker/assets/ublock-origin/privacy.txt",
];
/// Redirect and scriptlet resources (JSON list of `Resource`).
pub const RESOURCES_URL: &str = "https://raw.githubusercontent.com/brave/adblock-resources/master/dist/resources.json";

#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    Allow,
    Block,
    /// Serve this body instead (data URL content already built by the library).
    Redirect(String),
}

/// Electron/CEF resource type to the library's request type.
pub fn map_resource_type(t: &str) -> &'static str {
    match t {
        "mainFrame" | "main_frame" | "document" => "document",
        "subFrame" | "sub_frame" => "sub_frame",
        "stylesheet" => "stylesheet",
        "script" => "script",
        "image" => "image",
        "font" => "font",
        "object" => "object",
        "xhr" | "xmlhttprequest" | "fetch" => "xmlhttprequest",
        "ping" => "ping",
        "cspReport" | "csp_report" => "csp_report",
        "media" => "media",
        "webSocket" | "websocket" => "websocket",
        _ => "other",
    }
}

fn host_of(url: &str) -> Option<String> {
    url::Url::parse(url).ok().and_then(|u| u.host_str().map(|h| h.to_lowercase()))
}

/// Cosmetic result for a page URL.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Cosmetic {
    /// CSS to inject: `sel, sel { display: none !important }`.
    pub css: String,
    /// Scriptlets JS to run at document start.
    pub script: String,
    pub generichide: bool,
}

pub struct Adblock {
    engine: RwLock<Option<Engine>>,
    cache: PathBuf,
    allow: RwLock<HashSet<String>>,
    blocked: RwLock<HashMap<u64, u32>>,
}

impl Adblock {
    pub fn new(data_dir: &Path) -> Self {
        Adblock { engine: RwLock::new(None), cache: data_dir.join(CACHE_FILE), allow: RwLock::new(HashSet::new()), blocked: RwLock::new(HashMap::new()) }
    }

    pub fn is_loaded(&self) -> bool {
        self.engine.read().map(|e| e.is_some()).unwrap_or(false)
    }

    /// Turns the blocker off (drops the engine).
    pub fn unload(&self) {
        if let Ok(mut e) = self.engine.write() {
            *e = None;
        }
    }

    /// Installs an engine built from filter text (used by tests and by `load`).
    pub fn load_from_text(&self, lists: &[String]) {
        let mut set = FilterSet::new(false);
        for l in lists {
            set.add_filter_list(l.clone(), ParseOptions::default());
        }
        if let Ok(mut e) = self.engine.write() {
            *e = Some(Engine::new_with_filter_set(set));
        }
    }

    pub fn cache_is_stale(&self) -> bool {
        std::fs::metadata(&self.cache)
            .and_then(|m| m.modified())
            .map(|t| SystemTime::now().duration_since(t).map(|d| d > MAX_AGE).unwrap_or(false))
            .unwrap_or(false)
    }

    fn load_cache(&self) -> Result<()> {
        let bytes = std::fs::read(&self.cache)?;
        let mut engine = Engine::default();
        engine.deserialize(&bytes).map_err(|e| crate::util::Error::Msg(format!("adblock cache: {e:?}")))?;
        if let Ok(mut e) = self.engine.write() {
            *e = Some(engine);
        }
        Ok(())
    }

    fn fetch_and_build(&self, http: &dyn Http) -> Result<()> {
        let mut set = FilterSet::new(false);
        let mut got = 0;
        for url in LIST_URLS {
            if let Ok(r) = http.get(url, &[], Duration::from_secs(30), 30 * 1024 * 1024)
                && r.ok() {
                    set.add_filter_list(r.text(), ParseOptions::default());
                    got += 1;
                }
        }
        if got == 0 {
            return err("no filter list available");
        }
        let mut engine = Engine::new_with_filter_set(set);
        if let Ok(r) = http.get(RESOURCES_URL, &[], Duration::from_secs(30), 10 * 1024 * 1024)
            && r.ok()
                && let Ok(res) = serde_json::from_slice::<Vec<Resource>>(&r.body) {
                    engine.use_resources(res);
                }
        let bytes = engine.serialize();
        if let Ok(mut e) = self.engine.write() {
            *e = Some(engine);
        }
        write_atomic(&self.cache, &bytes)
    }

    /// Loads the engine (blocking, run off the main thread): fresh cache, else network, else the old cache.
    pub fn load(&self, http: &dyn Http) -> Result<()> {
        if self.cache.exists() && !self.cache_is_stale() && self.load_cache().is_ok() {
            return Ok(());
        }
        match self.fetch_and_build(http) {
            Ok(()) => Ok(()),
            Err(e) => {
                if self.cache.exists() && self.load_cache().is_ok() { Ok(()) } else { Err(e) }
            }
        }
    }

    // ----------------------------------------------------------- allowlist (hosts where blocking is off)

    pub fn set_allowed(&self, host: &str, allowed: bool) {
        if let Ok(mut a) = self.allow.write() {
            let h = host.trim().to_lowercase();
            if allowed { a.insert(h); } else { a.remove(&h); }
        }
    }

    pub fn allowed_hosts(&self) -> Vec<String> {
        let mut v: Vec<String> = self.allow.read().map(|a| a.iter().cloned().collect()).unwrap_or_default();
        v.sort();
        v
    }

    pub fn set_allowlist(&self, hosts: &[String]) {
        if let Ok(mut a) = self.allow.write() {
            *a = hosts.iter().map(|h| h.trim().to_lowercase()).filter(|h| !h.is_empty()).collect();
        }
    }

    /// True if the page's host (or a parent domain) is on the allowlist.
    pub fn is_allowed(&self, page_url: &str) -> bool {
        let Some(mut h) = host_of(page_url) else { return false };
        let Ok(a) = self.allow.read() else { return false };
        loop {
            if a.contains(&h) {
                return true;
            }
            match h.split_once('.') {
                Some((_, rest)) if rest.contains('.') => h = rest.to_string(),
                _ => return false,
            }
        }
    }

    // ----------------------------------------------------------- requests

    /// Decision for a request. `tab` counts blocked requests per tab (pass `None` to not count).
    pub fn check_request(&self, url: &str, source_url: &str, resource_type: &str, tab: Option<u64>) -> Verdict {
        let Ok(guard) = self.engine.read() else { return Verdict::Allow };
        let Some(engine) = guard.as_ref() else { return Verdict::Allow };
        if !source_url.is_empty() && self.is_allowed(source_url) {
            return Verdict::Allow;
        }
        let Ok(req) = Request::new(url, source_url, map_resource_type(resource_type), "GET") else { return Verdict::Allow };
        let r = engine.check_network_request(&req);
        let verdict = if r.exception.is_some() && !r.important {
            Verdict::Allow
        } else if r.filter.is_some() {
            match r.redirect {
                Some(body) => Verdict::Redirect(body),
                None => Verdict::Block,
            }
        } else {
            Verdict::Allow
        };
        if verdict != Verdict::Allow
            && let (Some(t), Ok(mut b)) = (tab, self.blocked.write()) {
                *b.entry(t).or_insert(0) += 1;
            }
        verdict
    }

    /// Cosmetic filters for a page: CSS to inject and scriptlets. Empty while off or allowlisted.
    pub fn cosmetic(&self, page_url: &str) -> Cosmetic {
        if self.is_allowed(page_url) {
            return Cosmetic::default();
        }
        let Ok(guard) = self.engine.read() else { return Cosmetic::default() };
        let Some(engine) = guard.as_ref() else { return Cosmetic::default() };
        let r = engine.url_cosmetic_resources(page_url);
        let mut sels: Vec<&String> = r.hide_selectors.iter().collect();
        sels.sort();
        let css = sels.chunks(500).map(|c| format!("{} {{ display: none !important }}", c.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "))).collect::<Vec<_>>().join("\n");
        // Each scriptlet runs in its own block, like the TS `scopedInjection`, so `class`/`const` stay local.
        let script = if r.injected_script.is_empty() { String::new() } else { format!("{{\n{}\n}}", r.injected_script) };
        Cosmetic { css, script, generichide: r.generichide }
    }

    /// Class and id based generic selectors for classes/ids seen in the page.
    pub fn generic_hide_css(&self, classes: &[String], ids: &[String], exceptions: &HashSet<String>) -> String {
        let Ok(guard) = self.engine.read() else { return String::new() };
        let Some(engine) = guard.as_ref() else { return String::new() };
        let mut sels = engine.hidden_class_id_selectors(classes, ids, exceptions);
        sels.sort();
        if sels.is_empty() { String::new() } else { format!("{} {{ display: none !important }}", sels.join(", ")) }
    }

    // ----------------------------------------------------------- counters

    pub fn blocked_count(&self, tab: u64) -> u32 {
        self.blocked.read().ok().and_then(|b| b.get(&tab).copied()).unwrap_or(0)
    }

    /// To call when the main frame navigates (or the view is destroyed).
    pub fn reset_blocked(&self, tab: u64) {
        if let Ok(mut b) = self.blocked.write() {
            b.remove(&tab);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{resp, FakeHttp};

    fn blocker() -> (tempfile::TempDir, Adblock) {
        let d = tempfile::tempdir().unwrap();
        let a = Adblock::new(d.path());
        a.load_from_text(&["||ads.example.com^\n@@||ads.example.com/ok^\nexample.org##.banner\n||track.net^$script".to_string()]);
        (d, a)
    }

    #[test]
    fn blocks_and_allows() {
        let (_d, a) = blocker();
        assert_eq!(a.check_request("https://ads.example.com/x.js", "https://site.com/", "script", Some(1)), Verdict::Block);
        assert_eq!(a.check_request("https://ads.example.com/ok", "https://site.com/", "script", Some(1)), Verdict::Allow);
        assert_eq!(a.check_request("https://good.com/x.js", "https://site.com/", "script", Some(1)), Verdict::Allow);
        assert_eq!(a.check_request("https://track.net/a", "https://site.com/", "image", Some(1)), Verdict::Allow);
        assert_eq!(a.check_request("https://track.net/a", "https://site.com/", "script", Some(1)), Verdict::Block);
        assert_eq!(a.blocked_count(1), 2);
        a.reset_blocked(1);
        assert_eq!(a.blocked_count(1), 0);
    }

    #[test]
    fn allowlist_per_site() {
        let (_d, a) = blocker();
        a.set_allowed("site.com", true);
        assert_eq!(a.check_request("https://ads.example.com/x.js", "https://www.site.com/", "script", None), Verdict::Allow);
        assert!(a.cosmetic("https://example.org/").css.contains(".banner"));
        a.set_allowed("example.org", true);
        assert!(a.cosmetic("https://example.org/").css.is_empty());
        a.set_allowed("site.com", false);
        assert_eq!(a.check_request("https://ads.example.com/x.js", "https://www.site.com/", "script", None), Verdict::Block);
    }

    #[test]
    fn cosmetic_css() {
        let (_d, a) = blocker();
        let c = a.cosmetic("https://example.org/page");
        assert!(c.css.contains(".banner") && c.css.contains("display: none !important"));
        assert!(a.cosmetic("https://other.org/").css.is_empty());
    }

    #[test]
    fn off_when_unloaded() {
        let d = tempfile::tempdir().unwrap();
        let a = Adblock::new(d.path());
        assert_eq!(a.check_request("https://ads.example.com/x.js", "https://s.com/", "script", None), Verdict::Allow);
        assert!(!a.is_loaded());
    }

    #[test]
    fn cache_roundtrip_and_offline_fallback() {
        let d = tempfile::tempdir().unwrap();
        let a = Adblock::new(d.path());
        let http = FakeHttp(Box::new(|u| if u.ends_with("/easylist/easylist.txt") { resp(200, "||ads.example.com^") } else { resp(404, "") }));
        a.load(&http).unwrap();
        assert!(d.path().join(CACHE_FILE).exists());
        assert_eq!(a.check_request("https://ads.example.com/a", "https://s.com/", "script", None), Verdict::Block);
        // A second instance with no network reads the cache.
        let b = Adblock::new(d.path());
        let offline = FakeHttp(Box::new(|_| Err("offline".into())));
        b.load(&offline).unwrap();
        assert_eq!(b.check_request("https://ads.example.com/a", "https://s.com/", "script", None), Verdict::Block);
        // No cache, no network: error.
        let d2 = tempfile::tempdir().unwrap();
        assert!(Adblock::new(d2.path()).load(&offline).is_err());
    }

    #[test]
    fn type_mapping() {
        assert_eq!(map_resource_type("mainFrame"), "document");
        assert_eq!(map_resource_type("xhr"), "xmlhttprequest");
        assert_eq!(map_resource_type("whatever"), "other");
    }
}

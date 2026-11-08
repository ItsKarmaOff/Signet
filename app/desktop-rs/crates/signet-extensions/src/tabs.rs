//! `chrome.tabs` members the host implements, and the Chrome tab object built from Signet tabs. Port of `apis/tabs.ts`.
//!
//! The browser layer implements [`TabHost`]; this module holds the rules (argument parsing, permission checks, CSS
//! bookkeeping, language detection result mapping). [`TabInfo::to_chrome_tab`] builds `tabs.Tab`.
//!
//! IMPORTANT: [`TabHost`] methods may block (screenshot, script evaluation round trips). Call
//! `ExtensionHost::call` from a worker thread, never from the UI thread.

use crate::common::{ApiError, ApiResult};
use crate::urlmatch::{can_access_url, Access};
use serde_json::{json, Value};
use std::collections::HashMap;

pub const DEFAULT_QUALITY: i64 = 92;

/// Signet tab as an extension sees it. Tab and window ids are the engine's integer browser ids.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TabInfo {
    pub id: i64,
    pub window_id: i64,
    /// Position in the bar's order.
    pub index: i64,
    pub active: bool,
    pub pinned: bool,
    pub highlighted: bool,
    pub incognito: bool,
    pub url: String,
    pub title: String,
    pub fav_icon_url: Option<String>,
    pub loading: bool,
    pub audible: bool,
    pub muted: bool,
    pub discarded: bool,
    pub width: i64,
    pub height: i64,
    /// Chrome tab group id (-1 = none); Signet folders are not tab groups.
    pub group_id: i64,
    pub opener_tab_id: Option<i64>,
}

impl TabInfo {
    /// The tab in Chrome's `tabs.Tab` shape. `url`, `title` and `favIconUrl` need the `tabs` permission (or a host
    /// permission for the tab's URL): pass `sensitive = false` to leave them out.
    pub fn to_chrome_tab(&self, sensitive: bool) -> Value {
        let mut t = json!({
            "id": self.id, "index": self.index, "windowId": self.window_id, "groupId": self.group_id,
            "active": self.active, "pinned": self.pinned, "highlighted": self.highlighted || self.active,
            "incognito": self.incognito, "audible": self.audible, "discarded": self.discarded,
            "autoDiscardable": true, "selected": self.active, "width": self.width, "height": self.height,
            "status": if self.loading { "loading" } else { "complete" },
            "mutedInfo": if self.muted { json!({ "muted": true, "reason": "user" }) } else { json!({ "muted": false }) },
        });
        if sensitive {
            t["url"] = json!(self.url);
            t["title"] = json!(self.title);
            if let Some(f) = &self.fav_icon_url {
                t["favIconUrl"] = json!(f);
            }
        }
        if let Some(o) = self.opener_tab_id {
            t["openerTabId"] = json!(o);
        }
        t
    }
}

/// What the browser layer answers for the tabs APIs.
pub trait TabHost: Send + Sync {
    fn tab(&self, tab_id: i64) -> Option<TabInfo>;
    /// Active tab of a window (the current one when `window_id` is None).
    fn active_tab(&self, window_id: Option<i64>) -> Option<TabInfo>;
    /// Screenshot of the displayed tab as a data URL (`jpeg` with quality 0-100, or `png`).
    fn capture(&self, tab_id: i64, format: &str, quality: i64) -> Result<String, String>;
    /// Injects a stylesheet; returns a key to remove it later.
    fn insert_css(&self, tab_id: i64, css: &str, origin: &str) -> Result<String, String>;
    fn remove_css(&self, tab_id: i64, key: &str);
    /// Result of evaluating the language probe script in the tab.
    fn page_language(&self, tab_id: i64) -> Result<String, String>;
    /// Activates the tab at `index` in the bar's order; returns the window as `windows.Window`, None if there is no such tab.
    fn activate_at(&self, index: i64, window_id: Option<i64>) -> Option<Value>;
    /// Ids of every tab, in the bar's order (for `tabs.query`).
    fn all_tab_ids(&self) -> Vec<i64> {
        vec![]
    }
    /// Text of a file of the extension.
    fn read_extension_file(&self, extension_id: &str, file: &str) -> Result<String, String>;
}

/// Script the browser layer evaluates for `page_language`.
pub const LANGUAGE_PROBE: &str = r#"(document.documentElement && document.documentElement.lang) || (document.querySelector('meta[http-equiv="content-language" i]') || {}).content || ''"#;

#[derive(Debug, Clone, PartialEq)]
pub struct CaptureArgs {
    pub window_id: Option<i64>,
    pub format: String,
    pub quality: i64,
}

/// `captureVisibleTab([windowId], [options])`: either argument may be missing, options may come first.
pub fn parse_capture_args(first: &Value, second: &Value) -> ApiResult<CaptureArgs> {
    let options_first = first.is_object();
    let options = if options_first { first } else { second };
    let window_id = if !options_first { first.as_i64().filter(|w| *w >= 0) } else { None };
    if let Some(f) = options.get("format").filter(|f| !f.is_null()) {
        if f.as_str() != Some("jpeg") && f.as_str() != Some("png") {
            return Err(ApiError::new(format!("Error in invocation of tabs.captureVisibleTab: invalid format '{}'.", f.as_str().map(String::from).unwrap_or_else(|| f.to_string()))));
        }
    }
    let q = options.get("quality").and_then(|q| q.as_f64()).filter(|q| q.is_finite()).unwrap_or(DEFAULT_QUALITY as f64);
    Ok(CaptureArgs {
        window_id,
        format: if options.get("format").and_then(|f| f.as_str()) == Some("png") { "png".into() } else { "jpeg".into() },
        quality: (q.round() as i64).clamp(0, 100),
    })
}

/// The primary language of an `<html lang>` value, the way `tabs.detectLanguage` reports it: `en-US` gives `en`, Chinese
/// keeps its script (`zh-CN`, `zh-TW`), anything that is not a language code gives `und`.
pub fn primary_language(raw: &str) -> String {
    let lower = raw.trim().to_lowercase();
    let parts: Vec<&str> = lower.split(['-', '_']).collect();
    let primary = parts[0];
    if !(2..=3).contains(&primary.len()) || !primary.chars().all(|c| c.is_ascii_lowercase()) {
        return "und".into();
    }
    if primary == "zh" {
        return if parts.iter().any(|p| ["tw", "hk", "mo", "hant"].contains(p)) { "zh-TW".into() } else { "zh-CN".into() };
    }
    primary.into()
}

#[derive(Debug, Clone, PartialEq)]
pub struct CssDetails {
    pub code: Option<String>,
    pub file: Option<String>,
    pub css_origin: String,
}

/// Normalises the details of `insertCSS` / `removeCSS`; exactly one of `code` and `file` is required, like Chrome.
pub fn parse_css_details(raw: &Value) -> ApiResult<CssDetails> {
    let code = raw.get("code").and_then(|c| c.as_str()).map(String::from);
    let file = raw.get("file").and_then(|c| c.as_str()).filter(|f| !f.is_empty()).map(String::from);
    if code.is_some() == file.is_some() {
        return Err(ApiError::new("Exactly one of \"code\" or \"file\" must be specified."));
    }
    let css_origin = if raw.get("cssOrigin").and_then(|o| o.as_str()) == Some("user") { "user" } else { "author" };
    Ok(CssDetails { code, file, css_origin: css_origin.into() })
}

/// Identity of an injection: what `removeCSS` must be given again to remove it.
pub fn css_source(d: &CssDetails) -> String {
    format!("{}|{}|{}", d.css_origin, if d.file.is_some() { "file" } else { "code" }, d.file.as_ref().or(d.code.as_ref()).cloned().unwrap_or_default())
}

/// Keys returned by the browser's CSS injection, per extension, tab and source.
#[derive(Default)]
pub struct CssRegistry {
    keys: HashMap<(String, i64, String), Vec<String>>,
}

impl CssRegistry {
    pub fn add(&mut self, ext: &str, tab: i64, source: &str, key: String) {
        self.keys.entry((ext.into(), tab, source.into())).or_default().push(key);
    }

    /// Removes and returns every key of this source (a stylesheet inserted twice is removed twice).
    pub fn take(&mut self, ext: &str, tab: i64, source: &str) -> Vec<String> {
        self.keys.remove(&(ext.into(), tab, source.into())).unwrap_or_default()
    }

    pub fn forget_tab(&mut self, tab: i64) {
        self.keys.retain(|(_, t, _), _| *t != tab);
    }

    pub fn forget_extension(&mut self, ext: &str) {
        self.keys.retain(|(e, _, _), _| e != ext);
    }
}

/// The `chrome.tabs` members that need Signet's help (the rest is answered from [`TabHost::tab`] by the host).
#[derive(Default)]
pub struct TabsExtras {
    css: CssRegistry,
}

const NO_TAB: &str = "No tab found.";

impl TabsExtras {
    pub fn forget_tab(&mut self, tab: i64) {
        self.css.forget_tab(tab);
    }

    pub fn forget_extension(&mut self, ext: &str) {
        self.css.forget_extension(ext);
    }

    fn tab(host: &dyn TabHost, tab_id: &Value) -> ApiResult<TabInfo> {
        match tab_id.as_i64() {
            Some(id) => host.tab(id).ok_or_else(|| ApiError::new(format!("No tab with id: {id}."))),
            None => host.active_tab(None).ok_or_else(|| ApiError::new(NO_TAB)),
        }
    }

    fn checked(access: &Access, ext: &str, tab: TabInfo) -> ApiResult<TabInfo> {
        if !can_access_url(access, ext, &tab.url) {
            return Err(ApiError::new(format!("Cannot access contents of url \"{}\". Extension manifest must request permission to access this host.", tab.url)));
        }
        Ok(tab)
    }

    pub fn capture(&self, host: &dyn TabHost, access: &Access, ext: &str, first: &Value, second: &Value) -> ApiResult<String> {
        let args = parse_capture_args(first, second)?;
        let tab = host.active_tab(args.window_id).ok_or_else(|| ApiError::new(NO_TAB))?;
        if !can_access_url(access, ext, &tab.url) {
            return Err(ApiError::new("Either the '<all_urls>' or 'activeTab' permission is required."));
        }
        host.capture(tab.id, &args.format, args.quality).map_err(ApiError::new)
    }

    pub fn highlight(&self, host: &dyn TabHost, info: &Value) -> ApiResult<Value> {
        let tabs = info.get("tabs").cloned().unwrap_or(Value::Null);
        let list = if tabs.is_array() { tabs } else { Value::Array(vec![tabs]) };
        let indexes: Vec<i64> = list.as_array().into_iter().flatten().filter_map(|n| n.as_i64()).filter(|n| *n >= 0).collect();
        let Some(first) = indexes.first() else { return Err(ApiError::new("No highlighted tab specified.")) };
        let window_id = info.get("windowId").and_then(|w| w.as_i64()).filter(|w| *w >= 0);
        host.activate_at(*first, window_id).ok_or_else(|| ApiError::new(format!("No tab at index: {first}.")))
    }

    pub fn insert_css(&mut self, host: &dyn TabHost, access: &Access, ext: &str, first: &Value, second: &Value) -> ApiResult<()> {
        let details = parse_css_details(if first.is_number() { second } else { first })?;
        let tab = Self::checked(access, ext, Self::tab(host, first)?)?;
        let css = match (&details.code, &details.file) {
            (Some(c), _) => c.clone(),
            (None, Some(f)) => host.read_extension_file(ext, f).map_err(|_| ApiError::new(format!("Could not load file: '{f}'.")))?,
            _ => String::new(),
        };
        let key = host.insert_css(tab.id, &css, &details.css_origin).map_err(ApiError::new)?;
        self.css.add(ext, tab.id, &css_source(&details), key);
        Ok(())
    }

    pub fn remove_css(&mut self, host: &dyn TabHost, access: &Access, ext: &str, first: &Value, second: &Value) -> ApiResult<()> {
        let details = parse_css_details(if first.is_number() { second } else { first })?;
        let tab = Self::checked(access, ext, Self::tab(host, first)?)?;
        for key in self.css.take(ext, tab.id, &css_source(&details)) {
            host.remove_css(tab.id, &key);
        }
        Ok(())
    }

    pub fn detect_language(&self, host: &dyn TabHost, tab_id: &Value) -> ApiResult<String> {
        let tab = Self::tab(host, tab_id)?;
        Ok(host.page_language(tab.id).map(|raw| primary_language(&raw)).unwrap_or_else(|_| "und".into()))
    }

    /// Router dispatch of the `tabs.*` members implemented here.
    pub fn call(&mut self, host: &dyn TabHost, access: &Access, ext: &str, method: &str, args: &[Value]) -> ApiResult<Value> {
        let a = |i: usize| args.get(i).cloned().unwrap_or(Value::Null);
        match method {
            "tabs.captureVisibleTab" => self.capture(host, access, ext, &a(0), &a(1)).map(|s| json!(s)),
            "tabs.highlight" => self.highlight(host, &a(0)),
            "tabs.insertCSS" => self.insert_css(host, access, ext, &a(0), &a(1)).map(|_| Value::Null),
            "tabs.removeCSS" => self.remove_css(host, access, ext, &a(0), &a(1)).map(|_| Value::Null),
            "tabs.detectLanguage" => self.detect_language(host, &a(0)).map(|s| json!(s)),
            _ => Err(ApiError::new(format!("Unknown Extension API - {method}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    const ID: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn access(perms: &[&str], origins: &[&str]) -> Access {
        Access { permissions: perms.iter().map(|s| s.to_string()).collect(), origins: origins.iter().map(|s| s.to_string()).collect() }
    }

    #[test]
    fn capture_args_defaults_and_positions() {
        let n = Value::Null;
        assert_eq!(parse_capture_args(&n, &n).unwrap(), CaptureArgs { window_id: None, format: "jpeg".into(), quality: 92 });
        assert_eq!(parse_capture_args(&json!(5), &json!({ "format": "png" })).unwrap(), CaptureArgs { window_id: Some(5), format: "png".into(), quality: 92 });
        assert_eq!(parse_capture_args(&json!({ "format": "jpeg", "quality": 40 }), &n).unwrap(), CaptureArgs { window_id: None, format: "jpeg".into(), quality: 40 });
    }

    #[test]
    fn capture_args_clamp_and_reject() {
        let a = parse_capture_args(&json!(-2), &json!({ "quality": 250 })).unwrap();
        assert_eq!((a.window_id, a.quality), (None, 100));
        assert_eq!(parse_capture_args(&json!(1), &json!({ "quality": -5.4 })).unwrap().quality, 0);
        assert!(parse_capture_args(&json!(1), &json!({ "format": "gif" })).unwrap_err().0.contains("format"));
    }

    #[test]
    fn primary_language_keeps_the_tag_and_chinese_script() {
        assert_eq!(primary_language("en-US"), "en");
        assert_eq!(primary_language(" FR_ca "), "fr");
        assert_eq!(primary_language("zh-Hant"), "zh-TW");
        assert_eq!(primary_language("zh"), "zh-CN");
        assert_eq!(primary_language(""), "und");
        assert_eq!(primary_language("x-default"), "und");
    }

    #[test]
    fn css_details_and_sources() {
        assert!(parse_css_details(&json!({})).is_err());
        assert!(parse_css_details(&json!({ "code": "a", "file": "b" })).is_err());
        let a = css_source(&parse_css_details(&json!({ "code": "a{}" })).unwrap());
        let b = css_source(&parse_css_details(&json!({ "code": "a{}", "cssOrigin": "user" })).unwrap());
        assert_ne!(a, b);
        assert_ne!(css_source(&parse_css_details(&json!({ "file": "a.css" })).unwrap()), css_source(&parse_css_details(&json!({ "code": "a.css" })).unwrap()));
    }

    #[test]
    fn css_keys_are_kept_per_extension_tab_and_source_and_taken_once() {
        let mut r = CssRegistry::default();
        r.add("e1", 1, "s", "k1".into());
        r.add("e1", 1, "s", "k2".into());
        r.add("e2", 1, "s", "k3".into());
        r.add("e1", 2, "s", "k4".into());
        assert_eq!(r.take("e1", 1, "s"), ["k1", "k2"]);
        assert!(r.take("e1", 1, "s").is_empty());
        r.forget_tab(1);
        assert!(r.take("e2", 1, "s").is_empty());
        r.forget_extension("e1");
        assert!(r.take("e1", 2, "s").is_empty());
    }

    struct Fake {
        url: String,
        log: Mutex<Vec<String>>,
        lang: Result<String, String>,
    }
    impl Fake {
        fn new(url: &str) -> Self {
            Fake { url: url.into(), log: Mutex::new(vec![]), lang: Ok("fr-CA".into()) }
        }
        fn info(&self) -> TabInfo {
            TabInfo { id: 7, url: self.url.clone(), active: true, ..Default::default() }
        }
    }
    impl TabHost for Fake {
        fn tab(&self, id: i64) -> Option<TabInfo> {
            (id == 7).then(|| self.info())
        }
        fn active_tab(&self, _: Option<i64>) -> Option<TabInfo> {
            Some(self.info())
        }
        fn capture(&self, _: i64, format: &str, quality: i64) -> Result<String, String> {
            Ok(if format == "png" { "data:image/png;base64,PNG".into() } else { format!("jpeg{quality}") })
        }
        fn insert_css(&self, _: i64, css: &str, origin: &str) -> Result<String, String> {
            let mut l = self.log.lock().unwrap();
            let key = format!("key{}", l.len() + 1);
            l.push(format!("insert {css} {origin}"));
            Ok(key)
        }
        fn remove_css(&self, _: i64, key: &str) {
            self.log.lock().unwrap().push(format!("remove {key}"));
        }
        fn page_language(&self, _: i64) -> Result<String, String> {
            self.lang.clone()
        }
        fn activate_at(&self, index: i64, _: Option<i64>) -> Option<Value> {
            (index < 3).then(|| json!({ "id": 1 }))
        }
        fn read_extension_file(&self, _: &str, file: &str) -> Result<String, String> {
            Ok(format!("/* {file} */"))
        }
    }

    #[test]
    fn capture_returns_jpeg_by_default_and_png_on_request() {
        let h = Fake::new("https://a.test/page");
        let t = TabsExtras::default();
        let acc = access(&[], &["<all_urls>"]);
        assert_eq!(t.capture(&h, &acc, ID, &Value::Null, &json!({ "quality": 10 })).unwrap(), "jpeg10");
        assert_eq!(t.capture(&h, &acc, ID, &json!(1), &json!({ "format": "png" })).unwrap(), "data:image/png;base64,PNG");
    }

    #[test]
    fn capture_needs_all_urls_active_tab_or_a_host_pattern() {
        let h = Fake::new("https://a.test/page");
        let t = TabsExtras::default();
        assert!(t.capture(&h, &access(&["tabs"], &[]), ID, &Value::Null, &Value::Null).unwrap_err().0.contains("permission"));
        assert!(t.capture(&h, &access(&["activeTab"], &[]), ID, &Value::Null, &Value::Null).is_ok());
        assert!(t.capture(&h, &access(&[], &["https://a.test/*"]), ID, &Value::Null, &Value::Null).is_ok());
    }

    #[test]
    fn insert_then_remove_css_removes_the_keys_of_the_same_source_only() {
        let h = Fake::new("https://a.test/page");
        let mut t = TabsExtras::default();
        let acc = access(&[], &["<all_urls>"]);
        t.insert_css(&h, &acc, ID, &json!(7), &json!({ "code": "a{}" })).unwrap();
        t.insert_css(&h, &acc, ID, &json!(7), &json!({ "file": "x.css" })).unwrap();
        t.remove_css(&h, &acc, ID, &json!(7), &json!({ "code": "a{}" })).unwrap();
        assert_eq!(*h.log.lock().unwrap(), ["insert a{} author", "insert /* x.css */ author", "remove key1"]);
        t.remove_css(&h, &acc, ID, &json!({ "file": "x.css" }), &Value::Null).unwrap();
        assert_eq!(h.log.lock().unwrap().last().unwrap(), "remove key2");
    }

    #[test]
    fn insert_css_is_refused_on_a_page_the_extension_cannot_access() {
        let h = Fake::new("https://a.test/page");
        let mut t = TabsExtras::default();
        assert!(t.insert_css(&h, &access(&[], &[]), ID, &json!(7), &json!({ "code": "a{}" })).unwrap_err().0.contains("Cannot access"));
    }

    #[test]
    fn detect_language_reads_the_page_language() {
        let mut h = Fake::new("https://a.test/page");
        let t = TabsExtras::default();
        assert_eq!(t.detect_language(&h, &json!(7)).unwrap(), "fr");
        h.lang = Err("x".into());
        assert_eq!(t.detect_language(&h, &Value::Null).unwrap(), "und");
    }

    #[test]
    fn highlight_activates_the_first_index_and_fails_without_such_a_tab() {
        let h = Fake::new("https://a.test/page");
        let t = TabsExtras::default();
        assert_eq!(t.highlight(&h, &json!({ "tabs": [1, 2] })).unwrap(), json!({ "id": 1 }));
        assert!(t.highlight(&h, &json!({ "tabs": 9 })).unwrap_err().0.contains("index"));
        assert!(t.highlight(&h, &json!({ "tabs": [] })).is_err());
    }

    #[test]
    fn chrome_tab_object() {
        let tab = TabInfo { id: 3, window_id: 1, index: 2, active: true, url: "https://a/".into(), title: "A".into(), loading: true, muted: true, group_id: -1, ..Default::default() };
        let full = tab.to_chrome_tab(true);
        assert_eq!((full["id"].clone(), full["status"].clone(), full["url"].clone(), full["mutedInfo"]["muted"].clone(), full["highlighted"].clone()), (json!(3), json!("loading"), json!("https://a/"), json!(true), json!(true)));
        let quiet = tab.to_chrome_tab(false);
        assert!(quiet.get("url").is_none() && quiet.get("title").is_none());
        assert_eq!(quiet["windowId"], 1);
    }
}

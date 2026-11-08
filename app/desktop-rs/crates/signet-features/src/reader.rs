//! Reader mode (port of the Rust-side logic of `main/features/reader.ts`): preferences, the script builders for the
//! overlay (the page scripts stay JavaScript, run by the app in an isolated world), re-probe timing rules.
//!
//! `@mozilla/readability` 0.6.0 (Apache 2.0, see `assets/readability/LICENSE.md`) is embedded: `READABILITY_JS` and
//! `READERABLE_JS`, used by `probe_script_default` and `open_script_default`.
use std::collections::HashMap;
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;

/// Isolated world reserved for reader mode (1001: Boosts, 1002: media, 1004: translation).
pub const WORLD: i32 = 1003;
/// After an in-page navigation (single-page sites), wait until the content is in place before examining.
pub const IN_PAGE_DELAY_MS: i64 = 900;
/// Between two examinations triggered by in-page navigations: at least this interval.
pub const IN_PAGE_MIN_GAP_MS: i64 = 5000;

pub const CLOSE_SCRIPT: &str = "window.__signetReader ? (window.__signetReader.close(), true) : false";
pub const NEXT_SCRIPT: &str = "window.__signetReader ? window.__signetReader.next() : null";
/// Reader view styling (inside a closed shadow root).
pub const CSS: &str = include_str!("assets/reader.css");

pub use signet_core::types::{ReaderFont, ReaderPrefs, ReaderTheme, ReaderWidth};

/// Validates preferences coming from the page.
pub fn clean_prefs(value: &Value) -> Option<ReaderPrefs> {
    let o = value.as_object()?;
    let s = |k: &str| o.get(k).and_then(|x| x.as_str());
    let d = ReaderPrefs::default();
    Some(ReaderPrefs {
        font: if s("font") == Some("sans") { ReaderFont::Sans } else { ReaderFont::Serif },
        size: o.get("size").and_then(|n| n.as_f64()).filter(|n| n.is_finite()).map(|n| n.round().clamp(14.0, 28.0) as u32).unwrap_or(d.size),
        theme: match s("theme") {
            Some("light") => ReaderTheme::Light,
            Some("sepia") => ReaderTheme::Sepia,
            Some("dark") => ReaderTheme::Dark,
            _ => ReaderTheme::Auto,
        },
        width: match s("width") {
            Some("narrow") => ReaderWidth::Narrow,
            Some("wide") => ReaderWidth::Wide,
            _ => ReaderWidth::Normal,
        },
    })
}

/// Address without its anchor.
pub fn page_key(url: &str) -> String {
    match url::Url::parse(url) {
        Ok(u) => format!("{}{}{}", u.origin().ascii_serialization(), u.path(), u.query().map(|q| format!("?{q}")).unwrap_or_default()),
        Err(_) => url.to_string(),
    }
}

/// Labels of the reader bar. Keys: smaller, bigger, serif, sans, themeAuto, themeLight, themeSepia, themeDark,
/// narrow, narrowTitle, normal, normalTitle, wide, wideTitle, close, closeTitle, minutes (with `{n}`).
pub const LABEL_KEYS: [&str; 17] = ["smaller", "bigger", "serif", "sans", "themeAuto", "themeLight", "themeSepia", "themeDark", "narrow", "narrowTitle", "normal", "normalTitle", "wide", "wideTitle", "close", "closeTitle", "minutes"];

fn json(v: &impl Serialize) -> String {
    serde_json::to_string(v).unwrap_or_else(|_| "null".into())
}

/// `Readability.js` of `@mozilla/readability` (Apache 2.0).
pub const READABILITY_JS: &str = include_str!("assets/readability/Readability.js");
/// `Readability-readerable.js` of `@mozilla/readability` (Apache 2.0).
pub const READERABLE_JS: &str = include_str!("assets/readability/Readability-readerable.js");

/// `probe_script` with the embedded `Readability-readerable.js`.
pub fn probe_script_default() -> String {
    probe_script(READERABLE_JS)
}

/// `open_script` with the embedded `Readability.js`.
pub fn open_script_default(prefs: &ReaderPrefs, labels: &HashMap<String, String>) -> String {
    open_script(READABILITY_JS, prefs, labels)
}

/// Script that examines the page: `{ readerable, lang }`. Evaluated as an expression.
pub fn probe_script(readerable_js: &str) -> String {
    include_str!("assets/reader-probe.js").replace("__READERABLE__", readerable_js)
}

/// Script that opens the reader view; evaluates to `open`, `none` (no article found) or `already`.
pub fn open_script(readability_js: &str, prefs: &ReaderPrefs, labels: &HashMap<String, String>) -> String {
    include_str!("assets/reader-open.js")
        .replace("__READABILITY__", readability_js)
        .replace("__PREFS__", &json(prefs))
        .replace("__LABELS__", &json(labels))
        .replace("__CSS__", &json(&CSS))
}

/// Message sent by the reader view (read by successive `NEXT_SCRIPT` calls).
#[derive(Debug, Clone, PartialEq)]
pub enum ReaderMessage {
    Prefs(ReaderPrefs),
    Close,
}

pub fn parse_message(v: &Value) -> Option<ReaderMessage> {
    match v.get("type")?.as_str()? {
        "prefs" => clean_prefs(v.get("prefs")?).map(ReaderMessage::Prefs),
        "close" => Some(ReaderMessage::Close),
        _ => None,
    }
}

/// Per-tab bookkeeping for the examinations (throttling after in-page navigations).
#[derive(Default)]
pub struct Probes {
    probed: HashMap<String, (String, i64)>,
    bases: HashMap<String, String>,
}

impl Probes {
    pub fn record(&mut self, tab: &str, url: &str, now_ms: i64) {
        self.probed.insert(tab.into(), (page_key(url), now_ms));
    }

    /// Address (without anchor) of the page being read: only another page closes the reading.
    pub fn set_base(&mut self, tab: &str, url: &str) {
        self.bases.insert(tab.into(), url.split('#').next().unwrap_or("").into());
    }

    /// In-page navigation while reading: true when the reading must be closed (not a simple anchor change).
    pub fn must_close_reading(&self, tab: &str, url: &str) -> bool {
        self.bases.get(tab).map(String::as_str) != Some(url.split('#').next().unwrap_or(""))
    }

    /// Delay before re-examining after an in-page navigation, or `None` when the content has not changed.
    pub fn reprobe_delay(&self, tab: &str, url: &str, now_ms: i64) -> Option<Duration> {
        let last = self.probed.get(tab);
        if last.is_some_and(|(k, _)| *k == page_key(url)) {
            return None;
        }
        let wait = last.map(|(_, at)| at + IN_PAGE_MIN_GAP_MS - now_ms).unwrap_or(0);
        Some(Duration::from_millis(IN_PAGE_DELAY_MS.max(wait) as u64))
    }

    pub fn forget(&mut self, tab: &str) {
        self.probed.remove(tab);
        self.bases.remove(tab);
    }
}

/// True if the address can be read.
pub fn can_read(url: &str) -> bool {
    let l = url.to_ascii_lowercase();
    l.starts_with("http:") || l.starts_with("https:") || l.starts_with("file:")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn embedded_readability() {
        assert!(READABILITY_JS.contains("Readability"));
        assert!(READERABLE_JS.contains("isProbablyReaderable"));
        let js = open_script_default(&ReaderPrefs::default(), &HashMap::new());
        assert!(!js.contains("__READABILITY__") && js.contains(READABILITY_JS));
        assert!(probe_script_default().contains(READERABLE_JS));
    }

    #[test]
    fn prefs_are_validated() {
        let p = clean_prefs(&json!({"font":"sans","size":99,"theme":"sepia","width":"wide"})).unwrap();
        assert_eq!((p.font, p.size, p.theme, p.width), (ReaderFont::Sans, 28, ReaderTheme::Sepia, ReaderWidth::Wide));
        let q = clean_prefs(&json!({"font":"comic","size":"x","theme":"neon","width":"huge"})).unwrap();
        assert_eq!(q, ReaderPrefs::default());
        assert_eq!(clean_prefs(&json!({"size": 3})).unwrap().size, 14);
        assert!(clean_prefs(&json!(null)).is_none());
        assert_eq!(serde_json::to_value(ReaderPrefs::default()).unwrap(), json!({"font":"serif","size":19,"theme":"auto","width":"normal"}));
    }

    #[test]
    fn keys_and_messages() {
        assert_eq!(page_key("https://a.com/p?q=1#frag"), "https://a.com/p?q=1");
        assert_eq!(page_key("not a url"), "not a url");
        assert_eq!(parse_message(&json!({"type":"close"})), Some(ReaderMessage::Close));
        assert!(matches!(parse_message(&json!({"type":"prefs","prefs":{"size":20}})), Some(ReaderMessage::Prefs(p)) if p.size == 20));
        assert_eq!(parse_message(&json!({"type":"x"})), None);
        assert!(can_read("https://a.com") && !can_read("signet://settings"));
    }

    #[test]
    fn scripts_are_filled() {
        let labels: HashMap<String, String> = LABEL_KEYS.iter().map(|k| (k.to_string(), format!("L-{k}"))).collect();
        let js = open_script("/*READABILITY*/", &ReaderPrefs::default(), &labels);
        assert!(js.starts_with("(() => {") && js.contains("/*READABILITY*/"));
        for marker in ["__READABILITY__", "__PREFS__", "__LABELS__", "__CSS__"] {
            assert!(!js.contains(marker), "{marker}");
        }
        assert!(js.contains(r#""size":19"#) && js.contains("L-minutes") && js.contains("window.__signetReader"));
        assert!(js.contains(r"/^\s*(javascript|vbscript|data:text\/html)/i"));
        let probe = probe_script("function isProbablyReaderable(){}");
        assert!(probe.contains("function isProbablyReaderable(){}") && !probe.contains("__READERABLE__"));
    }

    #[test]
    fn reprobe_rules() {
        let mut p = Probes::default();
        assert_eq!(p.reprobe_delay("t", "https://a.com/x", 1000), Some(Duration::from_millis(900)));
        p.record("t", "https://a.com/x#a", 1000);
        assert_eq!(p.reprobe_delay("t", "https://a.com/x#b", 1500), None);
        assert_eq!(p.reprobe_delay("t", "https://a.com/y", 2000), Some(Duration::from_millis(4000)));
        assert_eq!(p.reprobe_delay("t", "https://a.com/y", 9000), Some(Duration::from_millis(900)));
        p.set_base("t", "https://a.com/x#top");
        assert!(!p.must_close_reading("t", "https://a.com/x#other"));
        assert!(p.must_close_reading("t", "https://a.com/y"));
        p.forget("t");
        assert!(p.reprobe_delay("t", "https://a.com/x", 0).is_some());
    }
}

//! Ports `packages/shared/src/urls.ts` and the non-Electron parts of `app/desktop/src/main/core/urls.ts`
//! (search engine selection, `isAppUrl`). The JS module-level mutable state (search language, engine config)
//! lives in `SearchState`, owned by the app.

use crate::searchengines::{
    clean_engine_config, expand_template, keyword_hint, keyword_infos, parse_keyword, resolve_engines, EngineConfig, EngineSpec, KeywordInfo, KeywordMatch,
    DEFAULT_SEARCH_ENGINE,
};
use regex::Regex;
use std::sync::OnceLock;
use unicode_normalization::UnicodeNormalization;

fn re(cell: &'static OnceLock<Regex>, pat: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pat).expect("static regex"))
}

/// Address of the app's interface (`signetui:`), reserved for its own views: never typed nor loaded as is in a tab.
pub fn is_ui_address(url: &str) -> bool {
    static R: OnceLock<Regex> = OnceLock::new();
    re(&R, r"(?i)^[\s\x00-\x1f]*signetui:").is_match(url)
}

/// Turns an input into a URL, or returns None if it is rather a search.
pub fn to_url(input: &str) -> Option<String> {
    static KNOWN: OnceLock<Regex> = OnceLock::new();
    static OTHER: OnceLock<Regex> = OnceLock::new();
    static LOCAL: OnceLock<Regex> = OnceLock::new();
    static DOMAIN: OnceLock<Regex> = OnceLock::new();
    static HOSTPORT: OnceLock<Regex> = OnceLock::new();
    static INTERNAL: OnceLock<Regex> = OnceLock::new();
    static JS: OnceLock<Regex> = OnceLock::new();
    let text = input.trim();
    if text.is_empty() || text.chars().any(char::is_whitespace) {
        return None;
    }
    if let Some(c) = re(&INTERNAL, r"(?i)^signet:(?://)?(\S*)$").captures(text) {
        return Some(format!("signet://{}", &c[1]));
    }
    // Never javascript: (nor with leading spaces or control characters): it is a search.
    if re(&JS, r"(?i)^[\s\x00-\x1f]*javascript:").is_match(input) {
        return None;
    }
    // Never the interface address itself (signetui:): it does not load in a tab.
    if is_ui_address(text) {
        return None;
    }
    if re(&KNOWN, r"(?i)^(https?|file|ftp|about|data|view-source|devtools):").is_match(text) || re(&OTHER, r"(?i)^[a-z][a-z0-9+.-]*://").is_match(text) {
        return Some(text.to_string());
    }
    if re(&LOCAL, r"(?i)^(localhost|(\d{1,3}\.){3}\d{1,3})(:\d+)?([/?#]\S*)?$").is_match(text)
        || re(&HOSTPORT, r"(?i)^[a-z0-9-]+:\d{1,5}([/?#]\S*)?$").is_match(text)
    {
        return Some(format!("http://{text}"));
    }
    if re(&DOMAIN, r"(?i)^([a-z0-9-]+\.)+[a-z]{2,}(:\d+)?([/?#]\S*)?$").is_match(text) {
        return Some(format!("https://{text}"));
    }
    None
}

/// Network/TLS errors occurring before any response: retry over http (except DNS, -105).
pub fn is_https_fallback_error(code: i32) -> bool {
    if (-299..=-200).contains(&code) {
        return true; // ERR_CERT_*
    }
    [-100, -101, -102, -107, -109, -113, -118, -7, -324].contains(&code)
}

/// Input without scheme that `to_url` promoted to https:// (so eligible for the http fallback).
pub fn is_upgraded_input(input: &str, url: Option<&str>) -> bool {
    let Some(url) = url else { return false };
    url.starts_with("https://") && !input.trim().to_lowercase().starts_with("https:")
}

/// Fallback http URL for an https:// address that just failed, if it is indeed the typed page (not a redirect).
pub fn http_fallback(pending: &str, failed_url: &str) -> Option<String> {
    let mut a = url::Url::parse(pending).ok()?;
    let b = url::Url::parse(failed_url).ok()?;
    if a.scheme() != "https" || b.scheme() != "https" || a.host_str() != b.host_str() || a.port() != b.port() || a.path() != b.path() {
        return None;
    }
    a.set_scheme("http").ok()?;
    Some(a.to_string())
}

/// `kl` parameter of DuckDuckGo from a preferred locale (`fr-FR` gives `fr-fr`, `en-GB` gives `uk-en`); empty without region.
pub fn suggest_region(search_locale: &str) -> String {
    static R: OnceLock<Regex> = OnceLock::new();
    let Some(m) = re(&R, r"(?i)^([a-z]{2,3})-([a-z]{2})$").captures(search_locale) else { return String::new() };
    let lang = m[1].to_lowercase();
    let region = if m[2].eq_ignore_ascii_case("gb") { "uk".to_string() } else { m[2].to_lowercase() };
    if region == lang { format!("{region}-{region}") } else { format!("{region}-{lang}") }
}

/// Google search address; `search_language` is the `hl` value (see `SearchState::set_language`), empty for none.
pub fn search_url(query: &str, search_language: &str) -> String {
    let hl = if search_language.is_empty() { String::new() } else { format!("&hl={}", crate::searchengines::encode_component(search_language)) };
    format!("https://www.google.com/search?q={}{}", crate::searchengines::encode_component(query), hl)
}

/// Lowercase without accents, to compare labels.
pub fn fold(text: &str) -> String {
    text.nfd().filter(|c| !is_mark(*c)).collect::<String>().to_lowercase()
}

fn is_mark(c: char) -> bool {
    matches!(c as u32, 0x0300..=0x036f | 0x1ab0..=0x1aff | 0x1dc0..=0x1dff | 0x20d0..=0x20ff | 0xfe20..=0xfe2f)
}

pub use crate::site::site_of;

/// Search settings the command bar depends on (module-level state of the TS `urls.ts`).
#[derive(Debug, Clone)]
pub struct SearchState {
    /// `hl` language of Google results pages.
    search_language: String,
    /// Preferred language as is (`fr-FR`), for search suggestions.
    search_locale: String,
    engine_id: String,
    config: EngineConfig,
    /// Interface language (the Wikipedia preset follows it).
    engine_locale: String,
}

impl Default for SearchState {
    fn default() -> Self {
        SearchState {
            search_language: String::new(),
            search_locale: String::new(),
            engine_id: DEFAULT_SEARCH_ENGINE.into(),
            config: EngineConfig::default(),
            engine_locale: "en".into(),
        }
    }
}

impl SearchState {
    /// `setSearchLanguage`: `zh` and `pt` keep their region (zh-CN, zh-TW, pt-BR...), others keep the base language.
    pub fn set_language(&mut self, code: Option<&str>) {
        let c = code.unwrap_or("").trim().to_string();
        static R: OnceLock<Regex> = OnceLock::new();
        self.search_language = if re(&R, r"(?i)^(zh|pt)-").is_match(&c) { c.clone() } else { c.split('-').next().unwrap_or("").to_lowercase() };
        self.search_locale = c;
    }

    /// Google search address in the preferred language.
    pub fn search_url(&self, query: &str) -> String {
        search_url(query, &self.search_language)
    }

    pub fn suggest_region(&self) -> String {
        suggest_region(&self.search_locale)
    }

    /// Changes the default engine; an unknown or disabled one falls back to Google.
    pub fn set_search_engine(&mut self, id: Option<&str>) {
        self.engine_id = id.unwrap_or(DEFAULT_SEARCH_ENGINE).to_string();
    }

    /// Custom engines and keyword overrides (cleaned here again, like `setSearchEngines`).
    pub fn set_search_engines(&mut self, config: &EngineConfig) {
        let engines = serde_json::to_value(&config.search_engines).unwrap_or_default();
        let kw = serde_json::to_value(&config.search_keywords).unwrap_or_default();
        self.config = clean_engine_config(&engines, &kw);
    }

    pub fn set_search_locale(&mut self, locale: &str) {
        self.engine_locale = locale.to_string();
    }

    /// Every engine with its current keyword.
    pub fn search_engine_list(&self) -> Vec<EngineSpec> {
        resolve_engines(&self.config, Some(&self.engine_locale))
    }

    fn default_engine(&self) -> EngineSpec {
        let all = self.search_engine_list();
        all.iter()
            .find(|e| e.id == self.engine_id && e.enabled)
            .or_else(|| all.iter().find(|e| e.id == DEFAULT_SEARCH_ENGINE))
            .cloned()
            .unwrap_or_else(|| all[0].clone())
    }

    /// URL of the search for `query` at `engine`; Google keeps the page language (`hl`).
    pub fn search_url_at(&self, engine: &EngineSpec, query: &str) -> String {
        if engine.id == "google" { self.search_url(query) } else { expand_template(&engine.template, query) }
    }

    /// URL of the search for `query` at the default engine.
    pub fn engine_search_url(&self, query: &str) -> String {
        self.search_url_at(&self.default_engine(), query)
    }

    /// Name of the default engine, for labels.
    pub fn engine_name(&self) -> String {
        self.default_engine().name
    }

    /// `keyword query` typed in the bar: the engine and its results URL (query may be empty).
    pub fn keyword_search(&self, input: &str) -> Option<(EngineSpec, String, String)> {
        let list = self.search_engine_list();
        let KeywordMatch { engine, query } = parse_keyword(input, &list)?;
        let url = self.search_url_at(engine, &query);
        Some((engine.clone(), query, url))
    }

    /// Engine whose keyword is exactly `input`, for the "Press Tab" hint.
    pub fn keyword_hint_for(&self, input: &str) -> Option<EngineSpec> {
        keyword_hint(input, &self.search_engine_list()).cloned()
    }

    /// Keywords offered to the command bar.
    pub fn keyword_list(&self) -> Vec<KeywordInfo> {
        keyword_infos(&self.search_engine_list())
    }

    /// Suggestions request of `engine` (default engine if None): (engine id, URL), or None without suggestions address.
    pub fn engine_suggestions(&self, query: &str, engine: Option<&EngineSpec>) -> Option<(String, String)> {
        let default = self.default_engine();
        let engine = engine.unwrap_or(&default);
        let suggest = engine.suggest.as_ref()?;
        let kl = if engine.id == "duckduckgo" { self.suggest_region() } else { String::new() };
        let mut url = expand_template(suggest, query);
        if !kl.is_empty() {
            url.push_str(&format!("&kl={kl}"));
        }
        Some((engine.id.clone(), url))
    }
}

/// True if the URL is the app interface's (excluding fragment): never a web page.
pub fn is_app_url(url: Option<&str>) -> bool {
    let Some(url) = url.filter(|u| !u.is_empty()) else { return false };
    match url::Url::parse(url) {
        Ok(u) => u.scheme() == "signetui" && u.host_str() == Some("app") && u.path() == "/index.html",
        Err(_) => false,
    }
}

/// Document address (`signet://note/<id>`, `signet://easel/<id>`) for a note or Easel address as the Electron app may
/// have left it: its real address (`signetui://app/index.html#note/<id>`, or the dev server `http://localhost:<port>/#note/<id>`).
/// `None` for any other address, including the document address itself (already right).
/// Electron tabs store the document address, so the first-run import needs this only for stray real addresses.
pub fn electron_note_url(url: &str) -> Option<String> {
    let u = url::Url::parse(url.trim()).ok()?;
    let app = match u.scheme() {
        "signetui" => u.host_str() == Some("app") && u.path() == "/index.html",
        "http" => matches!(u.host_str(), Some("localhost" | "127.0.0.1")) && u.path() == "/",
        _ => false,
    };
    if !app {
        return None;
    }
    let (kind, id) = u.fragment()?.split_once('/')?;
    let kind = kind.to_ascii_lowercase();
    let valid = id.len() == 36
        && id.split('-').map(str::len).eq([8, 4, 4, 4, 12])
        && id.bytes().all(|b| b == b'-' || b.is_ascii_hexdigit());
    (matches!(kind.as_str(), "note" | "easel") && valid).then(|| format!("signet://{kind}/{}", id.to_ascii_lowercase()))
}

/// Normalizes a media session artwork URL for the card. Only `http(s)` and `data:image/` are kept (a `blob:` URL
/// is unreachable from the app). YouTube thumbnails (`sqp` variants are AVIF, which nothing here decodes) are
/// rewritten to the plain JPEG `mqdefault` of the same video.
pub fn artwork_url(src: &str) -> Option<String> {
    let src = src.trim();
    let lower = src.to_ascii_lowercase();
    if lower.starts_with("data:image/") {
        return Some(src.to_string());
    }
    let rest = if lower.starts_with("https://") {
        &src[8..]
    } else if lower.starts_with("http://") {
        &src[7..]
    } else {
        return None;
    };
    let (host, path) = rest.split_once('/').map_or((rest, ""), |(h, p)| (h, p));
    let host = host.to_ascii_lowercase();
    let youtube = host == "i.ytimg.com" || host.strip_prefix('i').and_then(|h| h.strip_suffix(".ytimg.com")).is_some_and(|d| d.len() == 1 && d.chars().all(|c| c.is_ascii_digit()));
    if youtube {
        let path = path.split(['?', '#']).next().unwrap_or("");
        let mut parts = path.split('/');
        if let (Some("vi" | "vi_webp"), Some(id), Some(file), None) = (parts.next(), parts.next(), parts.next(), parts.next()) {
            if !id.is_empty() && !file.is_empty() {
                let stem = file.split('.').next().unwrap_or("");
                let name = if stem.ends_with("_live") { "mqdefault_live" } else { "mqdefault" };
                return Some(format!("https://i.ytimg.com/vi/{id}/{name}.jpg"));
            }
        }
    }
    if host.is_empty() {
        return None;
    }
    Some(src.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::searchengines::CustomEngine;

    #[test]
    fn to_url_rejects() {
        for s in ["", "   ", "hello world", "example.com foo", "hello", "foo.", "a.b"] {
            assert_eq!(to_url(s), None, "{s}");
        }
    }

    #[test]
    fn to_url_keeps_schemes() {
        assert_eq!(to_url("https://example.com/a?b=1").unwrap(), "https://example.com/a?b=1");
        assert_eq!(to_url("HTTP://example.com").unwrap(), "HTTP://example.com");
        assert_eq!(to_url("file:///tmp/a.html").unwrap(), "file:///tmp/a.html");
        assert_eq!(to_url("about:blank").unwrap(), "about:blank");
        assert_eq!(to_url("view-source:https://example.com").unwrap(), "view-source:https://example.com");
        assert_eq!(to_url("chrome-extension://abc/page.html").unwrap(), "chrome-extension://abc/page.html");
        assert_eq!(to_url("ssh://host").unwrap(), "ssh://host");
    }

    #[test]
    fn to_url_internal_and_forbidden() {
        assert_eq!(to_url("signet:settings").unwrap(), "signet://settings");
        assert_eq!(to_url("signet://settings").unwrap(), "signet://settings");
        assert_eq!(to_url("SIGNET:history").unwrap(), "signet://history");
        assert_eq!(to_url("javascript:alert(1)"), None);
        assert_eq!(to_url("JavaScript:alert(1)"), None);
        assert_eq!(to_url("\u{1}javascript:alert(1)"), None);
        assert_eq!(to_url("signetui://app/index.html"), None);
        assert_eq!(to_url("SIGNETUI:x"), None);
    }

    #[test]
    fn to_url_hosts_and_domains() {
        assert_eq!(to_url("localhost").unwrap(), "http://localhost");
        assert_eq!(to_url("localhost:3000/x").unwrap(), "http://localhost:3000/x");
        assert_eq!(to_url("192.168.1.10:8080").unwrap(), "http://192.168.1.10:8080");
        assert_eq!(to_url("nas:8080").unwrap(), "http://nas:8080");
        assert_eq!(to_url("example.com").unwrap(), "https://example.com");
        assert_eq!(to_url("sub.example.co.uk/path?q=1").unwrap(), "https://sub.example.co.uk/path?q=1");
        assert_eq!(to_url("example.com:8443").unwrap(), "https://example.com:8443");
        assert_eq!(to_url("  example.com  ").unwrap(), "https://example.com");
    }

    #[test]
    fn ui_address() {
        assert!(is_ui_address("signetui://app"));
        assert!(is_ui_address("  SIGNETUI:x"));
        assert!(!is_ui_address("https://signetui.com"));
    }

    #[test]
    fn http_fallback_rules() {
        assert!(is_https_fallback_error(-200) && is_https_fallback_error(-299) && is_https_fallback_error(-102));
        assert!(!is_https_fallback_error(-105) && !is_https_fallback_error(-3) && !is_https_fallback_error(0));
        assert!(is_upgraded_input("example.com", Some("https://example.com")));
        assert!(!is_upgraded_input("https://example.com", Some("https://example.com")));
        assert!(!is_upgraded_input("localhost", Some("http://localhost")));
        assert!(!is_upgraded_input("x", None));
        assert_eq!(http_fallback("https://example.com/a", "https://example.com/a").unwrap(), "http://example.com/a");
        assert_eq!(http_fallback("https://example.com/a", "https://other.com/a"), None);
        assert_eq!(http_fallback("https://example.com/a", "https://example.com/b"), None);
        assert_eq!(http_fallback("http://example.com/a", "http://example.com/a"), None);
        assert_eq!(http_fallback("nope", "https://example.com"), None);
    }

    #[test]
    fn search_language() {
        let mut s = SearchState::default();
        assert_eq!(s.search_url("a b&c"), "https://www.google.com/search?q=a%20b%26c");
        s.set_language(Some("fr-FR"));
        assert_eq!(s.search_url("x"), "https://www.google.com/search?q=x&hl=fr");
        s.set_language(None);
        assert_eq!(s.search_url("x"), "https://www.google.com/search?q=x");
        s.set_language(Some("zh-TW"));
        assert_eq!(s.search_url("x"), "https://www.google.com/search?q=x&hl=zh-TW");
        s.set_language(Some("pt-BR"));
        assert!(s.search_url("x").contains("&hl=pt-BR"));
    }

    #[test]
    fn region() {
        assert_eq!(suggest_region("fr-FR"), "fr-fr");
        assert_eq!(suggest_region("en-GB"), "uk-en");
        assert_eq!(suggest_region("en-US"), "us-en");
        assert_eq!(suggest_region("fr"), "");
    }

    #[test]
    fn folding() {
        assert_eq!(fold("\u{c9}l\u{e9}phant \u{c7}A"), "elephant ca");
    }

    fn docs() -> CustomEngine {
        CustomEngine { id: "custom-abc123".into(), name: "Docs".into(), keyword: "d".into(), template: "https://docs.example/s?q=%s".into(), suggest: Some("https://docs.example/sg?q=%s".into()) }
    }

    fn fresh() -> SearchState {
        let mut s = SearchState::default();
        s.set_language(Some("fr-FR"));
        s
    }

    #[test]
    fn engine_selection() {
        let mut s = fresh();
        assert_eq!(s.engine_search_url("a b"), "https://www.google.com/search?q=a%20b&hl=fr");
        s.set_search_engine(Some("duckduckgo"));
        assert_eq!(s.engine_search_url("a b"), "https://duckduckgo.com/?q=a%20b");
        s.set_search_engine(Some("nope"));
        assert!(s.engine_search_url("x").contains("google.com"));
    }

    #[test]
    fn suggestions() {
        let mut s = fresh();
        assert_eq!(s.engine_suggestions("a", None).unwrap().0, "google");
        s.set_search_engine(Some("duckduckgo"));
        assert_eq!(s.engine_suggestions("a", None).unwrap().1, "https://duckduckgo.com/ac/?q=a&type=list&kl=fr-fr");
        s.set_search_engine(Some("brave"));
        assert_eq!(s.engine_suggestions("a", None).unwrap().1, "https://search.brave.com/api/suggest?q=a");
    }

    #[test]
    fn custom_engine_default() {
        let mut s = fresh();
        s.set_search_engines(&EngineConfig { search_engines: vec![docs()], ..Default::default() });
        s.set_search_engine(Some("custom-abc123"));
        assert_eq!(s.engine_search_url("a b"), "https://docs.example/s?q=a%20b");
        assert_eq!(s.engine_suggestions("a b", None).unwrap(), ("custom-abc123".to_string(), "https://docs.example/sg?q=a%20b".to_string()));
        s.set_search_engines(&EngineConfig::default());
        assert!(s.engine_search_url("x").contains("google.com"));
        let mut d = docs();
        d.suggest = None;
        s.set_search_engines(&EngineConfig { search_engines: vec![d], ..Default::default() });
        assert!(s.engine_suggestions("a", None).is_none());
    }

    #[test]
    fn keyword_search_urls() {
        let mut s = fresh();
        assert_eq!(s.keyword_search("yt cats").unwrap().2, "https://www.youtube.com/results?search_query=cats");
        assert_eq!(s.keyword_search("w paris").unwrap().2, "https://en.wikipedia.org/wiki/Special:Search?search=paris");
        s.set_search_locale("fr");
        assert_eq!(s.keyword_search("w paris").unwrap().2, "https://fr.wikipedia.org/wiki/Special:Search?search=paris");
        assert!(s.keyword_search("w").is_none());
        assert!(s.keyword_search("w.org").is_none());
        let mut cfg = EngineConfig::default();
        cfg.search_keywords.insert("youtube".into(), crate::searchengines::KeywordOverride { keyword: Some("vid".into()), disabled: None });
        s.set_search_engines(&cfg);
        assert!(s.keyword_search("yt cats").is_none());
        assert_eq!(s.keyword_search("vid cats").unwrap().0.id, "youtube");
    }

    #[test]
    fn app_url() {
        assert!(is_app_url(Some("signetui://app/index.html#overlay")));
        assert!(!is_app_url(Some("https://example.com")));
        assert!(!is_app_url(None));
    }

    #[test]
    fn electron_note_urls_are_mapped() {
        let id = "0B9D0F3E-1c1a-4a53-9a6f-2f6d2b1c9e11";
        assert_eq!(electron_note_url(&format!("signetui://app/index.html#note/{id}")).as_deref(), Some("signet://note/0b9d0f3e-1c1a-4a53-9a6f-2f6d2b1c9e11"));
        assert_eq!(electron_note_url(&format!("http://localhost:5173/#easel/{id}")).as_deref(), Some("signet://easel/0b9d0f3e-1c1a-4a53-9a6f-2f6d2b1c9e11"));
        for u in [
            format!("signet://note/{id}"),
            format!("https://evil.example/index.html#note/{id}"),
            format!("http://evil.example/#note/{id}"),
            format!("signetui://app/index.html#folder/{id}"),
            "signetui://app/index.html#note/short".to_string(),
            "signetui://app/index.html".to_string(),
        ] {
            assert_eq!(electron_note_url(&u), None, "{u}");
        }
    }

    #[test]
    fn artwork_urls_are_normalized() {
        let sqp = "https://i.ytimg.com/vi/-sA9c3QGHJk/hqdefault.jpg?sqp=-oaymwEnCNACELwB&rs=AOn4CLB";
        assert_eq!(artwork_url(sqp).as_deref(), Some("https://i.ytimg.com/vi/-sA9c3QGHJk/mqdefault.jpg"));
        assert_eq!(artwork_url("https://i9.ytimg.com/vi_webp/abc/maxresdefault.webp").as_deref(), Some("https://i.ytimg.com/vi/abc/mqdefault.jpg"));
        assert_eq!(artwork_url("https://i.ytimg.com/vi/abc/maxresdefault_live.jpg?x=1").as_deref(), Some("https://i.ytimg.com/vi/abc/mqdefault_live.jpg"));
        assert_eq!(artwork_url("https://example.com/a.png?x=1").as_deref(), Some("https://example.com/a.png?x=1"));
        assert_eq!(artwork_url("data:image/png;base64,AAAA").as_deref(), Some("data:image/png;base64,AAAA"));
        assert_eq!(artwork_url("blob:https://example.com/1234"), None);
        assert_eq!(artwork_url("/relative.png"), None);
        assert_eq!(artwork_url(""), None);
    }
}

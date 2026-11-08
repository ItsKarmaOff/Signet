//! Internal `signet://` addresses (port of `main/web/internal.ts`): address conversion, suggestions, navigation
//! guard decisions and the HTML of the information pages. The app serves the pages and runs the actions.
use crate::notes::{note_address, note_of_app_url, resolve_note_address};
use crate::util::{esc, Translate};
use regex::Regex;
use std::sync::OnceLock;

pub const ENGINE_PAGES: &[&str] = &[
    "gpu", "media-internals", "webrtc-internals", "process-internals", "accessibility", "serviceworker-internals",
    "indexeddb-internals", "quota-internals", "histograms", "tracing", "ukm",
];
pub const SETTINGS_SECTIONS: &[&str] = &[
    "general", "appearance", "privacy", "passwords", "backup", "import", "boosts", "routing", "extensions", "apps",
    "shortcuts", "mods", "developer", "system", "about",
];
pub const LIBRARY_SECTIONS: &[&str] = &["history", "archive", "downloads", "spaces"];
const INFO_PAGES: &[&str] = &["urls", "version"];

fn re(cell: &'static OnceLock<Regex>, p: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(p).expect("static regex"))
}

/// `signet://name/path` (or `signet:name`) to (name, path), lowercased; `None` if not internal.
pub fn parse_internal(url: &str) -> Option<(String, String)> {
    static R: OnceLock<Regex> = OnceLock::new();
    let c = re(&R, r"(?i)^signet:(?://)?([^/?#]*)(?:/([^?#]*))?").captures(url.trim())?;
    Some((c[1].to_lowercase(), c.get(2).map(|m| m.as_str()).unwrap_or("").to_lowercase().trim_end_matches('/').to_string()))
}

fn is_engine(name: &str) -> bool {
    ENGINE_PAGES.contains(&name)
}

fn engine_name(url: &str) -> Option<String> {
    static R: OnceLock<Regex> = OnceLock::new();
    let c = re(&R, r"(?i)^chrome://([a-z0-9-]+)/?").captures(url)?;
    let n = c[1].to_lowercase();
    is_engine(&n).then_some(n)
}

/// Displayed address: an engine page appears as `signet://<name>`; a note's UI address as its note address.
pub fn display_url(app_base: &str, url: &str) -> String {
    if let Some((k, id)) = note_of_app_url(app_base, url) {
        return note_address(k, &id);
    }
    if let Some(n) = engine_name(url) {
        return format!("signet://{n}");
    }
    static R: OnceLock<Regex> = OnceLock::new();
    if re(&R, r"(?i)^signet://[^/?#]+/$").is_match(url) { url[..url.len() - 1].to_string() } else { url.to_string() }
}

/// Real address to load for a displayed address (inverse of `display_url`).
pub fn resolve_internal(app_base: &str, url: &str) -> String {
    if let Some(n) = resolve_note_address(app_base, url) {
        return n;
    }
    match parse_internal(url) {
        Some((name, _)) if is_engine(&name) => format!("chrome://{name}"),
        _ => url.to_string(),
    }
}

/// A typed `chrome://` or `chrome-untrusted://` address goes to `signet://not-found`.
pub fn refuse_engine_input(url: &str) -> String {
    static R: OnceLock<Regex> = OnceLock::new();
    if re(&R, r"(?i)^chrome(-untrusted)?:").is_match(url.trim()) { "signet://not-found".into() } else { url.to_string() }
}

pub fn is_internal_url(url: &str) -> bool {
    static R: OnceLock<Regex> = OnceLock::new();
    re(&R, r"(?i)^(signet(ui)?|chrome(-untrusted)?):").is_match(url)
}

pub fn is_engine_error_page(url: &str) -> bool {
    url.len() >= 13 && url[..13].eq_ignore_ascii_case("chrome-error:")
}

fn is_internal_origin(url: &str) -> bool {
    url.len() >= 7 && url[..7].eq_ignore_ascii_case("signet:") || engine_name(url).is_some()
}

/// Shortcut to a screen of the app (no tab opens).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Shortcut {
    Settings(Option<String>),
    Library(Option<String>),
}

pub fn shortcut(url: &str) -> Option<Shortcut> {
    let (name, path) = parse_internal(url)?;
    let sub = path.split('/').next().unwrap_or("").to_string();
    match name.as_str() {
        "settings" => Some(Shortcut::Settings((!sub.is_empty()).then_some(sub))),
        "extensions" | "about" => Some(Shortcut::Settings(Some(name))),
        "library" => Some(Shortcut::Library(LIBRARY_SECTIONS.contains(&sub.as_str()).then_some(sub))),
        "history" | "downloads" => Some(Shortcut::Library(Some(name))),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    Shortcuts,
    Pages,
    Engine,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Address {
    pub address: String,
    pub description: String,
    pub family: Family,
}

pub fn addresses(t: Translate) -> Vec<Address> {
    let mut v = Vec::new();
    let mut add = |a: String, d: String, f| v.push(Address { address: a, description: d, family: f });
    add("signet://settings".into(), t("internal.desc.settings", &[]), Family::Shortcuts);
    for s in SETTINGS_SECTIONS {
        add(format!("signet://settings/{s}"), t(&format!("internal.desc.settingsSection.{s}"), &[]), Family::Shortcuts);
    }
    add("signet://extensions".into(), t("internal.desc.extensions", &[]), Family::Shortcuts);
    add("signet://about".into(), t("internal.desc.about", &[]), Family::Shortcuts);
    add("signet://library".into(), t("internal.desc.library", &[]), Family::Shortcuts);
    for s in LIBRARY_SECTIONS {
        add(format!("signet://library/{s}"), t(&format!("internal.desc.librarySection.{s}"), &[]), Family::Shortcuts);
    }
    add("signet://history".into(), t("internal.desc.history", &[]), Family::Shortcuts);
    add("signet://downloads".into(), t("internal.desc.downloads", &[]), Family::Shortcuts);
    add("signet://urls".into(), t("internal.desc.urls", &[]), Family::Pages);
    add("signet://version".into(), t("internal.desc.version", &[]), Family::Pages);
    for n in ENGINE_PAGES {
        add(format!("signet://{n}"), t(&format!("internal.desc.engine.{n}"), &[]), Family::Engine);
    }
    v
}

/// Command bar suggestions for an input that starts with `signet:`.
pub fn internal_suggestions(t: Translate, query: &str) -> Vec<Address> {
    static R: OnceLock<Regex> = OnceLock::new();
    let q = query.trim().to_lowercase();
    let q = re(&R, r"^signet:(//)?").replace(&q, "signet://").into_owned();
    if !q.starts_with("signet://") {
        return vec![];
    }
    addresses(t).into_iter().filter(|a| a.address.starts_with(&q)).collect()
}

/// What the app must do with a page-initiated navigation to an internal address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Guard {
    /// Not an internal address: not handled here.
    NotInternal,
    /// Handled: refused with no navigation.
    Refuse,
    Shortcut(Shortcut),
    /// Handled: navigate the current tab to this real address.
    Navigate(String),
}

pub fn guard_navigation(app_base: &str, from: &str, target: &str) -> Guard {
    if !is_internal_url(target) {
        return Guard::NotInternal;
    }
    if !is_internal_origin(from) {
        return Guard::Refuse;
    }
    if let Some(s) = shortcut(target) {
        return Guard::Shortcut(s);
    }
    let trimmed = target.trim();
    static R: OnceLock<Regex> = OnceLock::new();
    if let Some(e) = engine_name(trimmed)
        && re(&R, r"(?i)^chrome://[^/?#]+/?$").is_match(trimmed) {
            return Guard::Navigate(format!("chrome://{e}"));
        }
    if let Some((name, path)) = parse_internal(target)
        && (INFO_PAGES.contains(&name.as_str()) || is_engine(&name)) && path.is_empty() {
            return Guard::Navigate(resolve_internal(app_base, target));
        }
    Guard::Navigate("signet://not-found".into())
}

// ------------------------------------------------------------- pages

pub const CSP: &str = "default-src 'none'; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

const CSS: &str = r#"
:root { color-scheme: light dark; --bg: rgb(246 246 248); --surface: #fff; --border: rgb(0 0 0 / .09); --text: rgb(29 29 31); --text-2: rgb(92 92 98); --accent: hsl(250 70% 55%); }
@media (prefers-color-scheme: dark) { :root { --bg: rgb(24 24 27); --surface: rgb(31 31 35); --border: rgb(255 255 255 / .09); --text: rgb(236 236 240); --text-2: rgb(165 164 173); --accent: hsl(250 75% 70%); } }
* { box-sizing: border-box }
body { margin: 0; background: var(--bg); color: var(--text); font: 13px/1.5 -apple-system, BlinkMacSystemFont, system-ui, sans-serif; }
main { max-width: 760px; margin: 0 auto; padding: 40px 24px 64px }
main.center { min-height: 100vh; display: flex; flex-direction: column; justify-content: center; align-items: center; text-align: center; padding-block: 24px }
h1 { font-size: 28px; margin: 0 0 4px; letter-spacing: -.01em }
h2 { font-size: 12px; font-weight: 600; text-transform: uppercase; letter-spacing: .04em; color: var(--text-2); margin: 28px 0 8px }
p.lead { color: var(--text-2); margin: 0 0 8px; font-size: 14px }
.card { background: var(--surface); border: .5px solid var(--border); border-radius: 12px; overflow: hidden }
.row { display: flex; gap: 16px; padding: 9px 14px; border-top: .5px solid var(--border); align-items: baseline }
.row:first-child { border-top: 0 }
.row a, .row .k { flex: 0 0 280px; font-family: ui-monospace, 'SF Mono', Menlo, monospace; font-size: 12px }
.row .k { flex-basis: 190px; font-family: inherit; color: var(--text-2) }
.row .v { word-break: break-word; user-select: text }
.row .d { color: var(--text-2) }
a { color: var(--accent); text-decoration: none }
a:hover { text-decoration: underline }
"#;

fn document(locale: &str, title: &str, body: &str, center: bool) -> String {
    let main = if center { "<main class=\"center\">" } else { "<main>" };
    format!(
        r#"<!doctype html><html lang="{}"><head><meta charset="utf-8"><title>{}</title><meta name="viewport" content="width=device-width,initial-scale=1"><style>{}</style></head><body>{}{}</main></body></html>"#,
        esc(locale), esc(title), CSS, main, body
    )
}

pub fn urls_page(t: Translate, locale: &str) -> String {
    let all = addresses(t);
    let groups = [(Family::Shortcuts, "internal.urls.shortcuts"), (Family::Pages, "internal.urls.pages"), (Family::Engine, "internal.urls.engine")];
    let sections: String = groups
        .iter()
        .map(|(fam, key)| {
            let rows: String = all
                .iter()
                .filter(|a| a.family == *fam)
                .map(|a| format!(r#"<div class="row"><a href="{0}">{0}</a><span class="d">{1}</span></div>"#, esc(&a.address), esc(&a.description)))
                .collect();
            format!(r#"<h2>{}</h2><div class="card">{}</div>"#, esc(&t(key, &[])), rows)
        })
        .collect();
    let title = t("internal.urls.title", &[]);
    document(locale, &title, &format!(r#"<h1>{}</h1><p class="lead">{}</p>{}"#, esc(&title), esc(&t("internal.urls.lead", &[])), sections), false)
}

/// Rows (label, value) are computed by the app (versions, OS, user agent, folders).
pub fn version_page(t: Translate, locale: &str, app_name_version: &str, rows: &[(String, String)]) -> String {
    let html: String = rows.iter().map(|(k, v)| format!(r#"<div class="row"><span class="k">{}</span><span class="v">{}</span></div>"#, esc(k), esc(v))).collect();
    let title = t("internal.version.title", &[]);
    document(locale, &title, &format!(r#"<h1>{}</h1><p class="lead">{}</p><h2>&nbsp;</h2><div class="card">{}</div>"#, esc(&title), esc(app_name_version), html), false)
}

pub fn not_found_page(t: Translate, locale: &str) -> String {
    let title = t("internal.notFound.title", &[]);
    document(locale, &title, &format!(r#"<h1>{}</h1><p class="lead">{}</p><p><a href="signet://urls">signet://urls</a></p>"#, esc(&title), esc(&t("internal.notFound.text", &[]))), true)
}

/// Which page `signet://...` serves: `urls`, `version`, or `None` for the not-found page (404).
pub fn page_for(url: &str) -> Option<&'static str> {
    let (name, path) = parse_internal(url)?;
    if !path.is_empty() { return None; }
    match name.as_str() { "urls" => Some("urls"), "version" => Some("version"), _ => None }
}

#[cfg(test)]
mod tests {
    use super::*;
    const ID: &str = "123e4567-e89b-12d3-a456-426614174000";
    const BASE: &str = "signetui://app/index.html";
    fn tr(k: &str, _: &[(&str, String)]) -> String { k.to_string() }

    #[test]
    fn conversions() {
        assert_eq!(display_url(BASE, "chrome://gpu"), "signet://gpu");
        assert_eq!(display_url(BASE, "chrome://GPU/"), "signet://gpu");
        assert_eq!(display_url(BASE, "chrome://settings"), "chrome://settings");
        assert_eq!(display_url(BASE, "signet://urls/"), "signet://urls");
        assert_eq!(display_url(BASE, "https://example.com/"), "https://example.com/");
        for n in ENGINE_PAGES { assert_eq!(resolve_internal(BASE, &format!("signet://{n}")), format!("chrome://{n}")); }
        assert_eq!(resolve_internal(BASE, "signet:gpu"), "chrome://gpu");
        assert_eq!(resolve_internal(BASE, "signet://GPU"), "chrome://gpu");
        assert_eq!(resolve_internal(BASE, "signet://urls"), "signet://urls");
        assert_eq!(resolve_internal(BASE, &format!("signet://note/{ID}")), format!("{BASE}#note/{ID}"));
        assert_eq!(resolve_internal(BASE, &format!("signet://easel/{ID}")), format!("{BASE}#easel/{ID}"));
        assert_eq!(display_url(BASE, &format!("{BASE}#note/{ID}")), format!("signet://note/{ID}"));
    }

    #[test]
    fn predicates() {
        assert!(is_internal_url("signet://settings"));
        assert!(!is_internal_url("signetu://x"));
        assert!(is_internal_url("signetui://app/index.html"));
        assert!(is_internal_url("chrome://gpu"));
        assert!(is_internal_url("chrome-untrusted://x"));
        assert!(!is_internal_url("https://signet.com"));
        assert!(is_engine_error_page("chrome-error://chromewebdata/"));
        assert!(!is_engine_error_page("chrome://gpu"));
    }

    #[test]
    fn refuse_input() {
        for u in ["chrome://settings", "chrome://gpu", "CHROME://Flags/", "chrome-untrusted://x"] { assert_eq!(refuse_engine_input(u), "signet://not-found"); }
        assert_eq!(refuse_engine_input("https://a.com"), "https://a.com");
        assert_eq!(refuse_engine_input("signet://settings"), "signet://settings");
    }

    #[test]
    fn suggestions() {
        assert!(internal_suggestions(&tr, "chrome://set").is_empty());
        assert!(internal_suggestions(&tr, "github").is_empty());
        assert!(internal_suggestions(&tr, "").is_empty());
        let a = internal_suggestions(&tr, "signet:settings");
        assert!(a.len() > 1 && a.iter().all(|r| r.address.starts_with("signet://settings")));
        assert_eq!(a[0].address, "signet://settings");
        let p: Vec<_> = internal_suggestions(&tr, "SIGNET://Settings/pri").into_iter().map(|a| a.address).collect();
        assert_eq!(p, vec!["signet://settings/privacy"]);
        let all: Vec<_> = internal_suggestions(&tr, "signet://").into_iter().map(|a| a.address).collect();
        assert!(all.contains(&"signet://urls".to_string()) && all.contains(&"signet://gpu".to_string()));
    }

    #[test]
    fn shortcuts() {
        assert_eq!(shortcut("signet://settings"), Some(Shortcut::Settings(None)));
        assert_eq!(shortcut("signet://settings/privacy"), Some(Shortcut::Settings(Some("privacy".into()))));
        assert_eq!(shortcut("signet://about"), Some(Shortcut::Settings(Some("about".into()))));
        assert_eq!(shortcut("signet://library/archive"), Some(Shortcut::Library(Some("archive".into()))));
        assert_eq!(shortcut("signet://library/inconnu"), Some(Shortcut::Library(None)));
        assert_eq!(shortcut("signet://history"), Some(Shortcut::Library(Some("history".into()))));
        assert_eq!(shortcut("signet://urls"), None);
        assert_eq!(shortcut("https://example.com"), None);
    }

    #[test]
    fn guard() {
        let g = |f: &str, t: &str| guard_navigation(BASE, f, t);
        assert_eq!(g("https://evil.com", "chrome://gpu"), Guard::Refuse);
        assert_eq!(g("https://evil.com", "signetui://app/index.html"), Guard::Refuse);
        assert_eq!(g("https://evil.com", "signet://settings"), Guard::Refuse);
        assert_eq!(g("https://a.com", "https://b.com"), Guard::NotInternal);
        assert_eq!(g("signet://urls", "chrome://gpu"), Guard::Navigate("chrome://gpu".into()));
        assert_eq!(g("chrome://process-internals", "chrome://GPU/"), Guard::Navigate("chrome://gpu".into()));
        for t in ["chrome://settings", "chrome://gpu/extra", "chrome-untrusted://terminal"] { assert_eq!(g("signet://urls", t), Guard::Navigate("signet://not-found".into())); }
        assert_eq!(g("signet://urls", "signet://settings/privacy"), Guard::Shortcut(Shortcut::Settings(Some("privacy".into()))));
        assert_eq!(g("signet://urls", "signet://nimporte"), Guard::Navigate("signet://not-found".into()));
        assert_eq!(g("signet://urls", "signet://version"), Guard::Navigate("signet://version".into()));
    }

    #[test]
    fn pages() {
        assert_eq!(page_for("signet://urls"), Some("urls"));
        assert_eq!(page_for("signet://urls/x"), None);
        assert!(urls_page(&tr, "en").contains("signet://gpu"));
        assert!(not_found_page(&tr, "en").contains("class=\"center\""));
        assert!(version_page(&tr, "en", "Signet 1", &[("a<".into(), "b".into())]).contains("a&lt;"));
    }
}

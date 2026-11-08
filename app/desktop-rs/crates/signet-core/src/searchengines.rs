//! Ports `packages/shared/src/searchengines.ts` and the engine table of `mobile/search.ts`:
//! search keywords and custom engines (command bar).

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const KEYWORD_MAX: usize = 32;
pub const ENGINE_NAME_MAX: usize = 60;
pub const CUSTOM_ENGINES_MAX: usize = 50;
const TEMPLATE_MAX: usize = 2048;

/// A built-in search engine (`mobile/search.ts`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchEngine {
    pub id: &'static str,
    /// Proper name: never translated.
    pub name: &'static str,
    /// Address template; `%s` is replaced by the encoded query.
    pub template: &'static str,
    /// Suggestions API address (`%s` = encoded query).
    pub suggest: &'static str,
    /// Default keyword.
    pub keyword: &'static str,
}

pub const SEARCH_ENGINES: [SearchEngine; 6] = [
    SearchEngine { id: "google", name: "Google", template: "https://www.google.com/search?q=%s", suggest: "https://suggestqueries.google.com/complete/search?client=firefox&q=%s", keyword: "g" },
    SearchEngine { id: "duckduckgo", name: "DuckDuckGo", template: "https://duckduckgo.com/?q=%s", suggest: "https://duckduckgo.com/ac/?q=%s&type=list", keyword: "ddg" },
    SearchEngine { id: "bing", name: "Bing", template: "https://www.bing.com/search?q=%s", suggest: "https://api.bing.com/osjson.aspx?query=%s", keyword: "b" },
    SearchEngine { id: "ecosia", name: "Ecosia", template: "https://www.ecosia.org/search?q=%s", suggest: "https://ac.ecosia.org/autocomplete?q=%s&type=list", keyword: "eco" },
    SearchEngine { id: "brave", name: "Brave", template: "https://search.brave.com/search?q=%s", suggest: "https://search.brave.com/api/suggest?q=%s", keyword: "br" },
    SearchEngine { id: "qwant", name: "Qwant", template: "https://www.qwant.com/?q=%s", suggest: "https://api.qwant.com/v3/suggest?q=%s&locale=en_US&version=2", keyword: "q" },
];

pub const DEFAULT_SEARCH_ENGINE: &str = "google";
pub const SUGGESTION_LIMIT: usize = 5;

pub fn is_search_engine_id(id: &str) -> bool {
    SEARCH_ENGINES.iter().any(|e| e.id == id)
}

/// Built-in engine by id (Google if unknown).
pub fn search_engine(id: &str) -> &'static SearchEngine {
    SEARCH_ENGINES.iter().find(|e| e.id == id).unwrap_or(&SEARCH_ENGINES[0])
}

fn encode_uri_component(s: &str) -> String {
    // Same unreserved set as JavaScript's encodeURIComponent: A-Z a-z 0-9 - _ . ! ~ * ' ( )
    const SET: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
        .remove(b'-')
        .remove(b'_')
        .remove(b'.')
        .remove(b'!')
        .remove(b'~')
        .remove(b'*')
        .remove(b'\'')
        .remove(b'(')
        .remove(b')');
    percent_encoding::utf8_percent_encode(s, SET).to_string()
}

/// JavaScript's `encodeURIComponent`.
pub fn encode_component(s: &str) -> String {
    encode_uri_component(s)
}

/// Address of the search `query` at built-in engine `id`.
pub fn search_url_for(id: &str, query: &str) -> String {
    search_engine(id).template.replacen("%s", &encode_uri_component(query), 1)
}

/// Address of the suggestions request for `query` at built-in engine `id`.
pub fn suggestions_url_for(id: &str, query: &str) -> String {
    search_engine(id).suggest.replacen("%s", &encode_uri_component(query), 1)
}

/// Input -> address to load: a URL if the input is one, otherwise a search; None if the input is empty.
pub fn resolve_input(input: &str, engine: &str) -> Option<String> {
    let text = input.trim();
    if text.is_empty() {
        return None;
    }
    Some(crate::urls::to_url(text).unwrap_or_else(|| search_url_for(engine, text)))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EngineSource {
    Builtin,
    Preset,
    Custom,
}

/// Engine added by the user. `suggest`: OpenSearch JSON suggestions address (`%s` = query).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct CustomEngine {
    pub id: String,
    pub name: String,
    pub keyword: String,
    pub template: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggest: Option<String>,
}

/// Change of a built-in or preset engine: another keyword, or (presets only) turned off.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct KeywordOverride {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keyword: Option<String>,
    /// Only ever `true` when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disabled: Option<bool>,
}

pub type KeywordOverrides = IndexMap<String, KeywordOverride>;

#[derive(Debug, Clone, Default)]
pub struct EngineConfig {
    pub search_engines: Vec<CustomEngine>,
    pub search_keywords: KeywordOverrides,
}

impl EngineConfig {
    pub fn from_settings(s: &crate::types::Settings) -> Self {
        EngineConfig { search_engines: s.search_engines.clone().unwrap_or_default(), search_keywords: s.search_keywords.clone().unwrap_or_default() }
    }
}

/// An engine as the command bar sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct EngineSpec {
    pub id: String,
    /// Proper name: never translated.
    pub name: String,
    pub source: EngineSource,
    pub template: String,
    pub suggest: Option<String>,
    pub keyword: String,
    /// Keyword without override (built-ins and presets).
    pub default_keyword: Option<String>,
    pub enabled: bool,
}

/// Site searches on by default. The Wikipedia host follows the interface language.
pub fn wikipedia_host(locale: Option<&str>) -> &'static str {
    let l = locale.unwrap_or("").to_lowercase();
    if l == "fr" || l.starts_with("fr-") || l.starts_with("fr_") { "fr.wikipedia.org" } else { "en.wikipedia.org" }
}

struct Preset {
    id: &'static str,
    name: &'static str,
    keyword: &'static str,
    template: fn(Option<&str>) -> String,
    suggest: Option<fn(Option<&str>) -> String>,
}

const PRESETS: [Preset; 3] = [
    Preset {
        id: "youtube",
        name: "YouTube",
        keyword: "yt",
        template: |_| "https://www.youtube.com/results?search_query=%s".into(),
        suggest: Some(|_| "https://suggestqueries.google.com/complete/search?client=firefox&ds=yt&q=%s".into()),
    },
    Preset {
        id: "wikipedia",
        name: "Wikipedia",
        keyword: "w",
        template: |l| format!("https://{}/wiki/Special:Search?search=%s", wikipedia_host(l)),
        suggest: Some(|l| format!("https://{}/w/api.php?action=opensearch&format=json&namespace=0&limit=5&search=%s", wikipedia_host(l))),
    },
    Preset { id: "github", name: "GitHub", keyword: "gh", template: |_| "https://github.com/search?q=%s".into(), suggest: None },
];

/// `%s` replaced (everywhere) by the encoded query.
pub fn expand_template(template: &str, query: &str) -> String {
    template.replace("%s", &encode_uri_component(query))
}

/// http(s) address containing `%s`.
pub fn is_search_template(v: &str) -> bool {
    if v.chars().count() > TEMPLATE_MAX || !v.contains("%s") {
        return false;
    }
    match url::Url::parse(&v.replace("%s", "x")) {
        Ok(u) => u.scheme() == "http" || u.scheme() == "https",
        Err(_) => false,
    }
}

/// Keyword as stored (trimmed, lowercase), or None if invalid (empty, spaces, too long).
pub fn normalize_keyword(v: &str) -> Option<String> {
    let k = v.trim().to_lowercase();
    if !k.is_empty() && k.chars().count() <= KEYWORD_MAX && !k.chars().any(char::is_whitespace) { Some(k) } else { None }
}

/// Every engine, in the order built-ins, presets, custom; keywords as overridden. Expects cleaned settings.
pub fn resolve_engines(cfg: &EngineConfig, locale: Option<&str>) -> Vec<EngineSpec> {
    let over = &cfg.search_keywords;
    let mut out: Vec<EngineSpec> = SEARCH_ENGINES
        .iter()
        .map(|e| EngineSpec {
            id: e.id.into(),
            name: e.name.into(),
            source: EngineSource::Builtin,
            template: e.template.into(),
            suggest: Some(e.suggest.into()),
            keyword: over.get(e.id).and_then(|o| o.keyword.clone()).unwrap_or_else(|| e.keyword.into()),
            default_keyword: Some(e.keyword.into()),
            enabled: true,
        })
        .collect();
    for p in &PRESETS {
        out.push(EngineSpec {
            id: p.id.into(),
            name: p.name.into(),
            source: EngineSource::Preset,
            template: (p.template)(locale),
            suggest: p.suggest.map(|f| f(locale)),
            keyword: over.get(p.id).and_then(|o| o.keyword.clone()).unwrap_or_else(|| p.keyword.into()),
            default_keyword: Some(p.keyword.into()),
            enabled: over.get(p.id).and_then(|o| o.disabled) != Some(true),
        });
    }
    for c in &cfg.search_engines {
        out.push(EngineSpec {
            id: c.id.clone(),
            name: c.name.clone(),
            source: EngineSource::Custom,
            template: c.template.clone(),
            suggest: c.suggest.clone(),
            keyword: c.keyword.clone(),
            default_keyword: None,
            enabled: true,
        });
    }
    out
}

fn custom_id_ok(id: &str) -> bool {
    id.strip_prefix("custom-").is_some_and(|rest| (4..=32).contains(&rest.len()) && rest.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()))
}

/// Keywords cleaned: invalid or clashing entries dropped, the first one wins. Takes untyped JSON values.
pub fn clean_engine_config(search_engines: &Value, search_keywords: &Value) -> EngineConfig {
    let base = resolve_engines(&EngineConfig::default(), None);
    let mut keywords: IndexMap<String, String> = base.iter().map(|e| (e.id.clone(), e.keyword.clone())).collect();
    fn taken(keywords: &IndexMap<String, String>, k: &str, own: Option<&str>) -> bool {
        keywords.iter().any(|(id, v)| Some(id.as_str()) != own && v == k)
    }
    let mut out_kw = KeywordOverrides::new();
    let over = search_keywords.as_object();
    for e in &base {
        let Some(o) = over.and_then(|m| m.get(&e.id)).and_then(Value::as_object) else { continue };
        let mut entry = KeywordOverride::default();
        if let Some(k) = o.get("keyword").and_then(Value::as_str).and_then(normalize_keyword)
            && Some(&k) != e.default_keyword.as_ref()
            && !taken(&keywords, &k, Some(&e.id))
        {
            keywords.insert(e.id.clone(), k.clone());
            entry.keyword = Some(k);
        }
        if e.source == EngineSource::Preset && o.get("disabled") == Some(&Value::Bool(true)) {
            entry.disabled = Some(true);
        }
        if entry.keyword.is_some() || entry.disabled.is_some() {
            out_kw.insert(e.id.clone(), entry);
        }
    }
    let mut engines: Vec<CustomEngine> = Vec::new();
    let mut ids = std::collections::HashSet::new();
    for c in search_engines.as_array().map(Vec::as_slice).unwrap_or(&[]) {
        if engines.len() >= CUSTOM_ENGINES_MAX {
            continue;
        }
        let Some(c) = c.as_object() else { continue };
        let id = c.get("id").and_then(Value::as_str).unwrap_or("");
        let k = c.get("keyword").and_then(Value::as_str).and_then(normalize_keyword);
        let n = c.get("name").and_then(Value::as_str).map(str::trim).unwrap_or("");
        let template = c.get("template").and_then(Value::as_str).unwrap_or("");
        if !custom_id_ok(id) || ids.contains(id) {
            continue;
        }
        let Some(k) = k else { continue };
        if n.is_empty() || n.chars().count() > ENGINE_NAME_MAX || taken(&keywords, &k, None) || !is_search_template(template) {
            continue;
        }
        ids.insert(id.to_string());
        keywords.insert(id.to_string(), k.clone());
        let suggest = c.get("suggest").and_then(Value::as_str).filter(|s| is_search_template(s)).map(String::from);
        engines.push(CustomEngine { id: id.into(), name: n.into(), keyword: k, template: template.into(), suggest });
    }
    EngineConfig { search_engines: engines, search_keywords: out_kw }
}

/// Default engine to keep: it must exist and be enabled; otherwise None (= Google).
pub fn clean_default_engine(id: Option<&str>, engines: &[EngineSpec]) -> Option<String> {
    let id = id?;
    engines.iter().any(|e| e.id == id && e.enabled).then(|| id.to_string())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineDraftError {
    Name,
    Keyword,
    KeywordTaken,
    Template,
    Suggest,
}

pub struct EngineDraft<'a> {
    pub name: &'a str,
    pub keyword: &'a str,
    pub template: &'a str,
    pub suggest: &'a str,
}

/// Checks the add or edit form of a custom engine. `self_id`: the engine being edited.
pub fn validate_engine_draft(d: &EngineDraft, engines: &[EngineSpec], self_id: Option<&str>) -> Option<EngineDraftError> {
    let name = d.name.trim();
    if name.is_empty() || name.chars().count() > ENGINE_NAME_MAX {
        return Some(EngineDraftError::Name);
    }
    let Some(k) = normalize_keyword(d.keyword) else { return Some(EngineDraftError::Keyword) };
    if engines.iter().any(|e| Some(e.id.as_str()) != self_id && e.keyword == k) {
        return Some(EngineDraftError::KeywordTaken);
    }
    if !is_search_template(d.template.trim()) {
        return Some(EngineDraftError::Template);
    }
    if !d.suggest.trim().is_empty() && !is_search_template(d.suggest.trim()) {
        return Some(EngineDraftError::Suggest);
    }
    None
}

/// A keyword typed in the command bar, followed by a query.
#[derive(Debug, Clone, PartialEq)]
pub struct KeywordMatch<'a> {
    pub engine: &'a EngineSpec,
    /// Text after the keyword, trimmed (empty while the user has only typed the keyword and a space).
    pub query: String,
}

/// `keyword query`: the first token must equal an enabled engine's keyword (case-insensitive) AND be followed by
/// whitespace. `w.org` or a lone `w` are not keyword searches.
pub fn parse_keyword<'a>(input: &str, engines: &'a [EngineSpec]) -> Option<KeywordMatch<'a>> {
    let s = input.trim_start();
    let end = s.find(char::is_whitespace)?;
    if end == 0 {
        return None;
    }
    let token = s[..end].to_lowercase();
    // One whitespace char is consumed after the token, the rest is the query.
    let rest = &s[end..];
    let rest = &rest[rest.chars().next().map_or(0, char::len_utf8)..];
    let engine = engines.iter().find(|e| e.enabled && e.keyword == token)?;
    Some(KeywordMatch { engine, query: rest.trim().to_string() })
}

/// The engine whose keyword is exactly what was typed (no space yet): the bar offers "Press Tab to search ...".
pub fn keyword_hint<'a>(input: &str, engines: &'a [EngineSpec]) -> Option<&'a EngineSpec> {
    let token = input.trim().to_lowercase();
    if token.is_empty() || token.chars().any(char::is_whitespace) {
        return None;
    }
    engines.iter().find(|e| e.enabled && e.keyword == token)
}

/// Address of the engine's icon (its site's `/favicon.ico`), or None.
pub fn engine_favicon(template: &str) -> Option<String> {
    let u = url::Url::parse(&template.replace("%s", "x")).ok()?;
    Some(format!("{}/favicon.ico", u.origin().ascii_serialization()))
}

/// What the command bar needs to know about a keyword.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KeywordInfo {
    pub id: String,
    pub keyword: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub favicon: Option<String>,
}

pub fn keyword_infos(engines: &[EngineSpec]) -> Vec<KeywordInfo> {
    engines
        .iter()
        .filter(|e| e.enabled)
        .map(|e| KeywordInfo { id: e.id.clone(), keyword: e.keyword.clone(), name: e.name.clone(), favicon: engine_favicon(&e.template) })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn engines() -> Vec<EngineSpec> {
        resolve_engines(&EngineConfig::default(), Some("en"))
    }
    fn custom() -> serde_json::Value {
        json!({"id":"custom-abc123","name":"Docs","keyword":"d","template":"https://docs.example/s?q=%s","suggest":"https://docs.example/sg?q=%s"})
    }
    fn custom_engine() -> CustomEngine {
        serde_json::from_value(custom()).unwrap()
    }

    #[test]
    fn parse_keyword_cases() {
        let e = engines();
        let m = parse_keyword("yt cats", &e).unwrap();
        assert_eq!((m.engine.id.as_str(), m.query.as_str()), ("youtube", "cats"));
        assert_eq!(parse_keyword("yt ", &e).unwrap().query, "");
        assert!(parse_keyword("yt", &e).is_none());
        assert!(parse_keyword("zz cats", &e).is_none());
        assert!(parse_keyword("w.org", &e).is_none());
        assert!(parse_keyword("w.org/page", &e).is_none());
        assert_eq!(parse_keyword("  YT   many   cats  ", &e).unwrap().query, "many   cats");
    }

    #[test]
    fn disabled_preset_does_not_trigger() {
        let mut cfg = EngineConfig::default();
        cfg.search_keywords.insert("youtube".into(), KeywordOverride { keyword: None, disabled: Some(true) });
        let off = resolve_engines(&cfg, None);
        assert!(parse_keyword("yt cats", &off).is_none());
        assert!(keyword_hint("yt", &off).is_none());
    }

    #[test]
    fn hint_exact_keyword_only() {
        let e = engines();
        assert_eq!(keyword_hint(" W ", &e).unwrap().id, "wikipedia");
        assert!(keyword_hint("w cats", &e).is_none());
        let infos = keyword_infos(&e);
        assert_eq!(infos.iter().find(|k| k.id == "github").unwrap().favicon.as_deref(), Some("https://github.com/favicon.ico"));
    }

    #[test]
    fn templates() {
        assert_eq!(expand_template("https://x.y/?q=%s&r=%s", "a b&c"), "https://x.y/?q=a%20b%26c&r=a%20b%26c");
        assert_eq!(expand_template("https://x.y/?q=%s", "$&%s"), "https://x.y/?q=%24%26%25s");
        assert!(is_search_template("https://a.b/?q=%s"));
        assert!(is_search_template("http://a.b/%s"));
        assert!(!is_search_template("https://a.b/"));
        assert!(!is_search_template("ftp://a.b/%s"));
        assert!(!is_search_template("javascript:%s"));
    }

    #[test]
    fn wikipedia_follows_language() {
        assert_eq!(wikipedia_host(Some("fr")), "fr.wikipedia.org");
        assert_eq!(wikipedia_host(Some("fr-FR")), "fr.wikipedia.org");
        assert_eq!(wikipedia_host(Some("en")), "en.wikipedia.org");
        assert_eq!(wikipedia_host(None), "en.wikipedia.org");
        let fr = resolve_engines(&EngineConfig::default(), Some("fr"));
        assert_eq!(fr.iter().find(|e| e.id == "wikipedia").unwrap().template, "https://fr.wikipedia.org/wiki/Special:Search?search=%s");
    }

    #[test]
    fn builtin_defaults() {
        let e = engines();
        let kws: Vec<_> = e.iter().filter(|e| e.source == EngineSource::Builtin).map(|e| e.keyword.as_str()).collect();
        assert_eq!(kws, ["g", "ddg", "b", "eco", "br", "q"]);
        assert_eq!(e.iter().find(|e| e.id == "github").unwrap().template, "https://github.com/search?q=%s");
    }

    #[test]
    fn cleaning_custom_engines() {
        let mut bad_id = custom();
        bad_id["id"] = json!("bad id");
        let mut bad_kw = custom();
        bad_kw["id"] = json!("custom-zzz999");
        bad_kw["keyword"] = json!("has space");
        let mut bad_tpl = custom();
        bad_tpl["id"] = json!("custom-nnn111");
        bad_tpl["template"] = json!("https://x.y");
        let out = clean_engine_config(&json!([custom(), bad_id, bad_kw, bad_tpl, null, 4]), &Value::Null);
        assert_eq!(out.search_engines, vec![custom_engine()]);
        let mut bad_sg = custom();
        bad_sg["suggest"] = json!("nope");
        assert!(clean_engine_config(&json!([bad_sg]), &Value::Null).search_engines[0].suggest.is_none());
    }

    #[test]
    fn duplicate_keywords_first_wins() {
        let mut a = custom();
        a["keyword"] = json!("yt");
        let mut b = custom();
        b["id"] = json!("custom-two222");
        b["keyword"] = json!("D");
        let mut c = custom();
        c["id"] = json!("custom-thr333");
        c["keyword"] = json!("d");
        let out = clean_engine_config(
            &json!([a, b, c]),
            &json!({"bing":{"keyword":"G"},"brave":{"keyword":"Brv"},"github":{"keyword":"yt"}}),
        );
        let ids: Vec<_> = out.search_engines.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, ["custom-two222"]);
        assert_eq!(out.search_engines[0].keyword, "d");
        assert_eq!(out.search_keywords.len(), 1);
        assert_eq!(out.search_keywords["brave"].keyword.as_deref(), Some("brv"));
    }

    #[test]
    fn overrides_rules() {
        let out = clean_engine_config(
            &Value::Null,
            &json!({"google":{"disabled":true,"keyword":"g"},"youtube":{"disabled":true},"nope":{"keyword":"x"}}),
        );
        assert_eq!(out.search_keywords.len(), 1);
        assert_eq!(out.search_keywords["youtube"].disabled, Some(true));
        let e = clean_engine_config(&json!("y"), &json!("x"));
        assert!(e.search_engines.is_empty() && e.search_keywords.is_empty());
    }

    #[test]
    fn keyword_normalization() {
        assert_eq!(normalize_keyword(" Yt ").as_deref(), Some("yt"));
        assert!(normalize_keyword("a b").is_none());
        assert!(normalize_keyword("").is_none());
        assert!(normalize_keyword(&"a".repeat(32)).is_some());
        assert!(normalize_keyword(&"a".repeat(33)).is_none());
    }

    #[test]
    fn draft_validation() {
        let all = resolve_engines(&EngineConfig { search_engines: vec![custom_engine()], ..Default::default() }, None);
        let ok = EngineDraft { name: "X", keyword: "x", template: "https://x.y/?q=%s", suggest: "" };
        assert_eq!(validate_engine_draft(&ok, &all, None), None);
        assert_eq!(validate_engine_draft(&EngineDraft { name: " ", ..ok }, &all, None), Some(EngineDraftError::Name));
        assert_eq!(validate_engine_draft(&EngineDraft { keyword: "a b", ..ok }, &all, None), Some(EngineDraftError::Keyword));
        assert_eq!(validate_engine_draft(&EngineDraft { keyword: "YT", ..ok }, &all, None), Some(EngineDraftError::KeywordTaken));
        assert_eq!(validate_engine_draft(&EngineDraft { keyword: "d", ..ok }, &all, Some("custom-abc123")), None);
        assert_eq!(validate_engine_draft(&EngineDraft { template: "https://x.y", ..ok }, &all, None), Some(EngineDraftError::Template));
        assert_eq!(validate_engine_draft(&EngineDraft { suggest: "x", ..ok }, &all, None), Some(EngineDraftError::Suggest));
    }

    #[test]
    fn default_engine_fallback() {
        let mut cfg = EngineConfig { search_engines: vec![custom_engine()], ..Default::default() };
        cfg.search_keywords.insert("youtube".into(), KeywordOverride { keyword: None, disabled: Some(true) });
        let all = resolve_engines(&cfg, None);
        assert_eq!(clean_default_engine(Some("custom-abc123"), &all).as_deref(), Some("custom-abc123"));
        assert_eq!(clean_default_engine(Some("bing"), &all).as_deref(), Some("bing"));
        assert_eq!(clean_default_engine(Some("youtube"), &all), None);
        assert_eq!(clean_default_engine(Some("custom-gone"), &all), None);
        assert_eq!(clean_default_engine(None, &all), None);
    }
}

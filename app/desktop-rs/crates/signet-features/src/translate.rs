//! Page translation (port of the Rust-side logic of `main/features/translate.ts`): language codes, the translation
//! service client with retries, run bookkeeping. The page script (`assets/translate.js`) stays JavaScript: the app
//! runs it in an isolated world and moves batches between the page and `translate_batch`.
use std::collections::HashMap;
use std::time::Duration;

use serde_json::Value;

use crate::http::Http;

/// Isolated world reserved for translation (1001: Boosts, 1002: media, 1003: reader).
pub const WORLD: i32 = 1004;
pub const ENDPOINT: &str = "https://translate.googleapis.com/translate_a/t";
/// Maximum size (characters) of a batch sent to the service.
pub const BATCH_CHARS: usize = 6000;
/// With no visible text left to translate after this delay, the page is considered translated.
pub const SETTLE_MS: u64 = 1500;

/// Page script (installs `window.__signetTr`), evaluated as an expression. Resolves to `true`.
pub fn install_script() -> String {
    include_str!("assets/translate.js").replace("__BATCH_CHARS__", &BATCH_CHARS.to_string())
}
pub const NEXT_SCRIPT: &str = "window.__signetTr ? window.__signetTr.next() : null";
pub const RESTORE_SCRIPT: &str = "window.__signetTr && window.__signetTr.restore()";
pub const TITLE_SCRIPT: &str = r#"window.__signetTr ? window.__signetTr.title() : """#;

pub fn set_title_script(translated: &str) -> String {
    format!("window.__signetTr && window.__signetTr.setTitle({})", serde_json::to_string(translated).unwrap_or_else(|_| "\"\"".into()))
}

pub fn apply_script(ids: &[i64], htmls: &[String]) -> String {
    format!(
        "window.__signetTr && window.__signetTr.apply({}, {})",
        serde_json::to_string(ids).unwrap_or_else(|_| "[]".into()),
        serde_json::to_string(htmls).unwrap_or_else(|_| "[]".into())
    )
}

/// Language code accepted by the service: the base language, except Chinese (simplified / traditional).
pub fn service_lang(code: &str) -> String {
    let lower = code.to_lowercase();
    if lower.starts_with("zh") {
        let traditional = ["tw", "hk", "mo", "hant"].iter().any(|t| lower.contains(t));
        return if traditional { "zh-TW".into() } else { "zh-CN".into() };
    }
    lower.split('-').next().unwrap_or("").to_string()
}

/// Target language: the first preferred site language, otherwise English.
pub fn target(languages: &[String]) -> String {
    service_lang(languages.first().map(String::as_str).unwrap_or("en"))
}

/// Language of the page if it is not among the preferred languages (translation is then offered), else `None`.
pub fn foreign(lang: &str, languages: &[String]) -> Option<String> {
    let code = lang.trim().to_lowercase();
    let re = regex::Regex::new(r"^[a-z]{2,3}(-[a-z0-9]+)*$").ok()?;
    if !re.is_match(&code) {
        return None;
    }
    let wanted: Vec<String> = languages.iter().map(|l| service_lang(l)).collect();
    if wanted.contains(&service_lang(&code)) { None } else { Some(code) }
}

/// URL-encoded form body: `q=a&q=b`.
fn form_body(texts: &[String]) -> String {
    texts.iter().map(|t| format!("q={}", percent_encoding::utf8_percent_encode(t, crate::livefolders::URI_COMPONENT))).collect::<Vec<_>>().join("&")
}

/// Reads the service response: `[[translation, detected language], ...]` (a single text may come back unwrapped).
pub fn parse_response(data: &Value, texts: &[String]) -> Result<Vec<String>, String> {
    let list: Vec<Value> = match data {
        Value::Array(a) if texts.len() == 1 && a.first().is_some_and(|x| x.is_string()) => vec![data.clone()],
        Value::Array(a) => a.clone(),
        _ => return Err("Unexpected response from the translation service".into()),
    };
    if list.len() != texts.len() {
        return Err("Unexpected response from the translation service".into());
    }
    Ok(list
        .iter()
        .enumerate()
        .map(|(i, item)| match item {
            Value::String(s) => s.clone(),
            Value::Array(a) if a.first().is_some_and(|x| x.is_string()) => a[0].as_str().unwrap_or("").to_string(),
            _ => texts[i].clone(),
        })
        .collect())
}

/// Sends a batch to the service; returns the translations in the same order. A few attempts with growing wait on
/// 429 / 5xx or network error. `sleep` is injectable for tests.
pub fn translate_batch(http: &dyn Http, texts: &[String], target: &str, html: bool, sleep: &dyn Fn(Duration)) -> Result<Vec<String>, String> {
    let url = format!("{ENDPOINT}?client=gtx&sl=auto&tl={}&format={}", percent_encoding::utf8_percent_encode(target, crate::livefolders::URI_COMPONENT), if html { "html" } else { "text" });
    let body = form_body(texts);
    let mut last: Option<crate::http::Response> = None;
    for attempt in 0..4u32 {
        let delay = Duration::from_millis(1000 * 2u64.pow(attempt));
        match http.post_form(&url, &body, Duration::from_secs(30), 8 * 1024 * 1024) {
            Ok(r) => {
                let done = r.ok() || (r.status != 429 && r.status < 500) || attempt == 3;
                last = Some(r);
                if done {
                    break;
                }
            }
            Err(e) => {
                if attempt == 3 {
                    return Err(e);
                }
            }
        }
        sleep(delay);
    }
    let r = last.ok_or_else(|| "no response".to_string())?;
    if !r.ok() {
        return Err(format!("HTTP {}", r.status));
    }
    let data: Value = serde_json::from_slice(&r.body).map_err(|e| e.to_string())?;
    parse_response(&data, texts)
}

/// Translation state of a tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranslationState {
    Working,
    On,
}

/// Token of the translation in progress per tab: a navigation or a return to the original invalidates it.
#[derive(Default)]
pub struct Runs {
    runs: HashMap<String, u64>,
    seq: u64,
}

impl Runs {
    pub fn start(&mut self, tab: &str) -> u64 {
        self.seq += 1;
        self.runs.insert(tab.into(), self.seq);
        self.seq
    }
    pub fn is_current(&self, tab: &str, run: u64) -> bool {
        self.runs.get(tab) == Some(&run)
    }
    pub fn cancel(&mut self, tab: &str) {
        self.runs.remove(tab);
    }
}

/// True if the tab's address can be translated.
pub fn can_translate(url: &str) -> bool {
    let l = url.to_ascii_lowercase();
    l.starts_with("http:") || l.starts_with("https:") || l.starts_with("file:")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{resp, FakeHttp};
    use serde_json::json;
    use std::cell::Cell;

    fn s(v: &[&str]) -> Vec<String> { v.iter().map(|x| x.to_string()).collect() }

    #[test]
    fn service_lang_cases() {
        for (c, want) in [("fr-FR", "fr"), ("EN", "en"), ("pt-BR", "pt"), ("zh", "zh-CN"), ("zh-CN", "zh-CN"), ("zh-Hans", "zh-CN"), ("zh-TW", "zh-TW"), ("zh-HK", "zh-TW"), ("zh-Hant", "zh-TW")] {
            assert_eq!(service_lang(c), want, "{c}");
        }
    }

    #[test]
    fn target_and_foreign() {
        assert_eq!(target(&s(&["fr-FR", "en"])), "fr");
        assert_eq!(target(&[]), "en");
        let langs = s(&["fr-FR", "fr", "en-US"]);
        assert_eq!(foreign("de", &langs), Some("de".into()));
        assert_eq!(foreign("EN-gb", &langs), None);
        assert_eq!(foreign("fr", &langs), None);
        assert_eq!(foreign("x", &langs), None);
        assert_eq!(foreign("", &langs), None);
    }

    #[test]
    fn response_shapes() {
        let t = s(&["a", "b"]);
        assert_eq!(parse_response(&json!([["A", "en"], "B"]), &t).unwrap(), s(&["A", "B"]));
        assert_eq!(parse_response(&json!(["A", "en"]), &s(&["a"])).unwrap(), s(&["A"]));
        assert_eq!(parse_response(&json!([[1], null]), &t).unwrap(), s(&["a", "b"]));
        assert!(parse_response(&json!([["A"]]), &t).is_err());
        assert!(parse_response(&json!({}), &t).is_err());
    }

    #[test]
    fn batch_posts_form_and_retries() {
        let calls = Cell::new(0);
        let seen = std::sync::Mutex::new(Vec::<String>::new());
        let http = FakeHttp(Box::new(move |u| {
            seen.lock().unwrap().push(u.to_string());
            let n = seen.lock().unwrap().len();
            if n < 3 { resp(429, "") } else { resp(200, r#"[["Bonjour","en"],["Monde","en"]]"#) }
        }));
        let slept = Cell::new(0);
        let out = translate_batch(&http, &s(&["Hello", "World & co"]), "fr", true, &|_| slept.set(slept.get() + 1)).unwrap();
        assert_eq!(out, s(&["Bonjour", "Monde"]));
        assert_eq!(slept.get(), 2);
        calls.set(1);
        let ok = FakeHttp(Box::new(|u| {
            assert!(u.starts_with("POST https://translate.googleapis.com/translate_a/t?client=gtx&sl=auto&tl=fr&format=text q=Hello&q=World%20%26%20co"), "{u}");
            resp(200, r#"[["Bonjour","en"],["Monde","en"]]"#)
        }));
        translate_batch(&ok, &s(&["Hello", "World & co"]), "fr", false, &|_| {}).unwrap();
    }

    #[test]
    fn batch_failures() {
        let bad = FakeHttp(Box::new(|_| resp(503, "")));
        assert_eq!(translate_batch(&bad, &s(&["a"]), "fr", false, &|_| {}).unwrap_err(), "HTTP 503");
        let net = FakeHttp(Box::new(|_| Err("offline".into())));
        assert_eq!(translate_batch(&net, &s(&["a"]), "fr", false, &|_| {}).unwrap_err(), "offline");
        let client_err = FakeHttp(Box::new(|_| resp(400, "")));
        assert_eq!(translate_batch(&client_err, &s(&["a"]), "fr", false, &|_| {}).unwrap_err(), "HTTP 400");
    }

    #[test]
    fn runs_and_scripts() {
        let mut r = Runs::default();
        let a = r.start("t");
        assert!(r.is_current("t", a));
        let b = r.start("t");
        assert!(!r.is_current("t", a) && r.is_current("t", b));
        r.cancel("t");
        assert!(!r.is_current("t", b));
        assert!(can_translate("https://a.com") && can_translate("file:///x") && !can_translate("signet://x"));
        let script = install_script();
        assert!(script.contains("const MAX = 6000") && !script.contains("__BATCH_CHARS__"));
        assert_eq!(set_title_script("a\"b"), "window.__signetTr && window.__signetTr.setTitle(\"a\\\"b\")");
        assert_eq!(apply_script(&[1], &s(&["<a i=0>x</a>"])), "window.__signetTr && window.__signetTr.apply([1], [\"<a i=0>x</a>\"])");
    }
}

//! JSON viewer: script the app injects at the end of a page load (port of main/devtools/jsonview.ts).
//!
//! When: `did-finish-load`-like event of a tab, address `http(s):` or `file:`, setting `jsonViewer` not false.
//! The script itself checks `document.contentType`, the size (20 MB) and `JSON.parse`, then lays a closed
//! shadow root over the page. It returns `ok`, `already`, `not-json`, `too-big` or `invalid`.

use crate::strings;
use serde_json::{Value, json};

const SCRIPT: &str = include_str!("../web/json/jsonview.js");
const CSS: &str = include_str!("../web/json/jsonview.css");

/// Beyond this size the JSON stays displayed as the browser shows it.
pub const MAX_BYTES: u64 = 20 * 1024 * 1024;

/// Pages the viewer may run on (checked before injecting anything).
pub fn applies_to(url: &str) -> bool {
    let u = url.trim_start().to_ascii_lowercase();
    u.starts_with("http://") || u.starts_with("https://") || u.starts_with("file:")
}

/// `application/json`, `application/<x>+json` or `text/json` (parameters ignored).
pub fn is_json_content_type(ct: &str) -> bool {
    let base = ct.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
    if base == "text/json" || base == "application/json" {
        return true;
    }
    base.strip_prefix("application/")
        .and_then(|rest| rest.strip_suffix("+json"))
        .is_some_and(|x| !x.is_empty() && x.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')))
}

/// Complete JavaScript to evaluate in the page: labels in `lang`, styles, then the viewer. `expose` keeps the shadow
/// root reachable on `window.__signetJson` (development tests only).
pub fn script(lang: &str, expose: bool) -> String {
    let lang = strings::pick_lang(lang);
    let mut labels = strings::json_view_labels(lang);
    labels.insert("locale".into(), Value::String(lang.into()));
    labels.insert("zeroIsOne".into(), Value::Bool(lang == "fr"));
    labels.insert("maxBytes".into(), json!(MAX_BYTES));
    labels.insert("css".into(), Value::String(CSS.into()));
    labels.insert("expose".into(), Value::Bool(expose));
    let labels = serde_json::to_string(&Value::Object(labels)).unwrap_or_else(|_| "{}".into());
    // `</script>` cannot appear in an evaluated string, but keep the JSON safe if it is ever put in a tag.
    let labels = labels.replace('<', "\\u003c");
    format!("(function(){{\n{SCRIPT}\nreturn signetJsonView({labels});\n}})()")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_types() {
        for ok in ["application/json", "application/json; charset=utf-8", "text/json", "application/vnd.api+json", "Application/LD+JSON"] {
            assert!(is_json_content_type(ok), "{ok}");
        }
        for no in ["text/html", "application/jsonp", "application/+json", "application/xml", "", "text/plain"] {
            assert!(!is_json_content_type(no), "{no}");
        }
    }

    #[test]
    fn applies_to_web_and_file_only() {
        assert!(applies_to("https://api.example.com/x.json"));
        assert!(applies_to("file:///tmp/a.json"));
        assert!(!applies_to("signet://note/x"));
        assert!(!applies_to("chrome://version"));
    }

    #[test]
    fn script_embeds_labels() {
        let s = script("fr", false);
        assert!(s.starts_with("(function(){"));
        assert!(s.contains("\"tree\":\"Arbre\""));
        assert!(s.contains("\"zeroIsOne\":true"));
        assert!(s.contains("\"maxBytes\":20971520"));
        assert!(s.contains("signetJsonView({"));
        assert!(script("en", false).contains("\"zeroIsOne\":false"));
    }
}

//! Site languages (port of `main/web/languages.ts`): normalization and Accept-Language.
use regex::Regex;
use std::sync::OnceLock;

fn valid() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^[A-Za-z]{2,3}(-[A-Za-z0-9]{2,8})*$").expect("static regex"))
}

/// Regional language followed by its base language, no duplicates.
pub fn expand_languages<S: AsRef<str>>(codes: &[S]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut add = |c: String| {
        if !c.is_empty() && !out.contains(&c) {
            out.push(c);
        }
    };
    for raw in codes {
        let code = raw.as_ref().trim().replace('_', "-");
        if !valid().is_match(&code) {
            continue;
        }
        let base = code.split('-').next().unwrap_or("").to_lowercase();
        add(code);
        add(base);
    }
    out
}

/// System languages expanded, or English when the system gives none.
pub fn default_languages(system: &[String]) -> Vec<String> {
    let l = expand_languages(system);
    if l.is_empty() { vec!["en-US".into(), "en".into()] } else { l }
}

/// Valid non-empty list (deduplicated, not expanded) or the system's.
pub fn normalize_languages(value: &serde_json::Value, system: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    if let Some(arr) = value.as_array() {
        for c in arr.iter().filter_map(|v| v.as_str()) {
            if valid().is_match(c) && !out.iter().any(|o| o == c) {
                out.push(c.to_string());
            }
        }
    }
    if out.is_empty() { default_languages(system) } else { out }
}

pub fn accept_language(languages: &[String]) -> String {
    languages.join(",")
}

/// Spell checker languages to enable on non-macOS systems: up to 3 matches among `available`.
pub fn spellcheck_languages(languages: &[String], available: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for code in languages {
        let base = code.split('-').next().unwrap_or("");
        let pick = available
            .iter()
            .find(|a| a.eq_ignore_ascii_case(code))
            .or_else(|| available.iter().find(|a| a.split('-').next() == Some(base)));
        if let Some(p) = pick
            && !out.contains(p) {
                out.push(p.clone());
            }
    }
    out.truncate(3);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn s(v: &[&str]) -> Vec<String> { v.iter().map(|x| x.to_string()).collect() }
    #[test]
    fn expand() {
        assert_eq!(expand_languages(&["en-US", "fr-FR"]), s(&["en-US", "en", "fr-FR", "fr"]));
        assert_eq!(expand_languages(&["en-US", "en-GB"]), s(&["en-US", "en", "en-GB"]));
        assert_eq!(expand_languages(&["pt_BR", "x", "12", ""]), s(&["pt-BR", "pt"]));
    }
    #[test]
    fn defaults() {
        assert_eq!(default_languages(&s(&["fr-FR", "en-US"])), s(&["fr-FR", "fr", "en-US", "en"]));
        assert_eq!(default_languages(&[]), s(&["en-US", "en"]));
        assert_eq!(normalize_languages(&json!(["fr-FR", "fr", "fr", "zz zz", 3]), &[]), s(&["fr-FR", "fr"]));
        let sys = s(&["de-DE"]);
        assert_eq!(normalize_languages(&json!([]), &sys), s(&["de-DE", "de"]));
        assert_eq!(normalize_languages(&json!("fr"), &sys), s(&["de-DE", "de"]));
        assert_eq!(normalize_languages(&json!(null), &sys), s(&["de-DE", "de"]));
    }
    #[test]
    fn accept() {
        assert_eq!(accept_language(&s(&["fr-FR", "fr", "en"])), "fr-FR,fr,en");
    }
    #[test]
    fn spell() {
        assert_eq!(spellcheck_languages(&s(&["fr-FR", "fr", "en-US"]), &s(&["fr-FR", "en-GB"])), s(&["fr-FR", "en-GB"]));
    }
}

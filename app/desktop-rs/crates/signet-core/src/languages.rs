//! Ports the pure parts of `app/desktop/src/main/web/languages.ts`: preferred languages announced to sites.

use regex::Regex;
use std::sync::OnceLock;

fn code_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^[A-Za-z]{2,3}(-[A-Za-z0-9]{2,8})*$").expect("static regex"))
}

/// Regional language followed by its base language, without duplicates: [en-US, fr-FR] gives [en-US, en, fr-FR, fr].
pub fn expand_languages(codes: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut add = |c: String| {
        if !c.is_empty() && !out.contains(&c) {
            out.push(c);
        }
    };
    for raw in codes {
        let code = raw.trim().replace('_', "-");
        if !code_re().is_match(&code) {
            continue;
        }
        add(code.clone());
        add(code.split('-').next().unwrap_or("").to_lowercase());
    }
    out
}

/// Default languages: the system's, or English if the system gives none.
pub fn default_languages(system: &[String]) -> Vec<String> {
    let list = expand_languages(system);
    if list.is_empty() { vec!["en-US".into(), "en".into()] } else { list }
}

/// Valid (non-empty) list or, failing that, the system's.
pub fn normalize_languages(value: &[String], system: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for c in value {
        if code_re().is_match(c) && !out.contains(c) {
            out.push(c.clone());
        }
    }
    if out.is_empty() { default_languages(system) } else { out }
}

/// Simple list `fr-FR,fr,en-US,en`: Chromium adds the weights itself.
pub fn accept_language(languages: &[String]) -> String {
    languages.join(",")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn expands() {
        assert_eq!(expand_languages(&v(&["en-US", "fr_FR", "bad code!", "fr"])), v(&["en-US", "en", "fr-FR", "fr"]));
    }

    #[test]
    fn defaults_and_normalize() {
        assert_eq!(default_languages(&[]), v(&["en-US", "en"]));
        assert_eq!(default_languages(&v(&["en-US"])), v(&["en-US", "en"]));
        assert_eq!(normalize_languages(&v(&["fr-FR", "x", "fr-FR"]), &[]), v(&["fr-FR"]));
        assert_eq!(normalize_languages(&[], &v(&["fr-CA"])), v(&["fr-CA", "fr"]));
        assert_eq!(accept_language(&v(&["fr-FR", "fr"])), "fr-FR,fr");
    }
}

//! Translations: the subset of `packages/shared/src/locales/{en,fr}` the pages need, read at build time.

use serde_json::{Map, Value};
use std::sync::OnceLock;

const UI_EN: &str = include_str!("../../../../../packages/shared/src/locales/en/ui.json");
const UI_FR: &str = include_str!("../../../../../packages/shared/src/locales/fr/ui.json");
const MAIN_EN: &str = include_str!("../../../../../packages/shared/src/locales/en/main.json");
const MAIN_FR: &str = include_str!("../../../../../packages/shared/src/locales/fr/main.json");

/// Language of the pages: `fr` for any French tag, English otherwise (the only two catalogs).
pub fn pick_lang(lang: &str) -> &'static str {
    let l = lang.trim().to_ascii_lowercase();
    if l == "fr" || l.starts_with("fr-") || l.starts_with("fr_") { "fr" } else { "en" }
}

struct Catalogs {
    ui: [Value; 2],
    main: [Value; 2],
}

fn catalogs() -> &'static Catalogs {
    static C: OnceLock<Catalogs> = OnceLock::new();
    C.get_or_init(|| {
        let parse = |s: &str| serde_json::from_str::<Value>(s).unwrap_or(Value::Null);
        Catalogs { ui: [parse(UI_EN), parse(UI_FR)], main: [parse(MAIN_EN), parse(MAIN_FR)] }
    })
}

fn index(lang: &str) -> usize {
    usize::from(pick_lang(lang) == "fr")
}

/// A string of ui.json (`notes.bold`), in `lang`, falling back to English.
pub fn ui(lang: &str, key: &str) -> Option<String> {
    lookup(&catalogs().ui, lang, key)
}

fn lookup(cat: &[Value; 2], lang: &str, key: &str) -> Option<String> {
    let get = |v: &Value| key.split('.').try_fold(v, |acc, k| acc.get(k)).and_then(Value::as_str).map(str::to_owned);
    get(&cat[index(lang)]).or_else(|| get(&cat[0]))
}

/// Flat `section.key -> text` map of the given ui.json sections plus single keys, for injection in a page.
pub fn ui_subset(lang: &str, sections: &[&str], keys: &[&str]) -> Map<String, Value> {
    let mut out = Map::new();
    let en = &catalogs().ui[0];
    for sec in sections {
        if let Some(Value::Object(obj)) = en.get(*sec) {
            for k in obj.keys() {
                let full = format!("{sec}.{k}");
                if let Some(text) = ui(lang, &full) {
                    out.insert(full, Value::String(text));
                }
            }
        }
    }
    for key in keys {
        if let Some(text) = ui(lang, key) {
            out.insert((*key).to_owned(), Value::String(text));
        }
    }
    out
}

/// The `jsonView` labels of main.json, in `lang` (English fallback per key).
pub fn json_view_labels(lang: &str) -> Map<String, Value> {
    let mut out = Map::new();
    if let Some(Value::Object(obj)) = catalogs().main[0].get("jsonView") {
        for k in obj.keys() {
            if let Some(text) = lookup(&catalogs().main, lang, &format!("jsonView.{k}")) {
                out.insert(k.clone(), Value::String(text));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_language() {
        assert_eq!(pick_lang("fr"), "fr");
        assert_eq!(pick_lang("fr-CA"), "fr");
        assert_eq!(pick_lang("en-GB"), "en");
        assert_eq!(pick_lang("de"), "en");
    }

    #[test]
    fn reads_ui_keys() {
        assert_eq!(ui("en", "notes.bold").as_deref(), Some("Bold"));
        assert_eq!(ui("fr", "common.cancel").as_deref(), Some("Annuler"));
        assert_eq!(ui("en", "notes.nope"), None);
    }

    #[test]
    fn subset_has_sections_and_keys() {
        let m = ui_subset("fr", &["notes", "easel"], &["common.cancel"]);
        assert_eq!(m.get("notes.untitled").and_then(Value::as_str), Some("Note sans titre"));
        assert!(m.contains_key("easel.empty"));
        assert!(m.contains_key("common.cancel"));
        assert!(!m.contains_key("common.ok"));
    }

    #[test]
    fn json_labels_translate() {
        assert_eq!(json_view_labels("fr").get("tree").and_then(Value::as_str), Some("Arbre"));
        assert_eq!(json_view_labels("en").get("results_other").and_then(Value::as_str), Some("{{count}} results"));
    }
}

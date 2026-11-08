//! Ports `packages/shared/src/i18n.ts`: i18next semantics over the JSON catalogs in
//! `packages/shared/src/locales/{en,fr}/{main,ui}.json` (nested keys with dots, `{{var}}` interpolation without
//! escaping, `_one`/`_other` plurals driven by `count`, OS context keys, fallback to English).

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Locale {
    #[default]
    En,
    Fr,
}

impl Locale {
    pub fn id(self) -> &'static str {
        match self {
            Locale::En => "en",
            Locale::Fr => "fr",
        }
    }
}

/// User choice: `system` follows the OS language.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum UiLanguage {
    #[default]
    System,
    En,
    Fr,
}

pub struct LocaleInfo {
    pub id: Locale,
    pub name: &'static str,
}

pub const LOCALES: [LocaleInfo; 2] = [LocaleInfo { id: Locale::En, name: "English" }, LocaleInfo { id: Locale::Fr, name: "Fran\u{e7}ais" }];

/// First supported system language (`fr-CA`, `FR_fr`, `fr_FR.UTF-8`...), otherwise English.
pub fn resolve_locale(pref: Option<UiLanguage>, system_languages: &[&str]) -> Locale {
    match pref {
        Some(UiLanguage::En) => return Locale::En,
        Some(UiLanguage::Fr) => return Locale::Fr,
        _ => {}
    }
    for tag in system_languages {
        let base = tag.to_lowercase();
        let base = base.split(['-', '_', '.', '@']).next().unwrap_or("");
        if let Some(l) = LOCALES.iter().find(|l| l.id.id() == base) {
            return l.id;
        }
    }
    Locale::En
}

/// Languages of the system, from the usual environment variables (`LC_ALL`, `LC_MESSAGES`, `LANGUAGE`, `LANG`).
/// The app can pass a better list (macOS preferred languages) to `resolve_locale` instead.
pub fn system_languages_from_env() -> Vec<String> {
    let mut out = Vec::new();
    for var in ["LC_ALL", "LC_MESSAGES", "LANGUAGE", "LANG"] {
        if let Ok(v) = std::env::var(var) {
            for part in v.split(':') {
                if !part.is_empty() && part != "C" && part != "POSIX" {
                    out.push(part.to_string());
                }
            }
        }
    }
    out
}

/// Operating system context of i18next keys (`name_windows`, `name_linux`); None on macOS.
pub fn os_context() -> Option<&'static str> {
    if cfg!(target_os = "macos") {
        None
    } else if cfg!(target_os = "windows") {
        Some("windows")
    } else {
        Some("linux")
    }
}

pub const IS_MAC: bool = cfg!(target_os = "macos");

fn modifier(c: char) -> &'static str {
    match c {
        '\u{2303}' | '\u{2318}' => "Ctrl",
        '\u{2325}' => "Alt",
        '\u{21e7}' => "Shift",
        _ => "",
    }
}

/// Shortcuts written Mac-style (Shift+Cmd+T glyphs) become Ctrl+Shift+T elsewhere. Unchanged on Mac.
/// Two Mac spellings that give the same thing elsewhere ("ctrl-shift-S or cmd-shift-S") become just one.
pub fn platform_keys(text: &str, mac: bool) -> String {
    const GLYPHS: &str = "\u{2303}\u{2325}\u{21e7}\u{2318}\u{21e5}";
    if mac || !text.chars().any(|c| GLYPHS.contains(c)) {
        return text.to_string();
    }
    static MODS: OnceLock<Regex> = OnceLock::new();
    static DUP: OnceLock<Regex> = OnceLock::new();
    let mods = MODS.get_or_init(|| Regex::new("([\u{2303}\u{2325}\u{21e7}\u{2318}]+)(\\S)").expect("static regex"));
    let dup = DUP.get_or_init(|| Regex::new(r"(Ctrl\+[^\s)]+) (?:ou|or) (Ctrl\+[^\s)]+)").expect("static regex"));
    let step = mods.replace_all(text, |c: &regex::Captures| {
        let set: Vec<&str> = c[1].chars().map(modifier).collect();
        let mut parts: Vec<&str> = ["Ctrl", "Alt", "Shift"].into_iter().filter(|m| set.contains(m)).collect();
        parts.push(&c[2]);
        parts.join("+")
    });
    let step = dup.replace_all(&step, |c: &regex::Captures| if c[1] == c[2] { c[1].to_string() } else { c[0].to_string() });
    step.replace('\u{21e5}', "Tab")
}

type Flat = HashMap<String, String>;

fn flatten(v: &serde_json::Value, prefix: &str, out: &mut Flat) {
    match v {
        serde_json::Value::Object(m) => {
            for (k, x) in m {
                let key = if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") };
                flatten(x, &key, out);
            }
        }
        serde_json::Value::String(s) => {
            out.insert(prefix.to_string(), s.clone());
        }
        _ => {}
    }
}

fn parse_flat(json: &str) -> Flat {
    let mut out = Flat::new();
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(json) {
        flatten(&v, "", &mut out);
    }
    out
}

/// Plural category (`one` or `other`) of `n` in `locale`. French treats 0 and 1 as singular.
fn plural(locale: Locale, n: f64) -> &'static str {
    match locale {
        Locale::En => {
            if n == 1.0 {
                "one"
            } else {
                "other"
            }
        }
        Locale::Fr => {
            if (0.0..2.0).contains(&n) {
                "one"
            } else {
                "other"
            }
        }
    }
}

/// An i18next-like translator over in-memory catalogs (one flat map per language).
#[derive(Debug, Clone)]
pub struct I18n {
    locale: Locale,
    resources: HashMap<Locale, Flat>,
}

impl I18n {
    /// `catalogs`: JSON text per language (one namespace). Missing keys fall back to English.
    pub fn new(locale: Locale, catalogs: &[(Locale, &str)]) -> Self {
        I18n { locale, resources: catalogs.iter().map(|(l, j)| (*l, parse_flat(j))).collect() }
    }

    pub fn set_locale(&mut self, locale: Locale) {
        self.locale = locale;
    }

    pub fn locale(&self) -> Locale {
        self.locale
    }

    pub fn has(&self, key: &str) -> bool {
        self.lookup(key, &[]).is_some()
    }

    fn find(&self, locale: Locale, key: &str, count: Option<f64>) -> Option<&String> {
        let res = self.resources.get(&locale)?;
        let ctx = os_context();
        let mut candidates: Vec<String> = Vec::new();
        if let Some(n) = count {
            let cat = plural(locale, n);
            if let Some(c) = ctx {
                candidates.push(format!("{key}_{c}_{cat}"));
            }
            candidates.push(format!("{key}_{cat}"));
            if cat != "other" {
                // i18next falls back to `_other` when the exact category is missing.
            }
            candidates.push(format!("{key}_other"));
        }
        if let Some(c) = ctx {
            candidates.push(format!("{key}_{c}"));
        }
        candidates.push(key.to_string());
        candidates.iter().find_map(|k| res.get(k))
    }

    fn lookup(&self, key: &str, args: &[(&str, &str)]) -> Option<String> {
        self.lookup_in(self.locale, key, args)
    }

    fn lookup_in(&self, locale: Locale, key: &str, args: &[(&str, &str)]) -> Option<String> {
        let count = args.iter().find(|(k, _)| *k == "count").and_then(|(_, v)| v.parse::<f64>().ok());
        let raw = self.find(locale, key, count).or_else(|| self.find(Locale::En, key, count))?;
        Some(interpolate(raw, args))
    }

    /// Translation of `key`, or the key itself when it exists nowhere (i18next behavior).
    pub fn t(&self, key: &str) -> String {
        self.t_args(key, &[])
    }

    pub fn t_args(&self, key: &str, args: &[(&str, &str)]) -> String {
        self.t_args_in(self.locale, key, args)
    }

    /// Same as `t_args` in an explicit language.
    pub fn t_args_in(&self, locale: Locale, key: &str, args: &[(&str, &str)]) -> String {
        let out = self.lookup_in(locale, key, args).unwrap_or_else(|| key.to_string());
        platform_keys(&out, IS_MAC)
    }
}

fn interpolate(text: &str, args: &[(&str, &str)]) -> String {
    static VAR: OnceLock<Regex> = OnceLock::new();
    let re = VAR.get_or_init(|| Regex::new(r"\{\{\s*([\w.]+)\s*\}\}").expect("static regex"));
    re.replace_all(text, |c: &regex::Captures| args.iter().find(|(k, _)| *k == &c[1]).map(|(_, v)| (*v).to_string()).unwrap_or_default()).into_owned()
}

/// Namespaces bundled in the binary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Namespace {
    /// Menus, dialogs, command bar, default names.
    Main,
    /// Interface views.
    Ui,
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

fn bundled() -> &'static (I18n, I18n) {
    static B: OnceLock<(I18n, I18n)> = OnceLock::new();
    B.get_or_init(|| {
        let main = I18n::new(
            Locale::En,
            &[
                (Locale::En, include_str!("../../../../../packages/shared/src/locales/en/main.json")),
                (Locale::Fr, include_str!("../../../../../packages/shared/src/locales/fr/main.json")),
            ],
        );
        let ui = I18n::new(
            Locale::En,
            &[
                (Locale::En, include_str!("../../../../../packages/shared/src/locales/en/ui.json")),
                (Locale::Fr, include_str!("../../../../../packages/shared/src/locales/fr/ui.json")),
            ],
        );
        (main, ui)
    })
}

/// Picks the language used by `t`, `t_args` and `tn` from now on.
pub fn set_locale(locale: Locale) {
    CURRENT.store(if locale == Locale::Fr { 1 } else { 0 }, Ordering::Relaxed);
}

pub fn current_locale() -> Locale {
    if CURRENT.load(Ordering::Relaxed) == 1 {
        Locale::Fr
    } else {
        Locale::En
    }
}

/// Resolves the preference against the system languages and applies it (`initLocale` of the Electron app).
pub fn init_locale(pref: Option<UiLanguage>, system_languages: &[&str]) -> Locale {
    let l = resolve_locale(pref, system_languages);
    set_locale(l);
    l
}

/// Translation in a given namespace, in the current language.
pub fn tn(ns: Namespace, key: &str, args: &[(&str, &str)]) -> String {
    let (main, ui) = bundled();
    let i = if ns == Namespace::Main { main } else { ui };
    i.t_args_in(current_locale(), key, args)
}

/// Translation of `key`: looked up in `main` first, then in `ui`; the key itself if absent from both.
pub fn t(key: &str) -> String {
    t_args(key, &[])
}

pub fn t_args(key: &str, args: &[(&str, &str)]) -> String {
    let (main, ui) = bundled();
    let loc = current_locale();
    for i in [main, ui] {
        if i.lookup_in(loc, key, args).is_some() {
            return i.t_args_in(loc, key, args);
        }
    }
    key.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_follows_choice() {
        assert_eq!(resolve_locale(Some(UiLanguage::Fr), &["en-US"]), Locale::Fr);
        assert_eq!(resolve_locale(Some(UiLanguage::En), &["fr-FR"]), Locale::En);
    }

    #[test]
    fn resolve_takes_first_supported() {
        assert_eq!(resolve_locale(Some(UiLanguage::System), &["de-DE", "fr-CA", "en"]), Locale::Fr);
        assert_eq!(resolve_locale(None, &["FR_fr"]), Locale::Fr);
        assert_eq!(resolve_locale(None, &["fr_FR.UTF-8"]), Locale::Fr);
    }

    #[test]
    fn resolve_falls_back_to_english() {
        assert_eq!(resolve_locale(Some(UiLanguage::System), &["de", "ja"]), Locale::En);
        assert_eq!(resolve_locale(Some(UiLanguage::System), &[]), Locale::En);
    }

    #[test]
    fn locales_list() {
        let ids: Vec<_> = LOCALES.iter().map(|l| l.id.id()).collect();
        assert_eq!(ids, ["en", "fr"]);
    }

    #[test]
    fn platform_keys_mac_and_plain() {
        assert_eq!(platform_keys("\u{21e7}\u{2318}T", true), "\u{21e7}\u{2318}T");
        assert_eq!(platform_keys("Ctrl+T", false), "Ctrl+T");
    }

    #[test]
    fn platform_keys_translates() {
        assert_eq!(platform_keys("\u{21e7}\u{2318}T", false), "Ctrl+Shift+T");
        assert_eq!(platform_keys("\u{2325}\u{2318}B", false), "Ctrl+Alt+B");
        assert_eq!(platform_keys("\u{2318},", false), "Ctrl+,");
        assert_eq!(platform_keys("Ouvrir (\u{2318}L)", false), "Ouvrir (Ctrl+L)");
        assert_eq!(platform_keys("\u{21e7}\u{2325}\u{2303}K", false), "Ctrl+Alt+Shift+K");
        assert_eq!(platform_keys("\u{2303}\u{21e7}S ou \u{2318}\u{21e7}S", false), "Ctrl+Shift+S");
        assert_eq!(platform_keys("\u{2303}\u{21e7}S or \u{2318}\u{21e7}S", false), "Ctrl+Shift+S");
        assert_eq!(platform_keys("\u{2303}\u{21e5}", false), "Ctrl+Tab");
    }

    const EN: &str = r#"{"hello":"Hello {{name}}","only":"Only EN","count_one":"{{count}} tab","count_other":"{{count}} tabs","a":{"b":"nested"}}"#;
    const FR: &str = r#"{"hello":"Bonjour {{name}}"}"#;

    #[test]
    fn interpolates_without_escaping() {
        let i = I18n::new(Locale::En, &[(Locale::En, EN), (Locale::Fr, FR)]);
        assert_eq!(i.t_args("hello", &[("name", "<b>")]), "Hello <b>");
        assert_eq!(i.t("a.b"), "nested");
    }

    #[test]
    fn falls_back_to_english() {
        let i = I18n::new(Locale::Fr, &[(Locale::En, EN), (Locale::Fr, FR)]);
        assert_eq!(i.t_args("hello", &[("name", "A")]), "Bonjour A");
        assert_eq!(i.t("only"), "Only EN");
        assert_eq!(i.t("nowhere"), "nowhere");
    }

    #[test]
    fn handles_plurals() {
        let i = I18n::new(Locale::En, &[(Locale::En, EN)]);
        assert_eq!(i.t_args("count", &[("count", "1")]), "1 tab");
        assert_eq!(i.t_args("count", &[("count", "3")]), "3 tabs");
    }

    #[test]
    fn bundled_catalogs_work() {
        // The global language is never changed in tests (they run in parallel); use the explicit-language API.
        let (main, _) = bundled();
        assert_eq!(main.t_args_in(Locale::En, "defaults.folder", &[]), "New folder");
        assert_ne!(main.t_args_in(Locale::Fr, "defaults.folder", &[]), "defaults.folder");
        assert_eq!(t("defaults.folder"), "New folder");
        assert_eq!(t_args("defaults.spaceNumbered", &[("number", "2")]), "Space 2");
    }
}

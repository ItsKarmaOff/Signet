//! Boosts (port of the data side of `main/features/boost/boost.ts`, `emoji.ts` and `zap.ts`): per-site CSS and
//! hidden elements, stored in `state.boosts`. Applying CSS to tabs and the edit window belong to the app.
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::util::now_ms;

/// Script of the "Hide" tool, run in an isolated world. It evaluates to a promise resolved with a CSS selector,
/// or `null` on Esc / `window.__signetZapStop()`.
pub const ZAP_SCRIPT: &str = include_str!("assets/zap.js");
/// Stops the tool in the page (no effect if it is not active).
pub const ZAP_STOP_SCRIPT: &str = r#"(typeof window.__signetZapStop === "function" && window.__signetZapStop(), null)"#;
/// Script that exempts emojis from the dark-mode inversion (run in an isolated world).
pub const EMOJI_SCRIPT: &str = include_str!("assets/emoji.js");
pub const EMOJI_STOP_SCRIPT: &str = r#"(typeof window.__signetEmoji === "function" && window.__signetEmoji(), null)"#;
/// Cancels the dark-mode inversion on emojis wrapped by `EMOJI_SCRIPT`.
pub const EMOJI_CSS: &str = "[data-signet-emoji] { filter: invert(1) hue-rotate(180deg) !important; }";
/// Isolated world ids used by the TS app.
pub const WORLD: i32 = 1001;
pub const EMOJI_WORLD: i32 = 1007;

/// True if the CSS contains the editor's "Dark mode" preset (`signet:dark` block).
pub fn has_dark_preset(css: &str) -> bool {
    css.contains("/* signet:dark */")
}

pub use signet_core::types::Boost;

/// Empty Boost for a site that has none yet (enabled, as the editor shows it).
pub fn draft(site: &str) -> Boost {
    Boost { site: site.into(), enabled: true, css: String::new(), zapped: vec![], updated_at: 0 }
}

/// Full CSS: hidden elements first, so that malformed free CSS does not swallow them.
pub fn css_of(b: &Boost) -> String {
    let hidden = b.zapped.iter().map(|s| format!("{s} {{ display: none !important; }}")).collect::<Vec<_>>().join("\n");
    let emoji = if has_dark_preset(&b.css) { format!("\n{EMOJI_CSS}") } else { String::new() };
    format!("{hidden}\n{}{emoji}", b.css)
}

/// CSS to insert in a page, or `None` when there is nothing (disabled or empty).
pub fn applied_css(b: &Boost) -> Option<String> {
    if !b.enabled {
        return None;
    }
    let css = css_of(b);
    (!css.trim().is_empty()).then_some(css)
}

/// The `boosts` map of the state (site to Boost).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Boosts(pub BTreeMap<String, Boost>);

impl Boosts {
    pub fn get(&self, site: &str) -> Option<&Boost> {
        self.0.get(site)
    }

    /// Editor state of a site: the Boost, or an empty draft.
    pub fn editor_boost(&self, site: &str) -> Boost {
        self.0.get(site).cloned().unwrap_or_else(|| draft(site))
    }

    fn edit(&mut self, site: &str, f: impl FnOnce(&mut Boost)) -> bool {
        if site.is_empty() {
            return false;
        }
        let mut b = self.0.get(site).cloned().unwrap_or_else(|| draft(site));
        f(&mut b);
        b.updated_at = now_ms();
        self.0.insert(site.into(), b);
        true
    }

    pub fn save(&mut self, site: &str, css: Option<&str>, enabled: Option<bool>) -> bool {
        self.edit(site, |b| {
            if let Some(c) = css { b.css = c.into(); }
            if let Some(e) = enabled { b.enabled = e; }
        })
    }

    /// Adds a hidden element selector (from the Hide tool).
    pub fn zap(&mut self, site: &str, selector: &str) -> bool {
        self.edit(site, |b| {
            if !b.zapped.iter().any(|s| s == selector) {
                b.zapped.push(selector.into());
            }
        })
    }

    pub fn unzap(&mut self, site: &str, selector: &str) -> bool {
        self.0.contains_key(site) && self.edit(site, |b| b.zapped.retain(|s| s != selector))
    }

    pub fn delete(&mut self, site: &str) -> bool {
        self.0.remove(site).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn css_puts_hidden_first() {
        let b = Boost { zapped: vec![".ad".into(), "#x > p".into()], css: "body{color:red}".into(), ..draft("a.com") };
        assert_eq!(css_of(&b), ".ad { display: none !important; }\n#x > p { display: none !important; }\nbody{color:red}");
        let dark = Boost { css: "/* signet:dark */\nhtml{filter:invert(1)}".into(), ..draft("a.com") };
        assert!(css_of(&dark).ends_with(EMOJI_CSS));
    }

    #[test]
    fn applied_css_rules() {
        assert_eq!(applied_css(&draft("a.com")), None);
        assert!(applied_css(&Boost { css: "a{}".into(), ..draft("a.com") }).is_some());
        assert_eq!(applied_css(&Boost { css: "a{}".into(), enabled: false, ..draft("a.com") }), None);
    }

    #[test]
    fn editing() {
        let mut all = Boosts::default();
        assert!(!all.save("", Some("a"), None));
        assert!(all.save("a.com", Some("a{}"), None));
        assert!(all.get("a.com").unwrap().enabled && all.get("a.com").unwrap().updated_at > 0);
        assert!(all.zap("a.com", ".x"));
        all.zap("a.com", ".x");
        assert_eq!(all.get("a.com").unwrap().zapped, vec![".x"]);
        assert!(!all.unzap("b.com", ".x"));
        assert!(all.unzap("a.com", ".x"));
        assert!(all.get("a.com").unwrap().zapped.is_empty());
        assert!(all.save("a.com", None, Some(false)));
        assert!(!all.get("a.com").unwrap().enabled);
        assert_eq!(all.editor_boost("zzz").site, "zzz");
        assert!(all.delete("a.com") && !all.delete("a.com"));
    }

    #[test]
    fn json_matches_electron() {
        let all: Boosts = serde_json::from_value(json!({"a.com": {"site":"a.com","enabled":true,"css":"x","zapped":[".a"],"updatedAt":5}})).unwrap();
        assert_eq!(serde_json::to_value(&all).unwrap(), json!({"a.com": {"site":"a.com","enabled":true,"css":"x","zapped":[".a"],"updatedAt":5}}));
    }

    #[test]
    fn scripts_are_present() {
        assert!(ZAP_SCRIPT.contains("__signetZapStop") && ZAP_SCRIPT.trim_start().starts_with("new Promise"));
        assert!(EMOJI_SCRIPT.contains("__signetEmoji"));
        assert!(!ZAP_SCRIPT.contains('`'));
    }
}

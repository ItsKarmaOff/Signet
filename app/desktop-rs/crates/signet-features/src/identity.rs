//! Identity announced to pages (port of `main/web/identity.ts`): reduced user agent and client hints.
use regex::Regex;
use serde_json::Value;
use std::sync::{Mutex, OnceLock};

fn re_app() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)\s(Electron|signet)/\S+").expect("static regex"))
}
fn re_chrome() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"Chrome/(\d+)\.[\d.]+").expect("static regex"))
}

/// `Chrome/156.0.8078.48` becomes `Chrome/156.0.0.0`, app and Electron tokens removed.
pub fn reduced_user_agent(ua: &str) -> String {
    let s = re_app().replace_all(ua, "");
    re_chrome().replace(&s, "Chrome/${1}.0.0.0").into_owned()
}

/// JS run once in an interface page to read the native client hints (result is JSON or null).
pub const READ_HINTS_JS: &str = r#"(async () => {
  const d = navigator.userAgentData
  if (!d) return null
  const h = await d.getHighEntropyValues(['architecture', 'bitness', 'model', 'platformVersion', 'fullVersionList', 'uaFullVersion', 'wow64'])
  return { brands: d.brands, fullVersionList: h.fullVersionList, fullVersion: h.uaFullVersion, platform: d.platform, platformVersion: h.platformVersion,
    architecture: h.architecture, model: h.model, mobile: d.mobile, bitness: h.bitness, wow64: h.wow64 }
})()"#;

/// Holds the native hints once captured.
#[derive(Default)]
pub struct Identity {
    native: Mutex<Option<Value>>,
}

impl Identity {
    pub fn new() -> Self {
        Self::default()
    }

    /// Stores the hints read with `READ_HINTS_JS`; true if accepted (only the first valid set counts).
    pub fn capture_native_hints(&self, hints: Value) -> bool {
        let ok = hints.get("brands").and_then(|b| b.as_array()).is_some_and(|b| !b.is_empty());
        let Ok(mut g) = self.native.lock() else { return false };
        if !ok || g.is_some() {
            return false;
        }
        *g = Some(hints);
        true
    }

    /// Parameters of `Emulation.setUserAgentOverride`: agent, languages, hints once known.
    pub fn default_identity(&self, user_agent: &str, accept_language: &str) -> Value {
        let mut v = serde_json::json!({ "userAgent": user_agent, "acceptLanguage": accept_language });
        if let Some(h) = self.native.lock().ok().and_then(|g| g.clone()) {
            v["userAgentMetadata"] = h;
        }
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strips_electron_and_reduces() {
        let ua = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) signet/0.1.0 Chrome/156.0.8078.48 Electron/42.0.0 Safari/537.36";
        assert_eq!(reduced_user_agent(ua), "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/156.0.0.0 Safari/537.36");
    }
    #[test]
    fn already_reduced() {
        assert_eq!(reduced_user_agent("X Chrome/120.0.0.0 Safari/537.36"), "X Chrome/120.0.0.0 Safari/537.36");
    }
    #[test]
    fn hints_captured_once() {
        let i = Identity::new();
        assert!(!i.capture_native_hints(serde_json::json!({"brands": []})));
        assert!(i.capture_native_hints(serde_json::json!({"brands": [{"brand":"x"}]})));
        assert!(!i.capture_native_hints(serde_json::json!({"brands": [{"brand":"y"}]})));
        assert!(i.default_identity("ua", "fr").get("userAgentMetadata").is_some());
    }
}

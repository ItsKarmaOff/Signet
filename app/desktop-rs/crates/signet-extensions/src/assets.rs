//! The page-side scripts (JavaScript files in `assets/`) and how to assemble them for a context.
//!
//! Bridge assumption: the web layer provides `window.__signetPost(channel, json)` (page to app) and calls
//! `window.__signetDispatch(channel, json)` in the page (app to page); `bridge.js` builds a promise based
//! request/response helper on top of the two.

pub const BRIDGE_JS: &str = include_str!("../assets/bridge.js");
pub const INSTALL_JS: &str = include_str!("../assets/extension-apis-install.js");
pub const EXTENSION_APIS_JS: &str = include_str!("../assets/extension-apis.js");
pub const WEBSTORE_JS: &str = include_str!("../assets/webstore.js");
pub const API_CATALOG_JSON: &str = crate::catalog::CATALOG_JSON;

/// Script for the Web Store pages (bridge + `chrome.webstorePrivate`).
pub fn webstore_script() -> String {
    format!("{BRIDGE_JS}\n;\n{WEBSTORE_JS}\n")
}

/// Script for an extension context: the config object, the bridge, the installer and the entry point.
/// `config` is built by `ExtensionHost::page_script`.
pub fn context_script(config: &serde_json::Value) -> String {
    format!("globalThis.__signetExtensionConfig = {config};\n{BRIDGE_JS}\n;\n{INSTALL_JS}\n;\n{EXTENSION_APIS_JS}\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripts_are_ascii_dash_free_and_assemble() {
        for (name, s) in [("bridge", BRIDGE_JS), ("install", INSTALL_JS), ("entry", EXTENSION_APIS_JS), ("webstore", WEBSTORE_JS), ("catalog", API_CATALOG_JSON)] {
            assert!(!s.contains('\u{2014}') && !s.contains('\u{2013}'), "{name} has a long dash");
        }
        let s = context_script(&serde_json::json!({ "id": "x" }));
        assert!(s.starts_with("globalThis.__signetExtensionConfig = {\"id\":\"x\"};"));
        assert!(s.contains("__signetInstallApis") && s.contains("__signetCrx"));
        assert!(webstore_script().contains("webstorePrivate"));
    }
}

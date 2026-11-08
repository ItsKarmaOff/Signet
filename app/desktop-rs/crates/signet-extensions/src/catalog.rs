//! Catalog of the Chrome extension API namespaces (names only). Port of `shared/extension-api-catalog.ts`.
//! The data lives in `assets/api-catalog.json` (generated from the TypeScript catalog); the page-side installer reads
//! the same JSON.

use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::OnceLock;

pub const CATALOG_JSON: &str = include_str!("../assets/api-catalog.json");

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct NamespaceSpec {
    /// Manifest permissions that unlock the namespace (any of them). Empty = always available.
    #[serde(default)]
    pub permissions: Vec<String>,
    /// Alternative unlock: a manifest key such as `side_panel`, `omnibox`, `commands`, `action`.
    #[serde(default)]
    pub manifest_key: Option<String>,
    #[serde(default)]
    pub methods: Vec<String>,
    /// Methods that resolve a neutral value instead of failing when stubbed.
    #[serde(default)]
    pub soft: BTreeMap<String, Value>,
    #[serde(default)]
    pub events: Vec<String>,
    #[serde(default)]
    pub declarative_events: Vec<String>,
    #[serde(default)]
    pub constants: BTreeMap<String, Value>,
    #[serde(default)]
    pub classes: Vec<String>,
    #[serde(default)]
    pub children: BTreeMap<String, NamespaceSpec>,
    /// Members whose native implementation kills the process in Electron; stubbed with inert placeholders.
    #[serde(default)]
    pub crashes: Vec<String>,
    /// Signet will not provide this namespace.
    #[serde(default)]
    pub unsupported: bool,
    /// Only exists in content scripts, never stub it from the extension side.
    #[serde(default)]
    pub content_script_only: bool,
}

pub type Catalog = BTreeMap<String, NamespaceSpec>;

/// Namespaces in the Chrome reference with no member to provide.
pub const TYPE_ONLY_NAMESPACES: [&str; 3] = ["events", "types", "extensionTypes"];

/// The catalog, parsed once.
pub fn catalog() -> &'static Catalog {
    static CATALOG: OnceLock<Catalog> = OnceLock::new();
    CATALOG.get_or_init(|| serde_json::from_str(CATALOG_JSON).unwrap_or_default())
}

/// Every namespace path, children included (`system.cpu`, `storage.session`...).
pub fn catalog_paths(catalog: &Catalog, prefix: &str) -> Vec<String> {
    let mut out = Vec::new();
    for (name, spec) in catalog {
        out.push(format!("{prefix}{name}"));
        out.extend(catalog_paths(&spec.children, &format!("{prefix}{name}.")));
    }
    out
}

/// Every manifest permission named by the catalog, sorted.
pub fn catalog_permissions(catalog: &Catalog) -> Vec<String> {
    fn walk(c: &Catalog, out: &mut std::collections::BTreeSet<String>) {
        for spec in c.values() {
            out.extend(spec.permissions.iter().cloned());
            walk(&spec.children, out);
        }
    }
    let mut set = std::collections::BTreeSet::new();
    walk(catalog, &mut set);
    set.into_iter().collect()
}

/// The spec at a dotted path.
pub fn spec_at<'a>(catalog: &'a Catalog, path: &str) -> Option<&'a NamespaceSpec> {
    let mut parts = path.split('.');
    let mut spec = catalog.get(parts.next()?)?;
    for p in parts {
        spec = spec.children.get(p)?;
    }
    Some(spec)
}

#[cfg(test)]
mod tests {
    use super::*;

    const INVENTORY: &[&str] = &[
        "action", "alarms", "commands", "contextMenus", "cookies", "declarativeNetRequest", "downloads", "i18n", "notifications", "offscreen",
        "permissions", "privacy", "proxy", "runtime", "scripting", "storage.local", "storage.session", "storage.sync", "storage.managed", "tabs",
        "webNavigation", "webRequest", "windows", "bookmarks", "browsingData", "contentSettings", "declarativeContent", "desktopCapture",
        "devtools.inspectedWindow", "devtools.network", "devtools.panels", "devtools.performance", "devtools.recorder", "dom", "extension",
        "fontSettings", "history", "identity", "idle", "management", "omnibox", "pageCapture", "power", "search", "sessions", "sidePanel",
        "system.cpu", "system.memory", "system.display", "system.storage", "tabCapture", "tabGroups", "topSites", "tts", "userScripts",
        "accessibilityFeatures", "audio", "browserAction", "pageAction", "certificateProvider", "debugger", "dns", "documentScan",
        "enterprise.deviceAttributes", "enterprise.hardwarePlatform", "enterprise.login", "enterprise.networkingAttributes",
        "enterprise.platformKeys", "events", "types", "extensionTypes", "fileBrowserHandler", "fileSystemProvider", "gcm", "instanceID",
        "input.ime", "loginState", "mimeHandler", "platformKeys", "printerProvider", "printing", "printingMetrics", "processes", "publicSuffix",
        "readingList", "systemLog", "ttsEngine", "vpnProvider", "wallpaper", "webAuthenticationProxy", "clipboard",
    ];

    fn every_spec(c: &Catalog) -> Vec<&NamespaceSpec> {
        c.values().flat_map(|s| std::iter::once(s).chain(every_spec(&s.children))).collect()
    }

    fn is_names(list: &[String]) -> bool {
        list.iter().all(|x| {
            let mut ch = x.chars();
            ch.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == '$') && ch.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
        })
    }

    #[test]
    fn covers_every_namespace_of_the_inventory() {
        let paths = catalog_paths(catalog(), "");
        let missing: Vec<_> = INVENTORY.iter().filter(|n| !paths.iter().any(|p| p == *n) && !TYPE_ONLY_NAMESPACES.contains(n)).collect();
        assert!(missing.is_empty(), "{missing:?}");
    }

    #[test]
    fn has_no_duplicate_path_and_no_type_only_namespace() {
        let paths = catalog_paths(catalog(), "");
        let set: std::collections::HashSet<_> = paths.iter().collect();
        assert_eq!(set.len(), paths.len());
        for n in TYPE_ONLY_NAMESPACES {
            assert!(!paths.iter().any(|p| p == n));
        }
    }

    #[test]
    fn has_valid_shapes() {
        for path in catalog_paths(catalog(), "") {
            let spec = spec_at(catalog(), &path).unwrap();
            assert!(spec.permissions.iter().all(|p| !p.is_empty()), "{path}");
            assert!(is_names(&spec.methods), "{path} methods");
            assert!(is_names(&spec.events), "{path} events");
            for e in &spec.declarative_events {
                assert!(spec.events.contains(e), "{path} declarative {e}");
            }
            assert!(is_names(&spec.classes), "{path} classes");
            for m in spec.soft.keys() {
                assert!(spec.methods.contains(m), "{path} soft {m}");
            }
            for m in &spec.crashes {
                assert!(spec.methods.contains(m) || spec.events.contains(m), "{path} crashes {m}");
            }
            let m: std::collections::HashSet<_> = spec.methods.iter().collect();
            let e: std::collections::HashSet<_> = spec.events.iter().collect();
            assert_eq!(m.len(), spec.methods.len(), "{path} methods unique");
            assert_eq!(e.len(), spec.events.len(), "{path} events unique");
        }
    }

    #[test]
    fn is_plain_json_data() {
        let v: Value = serde_json::from_str(CATALOG_JSON).unwrap();
        assert!(v.is_object());
    }

    #[test]
    fn lists_the_native_crashers() {
        let c = catalog();
        let storage = &c["system"].children["storage"];
        assert!(storage.crashes.contains(&"onAttached".to_string()) && storage.crashes.contains(&"onDetached".to_string()));
        assert!(c["printerProvider"].crashes.contains(&"onGetUsbPrinterInfoRequested".to_string()));
        assert!(c["system"].children["cpu"].crashes.is_empty());
    }

    #[test]
    fn flags_only_the_namespaces_signet_never_provides() {
        let flagged: Vec<&str> = catalog().iter().filter(|(_, s)| s.unsupported).map(|(n, _)| n.as_str()).collect();
        for n in ["gcm", "instanceID", "enterprise", "tabCapture", "debugger", "mimeHandler", "readingList", "ttsEngine"] {
            assert!(flagged.contains(&n), "{n}");
        }
        for n in ["tabs", "storage", "downloads", "sidePanel", "identity", "bookmarks"] {
            assert!(!flagged.contains(&n), "{n}");
        }
    }

    #[test]
    fn every_supported_namespace_has_a_member() {
        let empty = ["publicSuffix", "system", "devtools", "storage", "privacy", "proxy", "contentSettings"];
        for (n, s) in catalog() {
            if s.unsupported || empty.contains(&n.as_str()) {
                continue;
            }
            assert!(s.methods.len() + s.events.len() + s.classes.len() > 0, "{n}");
        }
    }

    #[test]
    fn is_fully_requested_by_the_probe_manifest_when_available() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../desktop/tests/extensions/api-probe/manifest.json");
        let Ok(text) = std::fs::read_to_string(path) else { return };
        let m: Value = serde_json::from_str(&text).unwrap();
        let requested: std::collections::HashSet<String> = ["permissions", "optional_permissions"]
            .iter()
            .flat_map(|k| m[k].as_array().cloned().unwrap_or_default())
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
        let missing: Vec<_> = catalog_permissions(catalog()).into_iter().filter(|p| !requested.contains(p)).collect();
        assert!(missing.is_empty(), "{missing:?}");
        assert!(every_spec(catalog()).len() > 60);
    }
}

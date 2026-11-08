//! Extension manifest parsing and validation (MV2 and MV3). New code: Chrome's manifest rules for what Signet uses.
//!
//! [`Manifest::parse`] accepts what Chrome accepts (a UTF-8 BOM, `//` and `/* */` comments), checks the required keys and
//! the shape of the ones Signet reads, and keeps the raw JSON for everything else. Problems that do not stop an extension
//! from loading are [`Manifest::warnings`]; anything that Chrome refuses is a [`ManifestError`].

use crate::permissions::declared_of;
use crate::urlmatch::is_valid_pattern;
use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct ManifestError(pub String);

fn fail<T>(msg: impl Into<String>) -> Result<T, ManifestError> {
    Err(ManifestError(msg.into()))
}

/// Removes `//` and `/* */` comments outside strings, and a leading BOM.
pub fn strip_json_comments(text: &str) -> String {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut in_str = false;
    while let Some(c) = chars.next() {
        if in_str {
            out.push(c);
            if c == '\\' {
                if let Some(n) = chars.next() {
                    out.push(n);
                }
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '"' => {
                in_str = true;
                out.push(c);
            }
            '/' if chars.peek() == Some(&'/') => {
                for n in chars.by_ref() {
                    if n == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut prev = '\0';
                for n in chars.by_ref() {
                    if prev == '*' && n == '/' {
                        break;
                    }
                    prev = n;
                }
            }
            _ => out.push(c),
        }
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunAt {
    DocumentStart,
    DocumentEnd,
    DocumentIdle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum World {
    Isolated,
    Main,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ContentScript {
    pub matches: Vec<String>,
    pub exclude_matches: Vec<String>,
    pub include_globs: Vec<String>,
    pub exclude_globs: Vec<String>,
    pub js: Vec<String>,
    pub css: Vec<String>,
    pub run_at: RunAt,
    pub all_frames: bool,
    pub match_about_blank: bool,
    pub match_origin_as_fallback: bool,
    pub world: World,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ActionSpec {
    pub default_popup: Option<String>,
    pub default_title: Option<String>,
    /// Icon path, or size to path.
    pub default_icon: Vec<(u32, String)>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Background {
    pub service_worker: Option<String>,
    /// `module` for an ES module worker.
    pub module: bool,
    pub page: Option<String>,
    pub scripts: Vec<String>,
    pub persistent: Option<bool>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Manifest {
    pub manifest_version: u8,
    pub name: String,
    pub version: String,
    pub version_name: Option<String>,
    pub description: String,
    pub default_locale: Option<String>,
    pub permissions: Vec<String>,
    pub optional_permissions: Vec<String>,
    pub host_permissions: Vec<String>,
    pub optional_host_permissions: Vec<String>,
    pub content_scripts: Vec<ContentScript>,
    pub background: Option<Background>,
    /// `action` (MV3), `browser_action` or `page_action` (MV2).
    pub action: Option<ActionSpec>,
    pub icons: Vec<(u32, String)>,
    pub options_page: Option<String>,
    pub key: Option<String>,
    pub minimum_chrome_version: Option<String>,
    pub warnings: Vec<String>,
    pub raw: Value,
}

fn str_list(v: &Value) -> Vec<String> {
    v.as_array().map(|a| a.iter().filter_map(|s| s.as_str().map(String::from)).collect()).unwrap_or_default()
}

/// Chrome version string: 1 to 4 dot-separated integers, each in 0..=65535, no leading zeros.
pub fn is_valid_version(v: &str) -> bool {
    let parts: Vec<&str> = v.split('.').collect();
    (1..=4).contains(&parts.len()) && parts.iter().all(|p| !p.is_empty() && p.len() <= 5 && p.chars().all(|c| c.is_ascii_digit()) && (p.len() == 1 || !p.starts_with('0')) && p.parse::<u32>().is_ok_and(|n| n <= 65535))
}

/// Compares two Chrome versions (missing parts are 0).
pub fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    let parse = |v: &str| -> Vec<u32> { v.split('.').map(|p| p.parse().unwrap_or(0)).collect() };
    let (pa, pb) = (parse(a), parse(b));
    for i in 0..pa.len().max(pb.len()) {
        let o = pa.get(i).unwrap_or(&0).cmp(pb.get(i).unwrap_or(&0));
        if o != std::cmp::Ordering::Equal {
            return o;
        }
    }
    std::cmp::Ordering::Equal
}

fn icon_map(v: Option<&Value>) -> Vec<(u32, String)> {
    let mut out: Vec<(u32, String)> = match v {
        Some(Value::Object(o)) => o.iter().filter_map(|(k, p)| Some((k.parse().ok()?, p.as_str()?.to_string()))).collect(),
        Some(Value::String(s)) => vec![(0, s.clone())],
        _ => vec![],
    };
    out.sort();
    out
}

fn parse_action(v: &Value) -> ActionSpec {
    ActionSpec {
        default_popup: v.get("default_popup").and_then(|p| p.as_str()).filter(|p| !p.is_empty()).map(String::from),
        default_title: v.get("default_title").and_then(|p| p.as_str()).map(String::from),
        default_icon: icon_map(v.get("default_icon")),
    }
}

fn parse_content_script(i: usize, v: &Value, warnings: &mut Vec<String>) -> Result<ContentScript, ManifestError> {
    let matches = str_list(v.get("matches").unwrap_or(&Value::Null));
    if matches.is_empty() {
        return fail(format!("Invalid value for 'content_scripts[{i}].matches': at least one match pattern is required."));
    }
    for p in matches.iter().chain(str_list(v.get("exclude_matches").unwrap_or(&Value::Null)).iter()) {
        if !is_valid_pattern(p) {
            return fail(format!("Invalid value for 'content_scripts[{i}].matches': invalid match pattern \"{p}\"."));
        }
    }
    let js = str_list(v.get("js").unwrap_or(&Value::Null));
    let css = str_list(v.get("css").unwrap_or(&Value::Null));
    if js.is_empty() && css.is_empty() {
        return fail(format!("Invalid value for 'content_scripts[{i}]': at least one js or css file is required."));
    }
    let run_at = match v.get("run_at").and_then(|r| r.as_str()) {
        None | Some("document_idle") => RunAt::DocumentIdle,
        Some("document_start") => RunAt::DocumentStart,
        Some("document_end") => RunAt::DocumentEnd,
        Some(other) => {
            warnings.push(format!("content_scripts[{i}].run_at: unknown value \"{other}\", using document_idle."));
            RunAt::DocumentIdle
        }
    };
    let world = match v.get("world").and_then(|w| w.as_str()) {
        Some("MAIN") => World::Main,
        _ => World::Isolated,
    };
    Ok(ContentScript {
        matches,
        exclude_matches: str_list(v.get("exclude_matches").unwrap_or(&Value::Null)),
        include_globs: str_list(v.get("include_globs").unwrap_or(&Value::Null)),
        exclude_globs: str_list(v.get("exclude_globs").unwrap_or(&Value::Null)),
        js,
        css,
        run_at,
        all_frames: v.get("all_frames").and_then(|b| b.as_bool()) == Some(true),
        match_about_blank: v.get("match_about_blank").and_then(|b| b.as_bool()) == Some(true),
        match_origin_as_fallback: v.get("match_origin_as_fallback").and_then(|b| b.as_bool()) == Some(true),
        world,
    })
}

impl Manifest {
    /// Parses and validates the text of a `manifest.json`.
    pub fn parse(text: &str) -> Result<Manifest, ManifestError> {
        let value: Value = serde_json::from_str(&strip_json_comments(text)).map_err(|e| ManifestError(format!("Manifest is not valid JSON: {e}")))?;
        Manifest::from_value(value)
    }

    pub fn from_value(raw: Value) -> Result<Manifest, ManifestError> {
        let Some(obj) = raw.as_object() else { return fail("Manifest is not an object.") };
        let mut warnings = Vec::new();
        let manifest_version = match obj.get("manifest_version").and_then(|m| m.as_u64()) {
            Some(2) => 2,
            Some(3) => 3,
            Some(1) => return fail("Manifest version 1 is not supported."),
            None => return fail("Required value 'manifest_version' is missing or invalid. Only manifest version 2 and 3 are supported."),
            Some(_) => return fail("Only manifest version 2 and 3 are supported."),
        };
        let name = obj.get("name").and_then(|n| n.as_str()).filter(|n| !n.trim().is_empty());
        let Some(name) = name else { return fail("Required value 'name' is missing or invalid.") };
        let version = obj.get("version").and_then(|n| n.as_str());
        let Some(version) = version.filter(|v| is_valid_version(v)) else { return fail("Required value 'version' is missing or invalid. It must be between 1-4 dot-separated integers each between 0 and 65536.") };

        let declared = declared_of(&raw);
        let mut permissions = str_list(obj.get("permissions").unwrap_or(&Value::Null));
        let mut optional_permissions = str_list(obj.get("optional_permissions").unwrap_or(&Value::Null));
        let host_permissions = str_list(obj.get("host_permissions").unwrap_or(&Value::Null));
        if manifest_version == 2 {
            // MV2 lists host patterns inside `permissions`.
            permissions.retain(|p| !(p == "<all_urls>" || p.contains("://")));
            optional_permissions.retain(|p| !(p == "<all_urls>" || p.contains("://")));
        } else if !host_permissions.is_empty() || obj.get("permissions").and_then(|p| p.as_array()).is_some_and(|l| l.iter().any(|v| v.as_str().is_some_and(|s| s.contains("://")))) {
            if obj.get("permissions").and_then(|p| p.as_array()).is_some_and(|l| l.iter().any(|v| v.as_str().is_some_and(|s| s.contains("://") || s == "<all_urls>"))) {
                warnings.push("Hosts must be in 'host_permissions' in manifest version 3.".into());
            }
        }
        for h in declared.required.origins.iter().chain(declared.optional.origins.iter()) {
            if !is_valid_pattern(h) {
                warnings.push(format!("Permission '{h}' is not a valid match pattern."));
            }
        }

        let mut content_scripts = Vec::new();
        if let Some(cs) = obj.get("content_scripts") {
            let Some(list) = cs.as_array() else { return fail("Invalid value for 'content_scripts'.") };
            for (i, s) in list.iter().enumerate() {
                content_scripts.push(parse_content_script(i, s, &mut warnings)?);
            }
        }

        let background = obj.get("background").and_then(|b| b.as_object()).map(|b| Background {
            service_worker: b.get("service_worker").and_then(|s| s.as_str()).map(String::from),
            module: b.get("type").and_then(|t| t.as_str()) == Some("module"),
            page: b.get("page").and_then(|s| s.as_str()).map(String::from),
            scripts: str_list(b.get("scripts").unwrap_or(&Value::Null)),
            persistent: b.get("persistent").and_then(|p| p.as_bool()),
        });
        if let Some(b) = &background {
            if manifest_version == 3 && (b.page.is_some() || (!b.scripts.is_empty() && b.service_worker.is_none())) {
                warnings.push("MV3 background pages are not supported by Chrome: use 'service_worker'.".into());
            }
            if b.service_worker.is_some() && !b.scripts.is_empty() {
                warnings.push("'background.scripts' is ignored when 'service_worker' is set.".into());
            }
        }

        let action_key = if manifest_version == 3 { "action" } else { "browser_action" };
        let action = obj.get(action_key).or_else(|| if manifest_version == 2 { obj.get("page_action") } else { None }).filter(|a| a.is_object()).map(parse_action);
        let options_page = obj
            .get("options_ui")
            .and_then(|o| o.get("page"))
            .and_then(|p| p.as_str())
            .or_else(|| obj.get("options_page").and_then(|p| p.as_str()))
            .map(String::from);

        if let Some(v) = obj.get("minimum_chrome_version").and_then(|v| v.as_str()) {
            if !is_valid_version(v) {
                warnings.push("'minimum_chrome_version' is not a valid version.".into());
            }
        }

        Ok(Manifest {
            manifest_version,
            name: name.to_string(),
            version: version.to_string(),
            version_name: obj.get("version_name").and_then(|v| v.as_str()).map(String::from),
            description: obj.get("description").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            default_locale: obj.get("default_locale").and_then(|v| v.as_str()).map(String::from),
            permissions,
            optional_permissions,
            host_permissions: declared.required.origins.clone(),
            optional_host_permissions: declared.optional.origins.clone(),
            content_scripts,
            background,
            action,
            icons: icon_map(obj.get("icons")),
            options_page,
            key: obj.get("key").and_then(|v| v.as_str()).map(String::from),
            minimum_chrome_version: obj.get("minimum_chrome_version").and_then(|v| v.as_str()).map(String::from),
            warnings,
            raw,
        })
    }

    pub fn has_action(&self) -> bool {
        self.action.is_some()
    }

    /// Largest icon of the manifest (`icons`, else the action's default icon), as a relative path.
    pub fn best_icon(&self) -> Option<&str> {
        self.icons.iter().chain(self.action.iter().flat_map(|a| a.default_icon.iter())).max_by_key(|(size, _)| *size).map(|(_, p)| p.as_str())
    }

    /// Popup page of the toolbar button, if any.
    pub fn popup(&self) -> Option<&str> {
        self.action.as_ref().and_then(|a| a.default_popup.as_deref())
    }

    /// Every permission of the manifest as the permission model sees it.
    pub fn declared(&self) -> crate::permissions::Declared {
        declared_of(&self.raw)
    }

    /// Has the extension a service worker / background page / background scripts to run?
    pub fn background_entry(&self) -> BackgroundEntry {
        match &self.background {
            Some(b) if b.service_worker.is_some() => BackgroundEntry::ServiceWorker { path: b.service_worker.clone().unwrap_or_default(), module: b.module },
            Some(b) if b.page.is_some() => BackgroundEntry::Page(b.page.clone().unwrap_or_default()),
            Some(b) if !b.scripts.is_empty() => BackgroundEntry::Scripts(b.scripts.clone()),
            _ => BackgroundEntry::None,
        }
    }

    /// `web_accessible_resources` as `(resources, matches)` groups: MV2 lists strings (any page), MV3 lists objects.
    pub fn web_accessible_resources(&self) -> Vec<(Vec<String>, Vec<String>)> {
        match self.raw.get("web_accessible_resources").and_then(|w| w.as_array()) {
            Some(list) if self.manifest_version == 2 => vec![(list.iter().filter_map(|s| s.as_str().map(String::from)).collect(), vec!["<all_urls>".into()])],
            Some(list) => list
                .iter()
                .filter_map(|o| Some((str_list(o.get("resources")?), str_list(o.get("matches").unwrap_or(&Value::Null)))))
                .collect(),
            None => vec![],
        }
    }

    /// Permissions and hosts as a JSON summary (Settings, install dialog).
    pub fn summary(&self) -> Value {
        json!({ "name": self.name, "version": self.version, "manifestVersion": self.manifest_version, "permissions": self.permissions, "hostPermissions": self.host_permissions })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackgroundEntry {
    None,
    ServiceWorker { path: String, module: bool },
    Page(String),
    Scripts(Vec<String>),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mv3() -> Value {
        json!({
            "manifest_version": 3, "name": "Test", "version": "1.2.3", "description": "d",
            "permissions": ["storage", "alarms"], "host_permissions": ["https://*.example.com/*"],
            "optional_permissions": ["cookies"],
            "background": { "service_worker": "sw.js", "type": "module" },
            "action": { "default_popup": "popup.html", "default_icon": { "16": "i16.png", "48": "i48.png" } },
            "icons": { "16": "i16.png", "128": "i128.png" },
            "content_scripts": [{ "matches": ["https://*/*"], "js": ["c.js"], "run_at": "document_start", "all_frames": true }],
            "options_ui": { "page": "options.html" }
        })
    }

    #[test]
    fn parses_a_complete_mv3_manifest() {
        let m = Manifest::from_value(mv3()).unwrap();
        assert_eq!((m.manifest_version, m.name.as_str(), m.version.as_str()), (3, "Test", "1.2.3"));
        assert_eq!(m.host_permissions, ["https://*.example.com/*"]);
        assert_eq!(m.permissions, ["storage", "alarms"]);
        assert_eq!(m.best_icon(), Some("i128.png"));
        assert_eq!(m.popup(), Some("popup.html"));
        assert_eq!(m.options_page.as_deref(), Some("options.html"));
        assert_eq!(m.content_scripts[0].run_at, RunAt::DocumentStart);
        assert!(m.content_scripts[0].all_frames);
        assert_eq!(m.background_entry(), BackgroundEntry::ServiceWorker { path: "sw.js".into(), module: true });
        assert!(m.has_action() && m.warnings.is_empty());
    }

    #[test]
    fn mv2_hosts_in_permissions_are_split_out() {
        let m = Manifest::from_value(json!({
            "manifest_version": 2, "name": "Old", "version": "0.1",
            "permissions": ["tabs", "https://a.example/*", "<all_urls>"],
            "browser_action": { "default_title": "T" },
            "background": { "scripts": ["a.js", "b.js"], "persistent": false },
            "web_accessible_resources": ["img/*"]
        }))
        .unwrap();
        assert_eq!(m.permissions, ["tabs"]);
        assert_eq!(m.host_permissions, ["https://a.example/*", "<all_urls>"]);
        assert_eq!(m.action.as_ref().unwrap().default_title.as_deref(), Some("T"));
        assert_eq!(m.background_entry(), BackgroundEntry::Scripts(vec!["a.js".into(), "b.js".into()]));
        assert_eq!(m.web_accessible_resources(), vec![(vec!["img/*".to_string()], vec!["<all_urls>".to_string()])]);
    }

    #[test]
    fn mv3_web_accessible_resources_are_groups() {
        let mut v = mv3();
        v["web_accessible_resources"] = json!([{ "resources": ["a.png"], "matches": ["https://x/*"] }]);
        assert_eq!(Manifest::from_value(v).unwrap().web_accessible_resources(), vec![(vec!["a.png".to_string()], vec!["https://x/*".to_string()])]);
    }

    #[test]
    fn required_keys_and_versions_are_checked() {
        let bad = |v: Value| Manifest::from_value(v).unwrap_err().0;
        assert!(bad(json!({ "name": "x", "version": "1" })).contains("manifest_version"));
        assert!(bad(json!({ "manifest_version": 1, "name": "x", "version": "1" })).contains("not supported"));
        assert!(bad(json!({ "manifest_version": 4, "name": "x", "version": "1" })).contains("2 and 3"));
        assert!(bad(json!({ "manifest_version": 3, "version": "1" })).contains("'name'"));
        assert!(bad(json!({ "manifest_version": 3, "name": "x" })).contains("'version'"));
        assert!(bad(json!({ "manifest_version": 3, "name": "x", "version": "1.2.3.4.5" })).contains("'version'"));
        assert!(bad(json!({ "manifest_version": 3, "name": "x", "version": "01.2" })).contains("'version'"));
        assert!(bad(json!([])).contains("not an object"));
    }

    #[test]
    fn version_helpers() {
        assert!(is_valid_version("1") && is_valid_version("65535.0.1.2") && !is_valid_version("65536") && !is_valid_version("1..2") && !is_valid_version(""));
        use std::cmp::Ordering::*;
        assert_eq!(compare_versions("1.2", "1.2.0"), Equal);
        assert_eq!(compare_versions("1.10", "1.9"), Greater);
        assert_eq!(compare_versions("0.9.9.9", "1"), Less);
    }

    #[test]
    fn content_scripts_are_validated() {
        let mut v = mv3();
        v["content_scripts"] = json!([{ "matches": [], "js": ["c.js"] }]);
        assert!(Manifest::from_value(v).unwrap_err().0.contains("matches"));
        let mut v = mv3();
        v["content_scripts"] = json!([{ "matches": ["nonsense"], "js": ["c.js"] }]);
        assert!(Manifest::from_value(v).unwrap_err().0.contains("invalid match pattern"));
        let mut v = mv3();
        v["content_scripts"] = json!([{ "matches": ["https://a/*"] }]);
        assert!(Manifest::from_value(v).unwrap_err().0.contains("js or css"));
        let mut v = mv3();
        v["content_scripts"] = json!([{ "matches": ["https://a/*"], "css": ["c.css"], "run_at": "later", "world": "MAIN" }]);
        let m = Manifest::from_value(v).unwrap();
        assert_eq!((m.content_scripts[0].run_at, m.content_scripts[0].world), (RunAt::DocumentIdle, World::Main));
        assert_eq!(m.warnings.len(), 1);
    }

    #[test]
    fn warnings_do_not_stop_loading() {
        let mut v = mv3();
        v["permissions"] = json!(["storage", "https://a/*"]);
        v["host_permissions"] = json!(["bad pattern"]);
        v["background"] = json!({ "service_worker": "sw.js", "scripts": ["x.js"] });
        let m = Manifest::from_value(v).unwrap();
        assert!(m.warnings.iter().any(|w| w.contains("host_permissions")));
        assert!(m.warnings.iter().any(|w| w.contains("not a valid match pattern")));
        assert!(m.warnings.iter().any(|w| w.contains("ignored")));
    }

    #[test]
    fn comments_and_bom_are_accepted() {
        let text = "\u{feff}{\n // a comment\n \"manifest_version\": 3, /* block */ \"name\": \"http://x // not a comment\", \"version\": \"1.0\"\n}";
        let m = Manifest::parse(text).unwrap();
        assert_eq!(m.name, "http://x // not a comment");
        assert!(Manifest::parse("{ nope").unwrap_err().0.contains("not valid JSON"));
    }

    #[test]
    fn declared_permissions_summary() {
        let m = Manifest::from_value(mv3()).unwrap();
        let d = m.declared();
        assert_eq!(d.optional.permissions, ["cookies"]);
        assert_eq!(m.summary()["manifestVersion"], 3);
    }
}

//! Interface mods (port of `main/features/mods/mods.ts`): user CSS sheets applied to the app's own views.
//! File: `<data>/mods.json` (`{ version: 1, disabled, mods: [{ id, name, css, enabled }] }`).
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::modscss::sanitize_mod_css;
use crate::util::{clip, read_json, write_json};

/// Maximum size of a mod (characters): a style sheet, not an archive.
pub const MAX_CSS: usize = 200_000;
pub const MAX_NAME: usize = 60;

pub use signet_core::types::{InterfaceMod, ModsInfo};

/// Mod read from disk or received from the UI, validated; `None` if unusable. `untitled` is the default name.
pub fn clean(value: &Value, untitled: &str) -> Option<InterfaceMod> {
    let o = value.as_object()?;
    let id = o.get("id").and_then(|i| i.as_str()).filter(|i| !i.is_empty())?;
    let name = o.get("name").and_then(|n| n.as_str()).map(str::trim).filter(|n| !n.is_empty()).map(|n| clip(n, MAX_NAME)).unwrap_or_else(|| untitled.to_string());
    let css = o.get("css").and_then(|c| c.as_str()).map(|c| clip(c, MAX_CSS)).unwrap_or_default();
    Some(InterfaceMod { id: id.into(), name, css, enabled: o.get("enabled").and_then(|e| e.as_bool()) != Some(false) })
}

/// Name header of an exported file, read again on import.
pub fn parse_name_header(css: &str) -> Option<(String, &str)> {
    let t = css.trim_start();
    let rest = t.strip_prefix("/*")?.trim_start();
    if !rest.get(..5)?.eq_ignore_ascii_case("name:") {
        return None;
    }
    let rest = &rest[5..];
    let end = rest.find("*/")?;
    let name = rest[..end].trim();
    (!name.is_empty()).then(|| (name.to_string(), rest[end + 2..].trim_start()))
}

/// Content of an exported file.
pub fn export_css(m: &InterfaceMod) -> String {
    format!("/* name: {} */\n{}\n", m.name.replace("*/", ""), m.css)
}

/// File name for an export (`name.css`, forbidden characters replaced).
pub fn export_file_name(name: &str) -> String {
    let n: String = name.chars().map(|c| if "/\\?%*:|\"<>".contains(c) { '-' } else { c }).collect();
    format!("{n}.css")
}

pub struct Mods {
    file: PathBuf,
    pub mods: Vec<InterfaceMod>,
    pub disabled: bool,
    /// `--safe-mode` or `SIGNET_SAFE_MODE=1`.
    pub safe_mode: bool,
    untitled: String,
}

impl Mods {
    pub fn load(data_dir: &Path, safe_mode: bool, untitled: &str) -> Self {
        let file = data_dir.join("mods.json");
        let mut m = Mods { file, mods: vec![], disabled: false, safe_mode, untitled: untitled.into() };
        if let Some(raw) = read_json::<Value>(&m.file) {
            m.disabled = raw.get("disabled").and_then(|d| d.as_bool()) == Some(true);
            if let Some(a) = raw.get("mods").and_then(|m| m.as_array()) {
                m.mods = a.iter().filter_map(|v| clean(v, untitled)).collect();
            }
        }
        m
    }

    pub fn safe_mode_from_env() -> bool {
        std::env::args().any(|a| a == "--safe-mode") || std::env::var("SIGNET_SAFE_MODE").is_ok_and(|v| v == "1")
    }

    /// Are mods turned off (setting or safe mode)?
    pub fn off(&self) -> bool {
        self.disabled || self.safe_mode
    }

    pub fn info(&self) -> ModsInfo {
        ModsInfo { mods: self.mods.clone(), disabled: self.disabled, safe_mode: self.safe_mode }
    }

    /// Applied sheet: the active mods in list order, without remote loading.
    pub fn sheet(&self) -> String {
        if self.off() {
            return String::new();
        }
        self.mods.iter().filter(|m| m.enabled && !m.css.trim().is_empty()).map(|m| format!("/* {} */\n{}", m.name.replace("*/", ""), sanitize_mod_css(&m.css))).collect::<Vec<_>>().join("\n\n")
    }

    pub fn flush(&self) {
        let _ = write_json(&self.file, &serde_json::json!({ "version": 1, "disabled": self.disabled, "mods": self.mods }));
    }

    /// Adds or edits a mod; returns its id. The app reapplies `sheet()` to its views.
    pub fn save(&mut self, id: Option<&str>, name: &Value, css: &Value) -> String {
        let new_id = id.map(String::from).unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let Some(m) = clean(&serde_json::json!({"id": new_id, "name": name, "css": css, "enabled": true}), &self.untitled) else { return id.unwrap_or("").to_string() };
        match id.and_then(|i| self.mods.iter_mut().find(|x| x.id == i)) {
            Some(e) => { e.name = m.name.clone(); e.css = m.css.clone(); }
            None => self.mods.push(m.clone()),
        }
        self.flush();
        m.id
    }

    pub fn remove(&mut self, id: &str) {
        self.mods.retain(|m| m.id != id);
        self.flush();
    }

    pub fn set_enabled(&mut self, id: &str, enabled: bool) {
        if let Some(m) = self.mods.iter_mut().find(|m| m.id == id) {
            m.enabled = enabled;
            self.flush();
        }
    }

    pub fn move_by(&mut self, id: &str, delta: i32) {
        let Some(i) = self.mods.iter().position(|m| m.id == id) else { return };
        let j = i as i64 + delta.signum() as i64;
        if j < 0 || j as usize >= self.mods.len() {
            return;
        }
        self.mods.swap(i, j as usize);
        self.flush();
    }

    pub fn set_disabled(&mut self, disabled: bool) {
        if disabled != self.disabled {
            self.disabled = disabled;
            self.flush();
        }
    }

    /// Imports the content of a `.css` file; `file_stem` is used when it has no name header.
    pub fn import_css(&mut self, file_stem: &str, css: &str) -> String {
        match parse_name_header(css) {
            Some((name, rest)) => self.save(None, &Value::String(name), &Value::String(rest.to_string())),
            None => self.save(None, &Value::String(file_stem.to_string()), &Value::String(css.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn clean_rules() {
        assert_eq!(clean(&json!({"name":"n"}), "Untitled"), None);
        assert_eq!(clean(&json!(3), "Untitled"), None);
        let m = clean(&json!({"id":"1","name":"  ","css":"a".repeat(MAX_CSS + 5),"enabled":false}), "Untitled").unwrap();
        assert_eq!((m.name.as_str(), m.css.len(), m.enabled), ("Untitled", MAX_CSS, false));
        assert_eq!(clean(&json!({"id":"1","name":"x".repeat(100)}), "U").unwrap().name.len(), MAX_NAME);
        assert!(clean(&json!({"id":"1"}), "U").unwrap().enabled);
    }

    #[test]
    fn crud_persistence_and_sheet() {
        let d = tempfile::tempdir().unwrap();
        let mut m = Mods::load(d.path(), false, "Untitled");
        let a = m.save(None, &json!("A"), &json!("a{background:url(https://evil.com)}"));
        let b = m.save(None, &json!("B */"), &json!("b{color:red}"));
        assert_eq!(m.save(Some(&a), &json!("A2"), &json!("a{color:blue}")), a);
        assert_eq!(m.mods[0].name, "A2");
        let sheet = m.sheet();
        assert!(sheet.starts_with("/* A2 */\na{color:blue}\n\n/* B  */"), "{sheet}");
        m.move_by(&b, -1);
        assert_eq!(m.mods[0].id, b);
        m.move_by(&b, -1);
        m.set_enabled(&a, false);
        assert!(!m.sheet().contains("A2"));
        m.set_disabled(true);
        assert_eq!(m.sheet(), "");
        let again = Mods::load(d.path(), false, "Untitled");
        assert!(again.disabled);
        assert_eq!(again.mods.len(), 2);
        let raw: Value = serde_json::from_str(&std::fs::read_to_string(d.path().join("mods.json")).unwrap()).unwrap();
        assert_eq!(raw["version"], 1);
        let mut m2 = Mods::load(d.path(), true, "U");
        m2.disabled = false;
        assert_eq!(m2.sheet(), "");
        m.remove(&a);
        assert_eq!(m.mods.len(), 1);
    }

    #[test]
    fn remote_loads_removed_from_stored_mods() {
        let d = tempfile::tempdir().unwrap();
        let mut m = Mods::load(d.path(), false, "U");
        m.save(None, &json!("x"), &json!("a{background:url(https://evil.com)}"));
        assert!(!m.sheet().contains("evil.com"));
    }

    #[test]
    fn import_export() {
        let d = tempfile::tempdir().unwrap();
        let mut m = Mods::load(d.path(), false, "U");
        let id = m.import_css("file-name", "/* name: Dark UI */\n.a{color:red}");
        assert_eq!((m.mods[0].name.as_str(), m.mods[0].css.as_str()), ("Dark UI", ".a{color:red}"));
        m.import_css("plain", ".b{}");
        assert_eq!(m.mods[1].name, "plain");
        let exported = export_css(&m.mods.iter().find(|x| x.id == id).unwrap().clone());
        assert_eq!(exported, "/* name: Dark UI */\n.a{color:red}\n");
        assert_eq!(export_file_name("a/b:c"), "a-b-c.css");
    }
}

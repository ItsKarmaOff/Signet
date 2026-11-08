//! Installed extensions: the list, enabled state, the toolbar (pinned extensions, SPEC v10) and the data for Settings.
//!
//! Store extensions live in `<data>/Extensions/<id>/<version>_0/` (the layout Electron used), unpacked ones stay in
//! their folder. The registry scans the store folder at startup; `enabled` and install times are kept in
//! `extensions-state.json`. Pinned extensions and their order are `settings.pinnedExtensions` / `settings.extensionOrder`
//! of `state.json` (owned by signet-core): the functions here take those lists and return the new ones.

use crate::common::{write_atomic, JsonStore};
use crate::manifest::{compare_versions, Manifest};
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Store,
    Unpacked,
}

#[derive(Debug, Clone)]
pub struct Installed {
    pub id: String,
    pub version: String,
    pub name: String,
    pub dir: PathBuf,
    pub source: Source,
    pub enabled: bool,
    /// Milliseconds since the epoch; extensions loaded earlier rank lower when two fight for a shortcut or a rule.
    pub installed_at: u64,
    pub manifest: Manifest,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct UnpackedInfo {
    pub path: String,
    pub display: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reloaded_at: Option<i64>,
}

/// Installed extension as Settings shows it (same JSON as `ExtensionInfo` of the TypeScript app).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct ExtensionInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    pub has_action: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commands: Option<Vec<Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unpacked: Option<UnpackedInfo>,
}

pub struct ExtensionRegistry {
    extensions_dir: PathBuf,
    store: JsonStore,
    items: Vec<Installed>,
    /// `id -> (enabled, installedAt)` as persisted.
    saved: HashMap<String, (bool, u64)>,
}

fn is_version_dir(name: &str) -> bool {
    name.strip_suffix("_0").is_some_and(|v| crate::manifest::is_valid_version(v))
}

/// Reads the manifest of a folder.
pub fn read_manifest(dir: &Path) -> Result<Manifest, String> {
    let text = fs::read_to_string(dir.join("manifest.json")).map_err(|_| "Manifest file is missing or unreadable".to_string())?;
    Manifest::parse(&text).map_err(|e| e.0)
}

/// Extension id of a folder: the one of its `key`, or the one derived from the path (unpacked).
pub fn id_of_folder(dir: &Path, manifest: &Manifest) -> String {
    manifest.key.as_deref().and_then(crate::id::id_from_manifest_key).unwrap_or_else(|| crate::id::id_from_path(dir))
}

impl ExtensionRegistry {
    /// Scans `<extensions_dir>` (store installs); `state_file` keeps the enabled flags.
    pub fn load(extensions_dir: impl Into<PathBuf>, state_file: impl AsRef<Path>) -> Self {
        let store = JsonStore::new(state_file.as_ref());
        let mut saved = HashMap::new();
        if let Some(Value::Object(map)) = store.read().and_then(|d| d.get("extensions").cloned()) {
            for (id, e) in map {
                saved.insert(id, (e.get("enabled").and_then(|b| b.as_bool()) != Some(false), e.get("installedAt").and_then(|t| t.as_u64()).unwrap_or(0)));
            }
        }
        let mut r = ExtensionRegistry { extensions_dir: extensions_dir.into(), store, items: vec![], saved };
        r.scan();
        r
    }

    fn scan(&mut self) {
        let Ok(entries) = fs::read_dir(&self.extensions_dir) else { return };
        let mut found: Vec<Installed> = Vec::new();
        for entry in entries.flatten() {
            let id = entry.file_name().to_string_lossy().into_owned();
            if !crate::id::is_valid_id(&id) || !entry.path().is_dir() {
                continue;
            }
            // Newest version folder that has a valid manifest wins.
            let mut versions: Vec<String> = fs::read_dir(entry.path()).into_iter().flatten().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| is_version_dir(n)).collect();
            versions.sort_by(|a, b| compare_versions(a.trim_end_matches("_0"), b.trim_end_matches("_0")));
            for v in versions.iter().rev() {
                let dir = entry.path().join(v);
                if let Ok(manifest) = read_manifest(&dir) {
                    let (enabled, installed_at) = self.saved.get(&id).copied().unwrap_or((true, 0));
                    found.push(Installed { id: id.clone(), version: manifest.version.clone(), name: manifest.name.clone(), dir, source: Source::Store, enabled, installed_at, manifest });
                    break;
                }
            }
        }
        found.sort_by(|a, b| a.installed_at.cmp(&b.installed_at).then(a.id.cmp(&b.id)));
        self.items = found;
    }

    fn snapshot(&self) -> Value {
        json!({
            "version": 1,
            "extensions": self.items.iter().filter(|i| i.source == Source::Store).map(|i| (i.id.clone(), json!({ "enabled": i.enabled, "installedAt": i.installed_at }))).collect::<serde_json::Map<_, _>>()
        })
    }

    pub fn flush(&mut self) -> bool {
        let v = self.snapshot();
        self.store.flush(&v)
    }

    pub fn extensions_dir(&self) -> &Path {
        &self.extensions_dir
    }

    pub fn list(&self) -> &[Installed] {
        &self.items
    }

    pub fn get(&self, id: &str) -> Option<&Installed> {
        self.items.iter().find(|i| i.id == id)
    }

    /// Enabled extensions in load order.
    pub fn enabled(&self) -> impl Iterator<Item = &Installed> {
        self.items.iter().filter(|i| i.enabled)
    }

    /// Adds or replaces an extension (a store install or update, an unpacked load). A store update removes the folders of
    /// older versions of the same id.
    pub fn add(&mut self, mut item: Installed, now_ms: u64) {
        if let Some(pos) = self.items.iter().position(|i| i.id == item.id) {
            let old = self.items.remove(pos);
            item.installed_at = old.installed_at;
            item.enabled = old.enabled;
            if old.source == Source::Store && item.source == Source::Store && old.dir != item.dir {
                let _ = fs::remove_dir_all(&old.dir);
            }
            self.items.insert(pos, item);
        } else {
            item.installed_at = now_ms;
            self.items.push(item);
        }
        self.store.request();
    }

    /// Forgets an extension. A store extension's folder is erased; an unpacked one is never touched.
    pub fn remove(&mut self, id: &str) -> Option<Installed> {
        let pos = self.items.iter().position(|i| i.id == id)?;
        let item = self.items.remove(pos);
        if item.source == Source::Store {
            let _ = fs::remove_dir_all(self.extensions_dir.join(&item.id));
        }
        self.saved.remove(id);
        self.store.request();
        Some(item)
    }

    pub fn set_enabled(&mut self, id: &str, enabled: bool) -> bool {
        match self.items.iter_mut().find(|i| i.id == id) {
            Some(i) if i.enabled != enabled => {
                i.enabled = enabled;
                self.store.request();
                true
            }
            _ => false,
        }
    }

    /// Id of the extension that a rank comparison uses: lower comes first (loaded earlier).
    pub fn rank(&self, id: &str) -> usize {
        self.items.iter().position(|i| i.id == id).unwrap_or(usize::MAX)
    }

    /// Icon of the manifest as a data URL (largest declared size).
    pub fn icon_data_url(item: &Installed) -> Option<String> {
        let rel = item.manifest.best_icon()?;
        let file = crate::urlmatch::extension_file_path(&item.dir, &item.id, rel)?;
        let bytes = fs::read(&file).ok()?;
        let lower = file.to_string_lossy().to_lowercase();
        let ty = if lower.ends_with(".svg") {
            "image/svg+xml"
        } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
            "image/jpeg"
        } else {
            "image/png"
        };
        Some(format!("data:{ty};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
    }

    /// The Settings list: loaded extensions, then `failed` unpacked ones (no id), sorted by name.
    pub fn infos(&self, commands: &HashMap<String, Vec<Value>>, unpacked: &dyn Fn(&str) -> Option<UnpackedInfo>, failed: Vec<(String, UnpackedInfo)>) -> Vec<ExtensionInfo> {
        let mut out: Vec<ExtensionInfo> = self
            .items
            .iter()
            .map(|i| ExtensionInfo {
                id: i.id.clone(),
                name: i.name.clone(),
                version: i.version.clone(),
                icon: Self::icon_data_url(i),
                has_action: i.manifest.has_action(),
                commands: commands.get(&i.id).cloned(),
                unpacked: if i.source == Source::Unpacked { unpacked(&i.id) } else { None },
            })
            .collect();
        out.extend(failed.into_iter().map(|(name, u)| ExtensionInfo { name, unpacked: Some(u), ..Default::default() }));
        out.sort_by_key(|e| e.name.to_lowercase());
        out
    }
}

/// Pins or unpins an extension: returns the new `pinnedExtensions` (appended at the end when pinned).
pub fn set_pinned(pinned: &[String], id: &str, on: bool) -> Vec<String> {
    let mut v: Vec<String> = pinned.iter().filter(|x| *x != id).cloned().collect();
    if on {
        v.push(id.to_string());
    }
    v
}

/// Buttons of the toolbar: only pinned extensions that have a button, in `extensionOrder` order, then the other pinned
/// ones in pin order. Unknown or disabled ids are skipped.
pub fn toolbar_ids(pinned: &[String], order: &[String], has_button: &dyn Fn(&str) -> bool) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for id in order.iter().chain(pinned.iter()) {
        if pinned.contains(id) && has_button(id) && !out.contains(id) {
            out.push(id.clone());
        }
    }
    out
}

/// New `extensionOrder` after dragging `id` before `before` (or to the end when `before` is None).
pub fn move_in_order(order: &[String], id: &str, before: Option<&str>) -> Vec<String> {
    let mut v: Vec<String> = order.iter().filter(|x| *x != id).cloned().collect();
    let at = before.and_then(|b| v.iter().position(|x| x == b)).unwrap_or(v.len());
    v.insert(at, id.to_string());
    v
}

/// Writes a small file atomically (helper for the app's own metadata next to the registry).
pub fn write_file(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    write_atomic(path, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "mpedjdfgkmlmhabmbdmnledbjbnmdfgb";

    fn write_ext(root: &Path, id: &str, version: &str, extra: Value) {
        let dir = root.join(id).join(format!("{version}_0"));
        fs::create_dir_all(&dir).unwrap();
        let mut m = json!({ "manifest_version": 3, "name": format!("Ext {version}"), "version": version });
        for (k, v) in extra.as_object().unwrap() {
            m[k] = v.clone();
        }
        fs::write(dir.join("manifest.json"), m.to_string()).unwrap();
    }

    #[test]
    fn scan_picks_the_newest_valid_version_per_id() {
        let d = tempfile::tempdir().unwrap();
        write_ext(d.path(), ID, "1.9", json!({}));
        write_ext(d.path(), ID, "1.10", json!({}));
        let broken = d.path().join(ID).join("2.0_0");
        fs::create_dir_all(&broken).unwrap();
        fs::write(broken.join("manifest.json"), "{ nope").unwrap();
        fs::create_dir_all(d.path().join("not-an-id")).unwrap();
        let r = ExtensionRegistry::load(d.path(), d.path().join("state.json"));
        assert_eq!(r.list().len(), 1);
        assert_eq!((r.list()[0].version.as_str(), r.list()[0].source), ("1.10", Source::Store));
    }

    #[test]
    fn enabled_flag_and_install_time_persist() {
        let d = tempfile::tempdir().unwrap();
        write_ext(d.path(), ID, "1.0", json!({}));
        let state = d.path().join("state.json");
        let mut r = ExtensionRegistry::load(d.path(), &state);
        assert!(r.set_enabled(ID, false));
        assert!(!r.set_enabled(ID, false));
        assert!(r.flush());
        let again = ExtensionRegistry::load(d.path(), &state);
        assert!(!again.get(ID).unwrap().enabled);
        assert_eq!(again.enabled().count(), 0);
    }

    #[test]
    fn add_replaces_keeps_state_and_removes_old_version_folders() {
        let d = tempfile::tempdir().unwrap();
        write_ext(d.path(), ID, "1.0", json!({}));
        let mut r = ExtensionRegistry::load(d.path(), d.path().join("state.json"));
        r.set_enabled(ID, false);
        write_ext(d.path(), ID, "2.0", json!({}));
        let dir = d.path().join(ID).join("2.0_0");
        let manifest = read_manifest(&dir).unwrap();
        r.add(Installed { id: ID.into(), version: "2.0".into(), name: "n".into(), dir, source: Source::Store, enabled: true, installed_at: 0, manifest }, 99);
        assert_eq!(r.list().len(), 1);
        assert_eq!(r.get(ID).unwrap().version, "2.0");
        assert!(!r.get(ID).unwrap().enabled, "keeps the user's choice");
        assert!(!d.path().join(ID).join("1.0_0").exists());
    }

    #[test]
    fn removing_a_store_extension_erases_its_folder_but_never_an_unpacked_one() {
        let d = tempfile::tempdir().unwrap();
        write_ext(d.path(), ID, "1.0", json!({}));
        let mut r = ExtensionRegistry::load(d.path(), d.path().join("state.json"));
        assert!(r.remove(ID).is_some());
        assert!(!d.path().join(ID).exists());
        assert!(r.remove(ID).is_none());
        let unpacked = tempfile::tempdir().unwrap();
        fs::write(unpacked.path().join("manifest.json"), json!({ "manifest_version": 3, "name": "U", "version": "1" }).to_string()).unwrap();
        let manifest = read_manifest(unpacked.path()).unwrap();
        let id = id_of_folder(unpacked.path(), &manifest);
        r.add(Installed { id: id.clone(), version: "1".into(), name: "U".into(), dir: unpacked.path().to_path_buf(), source: Source::Unpacked, enabled: true, installed_at: 0, manifest }, 5);
        r.remove(&id);
        assert!(unpacked.path().join("manifest.json").exists());
    }

    #[test]
    fn infos_for_settings_have_the_electron_shape() {
        let d = tempfile::tempdir().unwrap();
        write_ext(d.path(), ID, "1.0", json!({ "action": {}, "icons": { "16": "i.png" } }));
        fs::write(d.path().join(ID).join("1.0_0").join("i.png"), [1u8, 2, 3]).unwrap();
        let r = ExtensionRegistry::load(d.path(), d.path().join("state.json"));
        let commands = HashMap::from([(ID.to_string(), vec![json!({ "name": "x", "description": "", "shortcut": "" })])]);
        let failed = vec![("Broken".to_string(), UnpackedInfo { path: "/p".into(), display: "/p".into(), error: Some("bad".into()), reloaded_at: None })];
        let infos = r.infos(&commands, &|_| None, failed);
        assert_eq!(infos.len(), 2);
        assert_eq!(infos[0].name, "Broken");
        assert!(infos[0].id.is_empty());
        assert!(infos[1].has_action);
        assert_eq!(infos[1].icon.as_deref(), Some("data:image/png;base64,AQID"));
        let json = serde_json::to_value(&infos[1]).unwrap();
        assert_eq!(json["hasAction"], true);
        assert!(json.get("unpacked").is_none());
    }

    #[test]
    fn pinning_and_toolbar_order() {
        let pinned = set_pinned(&[], "a", true);
        let pinned = set_pinned(&pinned, "b", true);
        let pinned = set_pinned(&pinned, "c", true);
        assert_eq!(pinned, ["a", "b", "c"]);
        assert_eq!(set_pinned(&pinned, "b", false), ["a", "c"]);
        assert_eq!(set_pinned(&pinned, "a", true), ["b", "c", "a"]);
        let order = vec!["c".to_string(), "x".to_string(), "a".to_string()];
        let has = |id: &str| id != "b";
        // Pinned buttons in the order list first, then the other pinned ones; b has no button, x is not pinned.
        assert_eq!(toolbar_ids(&pinned, &order, &has), ["c", "a"]);
        assert_eq!(toolbar_ids(&["z".to_string(), "a".to_string()], &[], &has), ["z", "a"]);
        assert_eq!(move_in_order(&order, "a", Some("c")), ["a", "c", "x"]);
        assert_eq!(move_in_order(&order, "c", None), ["x", "a", "c"]);
        assert_eq!(move_in_order(&order, "n", Some("missing")), ["c", "x", "a", "n"]);
    }

    #[test]
    fn rank_is_the_load_order() {
        let d = tempfile::tempdir().unwrap();
        write_ext(d.path(), ID, "1.0", json!({}));
        let r = ExtensionRegistry::load(d.path(), d.path().join("state.json"));
        assert_eq!(r.rank(ID), 0);
        assert_eq!(r.rank("zzz"), usize::MAX);
    }
}

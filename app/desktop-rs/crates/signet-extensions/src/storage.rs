//! `chrome.storage`: `local`, `session`, `sync` and `managed` areas. New code (the Electron app used Chromium's own
//! implementation). `local` and `sync` are persisted as one JSON file per extension and area under
//! `<data>/Extension Storage/<id>/`; `session` lives in memory; `managed` is empty and read-only (enterprise policy).
//! `sync` is local-only (no account), with Chrome's quotas, so extensions that rely on its limits behave the same.

use crate::common::{write_atomic, ApiError, ApiResult, Emitter};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

pub const LOCAL_QUOTA: usize = 10 * 1024 * 1024;
pub const SESSION_QUOTA: usize = 10 * 1024 * 1024;
pub const SYNC_QUOTA: usize = 102_400;
pub const SYNC_ITEM_QUOTA: usize = 8_192;
pub const SYNC_MAX_ITEMS: usize = 512;
const READ_ONLY: &str = "This is a read-only store.";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Area {
    Local,
    Session,
    Sync,
    Managed,
}

impl Area {
    pub fn parse(s: &str) -> Option<Area> {
        match s {
            "local" => Some(Area::Local),
            "session" => Some(Area::Session),
            "sync" => Some(Area::Sync),
            "managed" => Some(Area::Managed),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Area::Local => "local",
            Area::Session => "session",
            Area::Sync => "sync",
            Area::Managed => "managed",
        }
    }
}

/// Bytes Chrome counts for an item: the key plus its JSON text.
fn item_bytes(key: &str, value: &Value) -> usize {
    key.len() + value.to_string().len()
}

fn total_bytes(map: &Map<String, Value>) -> usize {
    map.iter().map(|(k, v)| item_bytes(k, v)).sum()
}

pub struct ExtensionStorage {
    dir: PathBuf,
    emitter: Arc<dyn Emitter>,
    /// (extension, area) to the contents.
    data: HashMap<(String, Area), Map<String, Value>>,
    dirty: Vec<(String, Area)>,
    /// Extensions with `unlimitedStorage`: the local quota does not apply.
    unlimited: Vec<String>,
}

impl ExtensionStorage {
    pub fn new(dir: impl Into<PathBuf>, emitter: Arc<dyn Emitter>) -> Self {
        ExtensionStorage { dir: dir.into(), emitter, data: HashMap::new(), dirty: vec![], unlimited: vec![] }
    }

    pub fn set_unlimited(&mut self, ext: &str, on: bool) {
        self.unlimited.retain(|e| e != ext);
        if on {
            self.unlimited.push(ext.to_string());
        }
    }

    fn file(&self, ext: &str, area: Area) -> PathBuf {
        self.dir.join(ext).join(format!("{}.json", area.name()))
    }

    fn area(&mut self, ext: &str, area: Area) -> &mut Map<String, Value> {
        if !self.data.contains_key(&(ext.to_string(), area)) {
            let loaded = match area {
                Area::Local | Area::Sync => fs::read_to_string(self.file(ext, area)).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).and_then(|v| v.as_object().cloned()).unwrap_or_default(),
                _ => Map::new(),
            };
            self.data.insert((ext.to_string(), area), loaded);
        }
        self.data.get_mut(&(ext.to_string(), area)).expect("inserted")
    }

    /// Writes the areas that changed.
    pub fn flush(&mut self) -> bool {
        let mut ok = true;
        for (ext, area) in std::mem::take(&mut self.dirty) {
            if let Some(map) = self.data.get(&(ext.clone(), area)) {
                let bytes = serde_json::to_vec(map).unwrap_or_default();
                ok &= write_atomic(&self.file(&ext, area), &bytes).is_ok();
            }
        }
        ok
    }

    pub fn has_unsaved(&self) -> bool {
        !self.dirty.is_empty()
    }

    fn mark(&mut self, ext: &str, area: Area) {
        if matches!(area, Area::Local | Area::Sync) && !self.dirty.iter().any(|(e, a)| e == ext && *a == area) {
            self.dirty.push((ext.to_string(), area));
        }
    }

    /// `get(keys)`: null/undefined = everything, a string, a list, or an object of defaults.
    pub fn get(&mut self, ext: &str, area: Area, keys: &Value) -> Value {
        let map = self.area(ext, area).clone();
        match keys {
            Value::Null => Value::Object(map),
            Value::String(k) => Value::Object(map.get(k).map(|v| Map::from_iter([(k.clone(), v.clone())])).unwrap_or_default()),
            Value::Array(list) => Value::Object(list.iter().filter_map(|k| k.as_str()).filter_map(|k| map.get(k).map(|v| (k.to_string(), v.clone()))).collect()),
            Value::Object(defaults) => Value::Object(defaults.iter().map(|(k, d)| (k.clone(), map.get(k).cloned().unwrap_or_else(|| d.clone()))).collect()),
            _ => Value::Object(Map::new()),
        }
    }

    pub fn keys(&mut self, ext: &str, area: Area) -> Value {
        Value::Array(self.area(ext, area).keys().map(|k| json!(k)).collect())
    }

    pub fn bytes_in_use(&mut self, ext: &str, area: Area, keys: &Value) -> usize {
        let map = self.area(ext, area);
        match keys {
            Value::Null => total_bytes(map),
            Value::String(k) => map.get(k).map_or(0, |v| item_bytes(k, v)),
            Value::Array(l) => l.iter().filter_map(|k| k.as_str()).filter_map(|k| map.get(k).map(|v| item_bytes(k, v))).sum(),
            _ => 0,
        }
    }

    fn quota(&self, ext: &str, area: Area) -> usize {
        match area {
            Area::Local if self.unlimited.iter().any(|e| e == ext) => usize::MAX,
            Area::Local => LOCAL_QUOTA,
            Area::Session => SESSION_QUOTA,
            Area::Sync => SYNC_QUOTA,
            Area::Managed => 0,
        }
    }

    fn notify(&self, ext: &str, area: Area, changes: Map<String, Value>) {
        if changes.is_empty() {
            return;
        }
        let changes = Value::Object(changes);
        self.emitter.emit(ext, "storage.onChanged", vec![changes.clone(), json!(area.name())], false, false);
        self.emitter.emit(ext, &format!("storage.{}.onChanged", area.name()), vec![changes], false, false);
    }

    pub fn set(&mut self, ext: &str, area: Area, items: &Value) -> ApiResult<()> {
        if area == Area::Managed {
            return Err(ApiError::new(READ_ONLY));
        }
        let Some(items) = items.as_object() else { return Err(ApiError::new("Invalid arguments.")) };
        let quota = self.quota(ext, area);
        let current = self.area(ext, area).clone();
        let mut next = current.clone();
        for (k, v) in items {
            if area == Area::Sync && item_bytes(k, v) > SYNC_ITEM_QUOTA {
                return Err(ApiError::new(format!("QUOTA_BYTES_PER_ITEM quota exceeded")));
            }
            next.insert(k.clone(), v.clone());
        }
        if total_bytes(&next) > quota {
            return Err(ApiError::new(if area == Area::Sync { "QUOTA_BYTES quota exceeded" } else { "QUOTA_BYTES quota exceeded" }));
        }
        if area == Area::Sync && next.len() > SYNC_MAX_ITEMS {
            return Err(ApiError::new("MAX_ITEMS quota exceeded"));
        }
        let mut changes = Map::new();
        for (k, v) in items {
            if current.get(k) != Some(v) {
                let mut c = Map::new();
                if let Some(old) = current.get(k) {
                    c.insert("oldValue".into(), old.clone());
                }
                c.insert("newValue".into(), v.clone());
                changes.insert(k.clone(), Value::Object(c));
            }
        }
        *self.area(ext, area) = next;
        self.mark(ext, area);
        self.notify(ext, area, changes);
        Ok(())
    }

    pub fn remove(&mut self, ext: &str, area: Area, keys: &Value) -> ApiResult<()> {
        if area == Area::Managed {
            return Err(ApiError::new(READ_ONLY));
        }
        let list: Vec<String> = match keys {
            Value::String(k) => vec![k.clone()],
            Value::Array(l) => l.iter().filter_map(|k| k.as_str().map(String::from)).collect(),
            _ => return Err(ApiError::new("Invalid arguments.")),
        };
        let mut changes = Map::new();
        let map = self.area(ext, area);
        for k in list {
            if let Some(old) = map.remove(&k) {
                changes.insert(k, json!({ "oldValue": old }));
            }
        }
        self.mark(ext, area);
        self.notify(ext, area, changes);
        Ok(())
    }

    pub fn clear(&mut self, ext: &str, area: Area) -> ApiResult<()> {
        if area == Area::Managed {
            return Err(ApiError::new(READ_ONLY));
        }
        let old = std::mem::take(self.area(ext, area));
        let changes: Map<String, Value> = old.into_iter().map(|(k, v)| (k, json!({ "oldValue": v }))).collect();
        self.mark(ext, area);
        self.notify(ext, area, changes);
        Ok(())
    }

    /// The extension is uninstalled: its data goes, on disk too. An unload keeps it.
    pub fn forget(&mut self, ext: &str) {
        self.data.retain(|(e, _), _| e != ext);
        self.dirty.retain(|(e, _)| e != ext);
        let _ = fs::remove_dir_all(self.dir.join(ext));
    }

    /// The service worker stopped / browser restarted: session storage is gone.
    pub fn clear_session(&mut self, ext: &str) {
        self.data.remove(&(ext.to_string(), Area::Session));
    }

    /// Ids with data on disk.
    pub fn known_ids(&self) -> Vec<String> {
        fs::read_dir(&self.dir).into_iter().flatten().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| crate::id::is_valid_id(n)).collect()
    }

    /// Router dispatch of `storage.<area>.<op>`.
    pub fn call(&mut self, ext: &str, method: &str, args: &[Value]) -> ApiResult<Value> {
        let mut parts = method.split('.');
        let (Some("storage"), Some(area), Some(op), None) = (parts.next(), parts.next(), parts.next(), parts.next()) else {
            return Err(ApiError::new(format!("Unknown Extension API - {method}")));
        };
        let area = Area::parse(area).ok_or_else(|| ApiError::new(format!("Unknown Extension API - {method}")))?;
        let a = args.first().cloned().unwrap_or(Value::Null);
        match op {
            "get" => Ok(self.get(ext, area, &a)),
            "getKeys" => Ok(self.keys(ext, area)),
            "getBytesInUse" => Ok(json!(self.bytes_in_use(ext, area, &a))),
            "set" => self.set(ext, area, &a).map(|_| Value::Null),
            "remove" => self.remove(ext, area, &a).map(|_| Value::Null),
            "clear" => self.clear(ext, area).map(|_| Value::Null),
            "setAccessLevel" => Ok(Value::Null),
            _ => Err(ApiError::new(format!("Unknown Extension API - {method}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::Outbox;

    const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn make(dir: &std::path::Path) -> (ExtensionStorage, Arc<Outbox>) {
        let out = Arc::new(Outbox::new());
        (ExtensionStorage::new(dir, out.clone()), out)
    }

    #[test]
    fn get_accepts_null_string_list_and_defaults() {
        let d = tempfile::tempdir().unwrap();
        let (mut s, _) = make(d.path());
        s.set(A, Area::Local, &json!({ "a": 1, "b": "two", "c": [3] })).unwrap();
        assert_eq!(s.get(A, Area::Local, &Value::Null), json!({ "a": 1, "b": "two", "c": [3] }));
        assert_eq!(s.get(A, Area::Local, &json!("a")), json!({ "a": 1 }));
        assert_eq!(s.get(A, Area::Local, &json!(["a", "zz", "b"])), json!({ "a": 1, "b": "two" }));
        assert_eq!(s.get(A, Area::Local, &json!({ "a": 9, "x": "dflt" })), json!({ "a": 1, "x": "dflt" }));
    }

    #[test]
    fn on_changed_reports_old_and_new_values_to_running_contexts_only() {
        let d = tempfile::tempdir().unwrap();
        let (mut s, out) = make(d.path());
        s.set(A, Area::Local, &json!({ "a": 1 })).unwrap();
        s.set(A, Area::Local, &json!({ "a": 2, "b": 1 })).unwrap();
        s.set(A, Area::Local, &json!({ "a": 2 })).unwrap(); // nothing changed
        s.remove(A, Area::Local, &json!(["a", "nope"])).unwrap();
        let ev = out.drain();
        let global: Vec<_> = ev.iter().filter(|e| e.name == "storage.onChanged").collect();
        assert_eq!(global.len(), 3);
        assert_eq!(global[0].args, vec![json!({ "a": { "newValue": 1 } }), json!("local")]);
        assert_eq!(global[1].args[0], json!({ "a": { "oldValue": 1, "newValue": 2 }, "b": { "newValue": 1 } }));
        assert_eq!(global[2].args[0], json!({ "a": { "oldValue": 2 } }));
        assert!(ev.iter().all(|e| !e.wake), "storage changes do not wake a stopped worker");
        assert!(ev.iter().any(|e| e.name == "storage.local.onChanged"));
    }

    #[test]
    fn local_persists_session_does_not_and_forget_erases() {
        let d = tempfile::tempdir().unwrap();
        let (mut s, _) = make(d.path());
        s.set(A, Area::Local, &json!({ "keep": true })).unwrap();
        s.set(A, Area::Session, &json!({ "temp": true })).unwrap();
        assert!(s.has_unsaved());
        assert!(s.flush());
        assert!(!s.has_unsaved());
        let (mut again, _) = make(d.path());
        assert_eq!(again.get(A, Area::Local, &Value::Null), json!({ "keep": true }));
        assert_eq!(again.get(A, Area::Session, &Value::Null), json!({}));
        assert_eq!(again.known_ids(), [A]);
        again.forget(A);
        assert!(again.known_ids().is_empty());
        assert_eq!(again.get(A, Area::Local, &Value::Null), json!({}));
    }

    #[test]
    fn managed_is_empty_and_read_only() {
        let d = tempfile::tempdir().unwrap();
        let (mut s, _) = make(d.path());
        assert_eq!(s.get(A, Area::Managed, &Value::Null), json!({}));
        assert_eq!(s.get(A, Area::Managed, &json!({ "d": 1 })), json!({ "d": 1 }));
        assert_eq!(s.set(A, Area::Managed, &json!({ "a": 1 })).unwrap_err().0, "This is a read-only store.");
        assert!(s.remove(A, Area::Managed, &json!("a")).is_err());
        assert!(s.clear(A, Area::Managed).is_err());
    }

    #[test]
    fn sync_has_chrome_quotas_and_local_has_ten_megabytes() {
        let d = tempfile::tempdir().unwrap();
        let (mut s, _) = make(d.path());
        let big = "x".repeat(9000);
        assert!(s.set(A, Area::Sync, &json!({ "k": big })).unwrap_err().0.contains("QUOTA_BYTES_PER_ITEM"));
        for i in 0..12 {
            s.set(A, Area::Sync, &json!({ format!("k{i}"): "y".repeat(8000) })).ok();
        }
        assert!(s.set(A, Area::Sync, &json!({ "more": "y".repeat(8000) })).unwrap_err().0.contains("QUOTA_BYTES"));
        let huge = "z".repeat(LOCAL_QUOTA);
        assert!(s.set(A, Area::Local, &json!({ "h": huge })).is_err());
        s.set_unlimited(A, true);
        assert!(s.set(A, Area::Local, &json!({ "h": "z".repeat(LOCAL_QUOTA) })).is_ok());
    }

    #[test]
    fn bytes_in_use_keys_and_clear() {
        let d = tempfile::tempdir().unwrap();
        let (mut s, _) = make(d.path());
        s.set(A, Area::Local, &json!({ "a": "xx", "bb": 12 })).unwrap();
        assert_eq!(s.bytes_in_use(A, Area::Local, &json!("a")), 1 + 4);
        assert_eq!(s.bytes_in_use(A, Area::Local, &Value::Null), (1 + 4) + (2 + 2));
        assert_eq!(s.keys(A, Area::Local), json!(["a", "bb"]));
        s.clear(A, Area::Local).unwrap();
        assert_eq!(s.keys(A, Area::Local), json!([]));
    }

    #[test]
    fn call_dispatches_and_rejects_unknown_areas() {
        let d = tempfile::tempdir().unwrap();
        let (mut s, _) = make(d.path());
        s.call(A, "storage.local.set", &[json!({ "a": 1 })]).unwrap();
        assert_eq!(s.call(A, "storage.local.get", &[json!("a")]).unwrap(), json!({ "a": 1 }));
        assert_eq!(s.call(A, "storage.session.getKeys", &[]).unwrap(), json!([]));
        assert!(s.call(A, "storage.nowhere.get", &[]).is_err());
        assert!(s.call(A, "storage.local.set", &[json!("not an object")]).is_err());
    }
}

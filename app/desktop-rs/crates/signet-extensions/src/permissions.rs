//! `chrome.permissions` and the runtime grants store. Port of `apis/permissions.ts`.
//!
//! The Electron code awaits a native dialog inside `request`. Here the dialog is a step of a small state machine:
//! `request` returns a ticket plus [`PermEvent`]s; the app shows the dialog for each `PermEvent::Prompt` and calls
//! [`PermissionGrants::answer`] with the user's choice, which yields the `PermEvent::Resolved` that completes the call.

use crate::common::{strings, uniq, ApiError, ApiResult, Emitter, JsonStore};
use crate::urlmatch::{is_valid_pattern, pattern_covers};
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::sync::Arc;

/// What an extension holds or asks for: API permissions and host match patterns.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PermissionSet {
    pub permissions: Vec<String>,
    pub origins: Vec<String>,
}

impl PermissionSet {
    pub fn is_empty(&self) -> bool {
        self.permissions.is_empty() && self.origins.is_empty()
    }
    pub fn to_json(&self) -> Value {
        json!({ "permissions": self.permissions, "origins": self.origins })
    }
}

const NOT_DECLARED: &str = "Only permissions specified in the manifest may be requested.";
const REQUIRED: &str = "You cannot remove required permissions.";

/// MV2 manifests list host patterns inside `permissions`.
fn is_host(p: &str) -> bool {
    p == "<all_urls>" || p.contains("://")
}

pub struct Declared {
    pub required: PermissionSet,
    pub optional: PermissionSet,
}

/// What the manifest declares, split into API permissions and hosts, required and optional.
pub fn declared_of(manifest: &Value) -> Declared {
    let required = strings(manifest.get("permissions"));
    let optional = strings(manifest.get("optional_permissions"));
    let split = |list: Vec<String>, hosts: Vec<String>| PermissionSet {
        permissions: list.iter().filter(|p| !is_host(p)).cloned().collect(),
        origins: hosts.into_iter().chain(list.iter().filter(|p| is_host(p)).cloned()).collect(),
    };
    Declared {
        required: split(required, strings(manifest.get("host_permissions"))),
        optional: split(optional, strings(manifest.get("optional_host_permissions"))),
    }
}

fn covered_by_any(held: &[String], origin: &str) -> bool {
    held.iter().any(|p| pattern_covers(p, origin))
}

/// Validates the argument shared by request, contains and remove.
pub fn parse_request(raw: &Value) -> ApiResult<PermissionSet> {
    let invalid = || ApiError::new("Invalid arguments.");
    let obj = raw.as_object().ok_or_else(invalid)?;
    for key in ["permissions", "origins"] {
        match obj.get(key) {
            None | Some(Value::Null) => {}
            Some(Value::Array(a)) if a.iter().all(|s| s.is_string()) => {}
            _ => return Err(invalid()),
        }
    }
    let set = PermissionSet { permissions: uniq(strings(obj.get("permissions"))), origins: uniq(strings(obj.get("origins"))) };
    for o in &set.origins {
        if !is_valid_pattern(o) {
            return Err(ApiError::new(format!("Invalid host permission pattern '{o}'.")));
        }
    }
    Ok(set)
}

/// A step of the request state machine.
#[derive(Debug, Clone, PartialEq)]
pub enum PermEvent {
    /// Show the dialog for `need`, then call `answer(ticket, allowed)`.
    Prompt { ticket: u64, extension_id: String, need: PermissionSet },
    /// The call `ticket` is complete.
    Resolved { ticket: u64, granted: bool },
}

struct Pending {
    ticket: u64,
    manifest: Value,
    req: PermissionSet,
}

/// Optional permissions granted at runtime, persisted per extension in `{ [extensionId]: { permissions, origins } }`.
/// The effective permissions of an extension are the manifest's required ones plus these.
pub struct PermissionGrants {
    grants: HashMap<String, PermissionSet>,
    store: JsonStore,
    emitter: Arc<dyn Emitter>,
    changed: Vec<String>,
    queues: HashMap<String, VecDeque<Pending>>,
    /// The ticket whose dialog is open, per extension.
    open: HashMap<String, (u64, PermissionSet)>,
    next_ticket: u64,
}

impl PermissionGrants {
    pub fn new(file: impl AsRef<Path>, emitter: Arc<dyn Emitter>) -> Self {
        let store = JsonStore::new(file.as_ref());
        let mut grants = HashMap::new();
        if let Some(Value::Object(map)) = store.read() {
            for (id, g) in map {
                let set = PermissionSet { permissions: uniq(strings(g.get("permissions"))), origins: uniq(strings(g.get("origins"))) };
                if !set.is_empty() {
                    grants.insert(id, set);
                }
            }
        }
        PermissionGrants { grants, store, emitter, changed: Vec::new(), queues: HashMap::new(), open: HashMap::new(), next_ticket: 0 }
    }

    fn snapshot(&self) -> Value {
        Value::Object(self.grants.iter().map(|(k, v)| (k.clone(), v.to_json())).collect())
    }

    pub fn flush(&mut self) -> bool {
        let value = self.snapshot();
        self.store.flush(&value)
    }

    /// Ids whose grants changed since the last call (the DNR engine recomputes their host access).
    pub fn take_changed(&mut self) -> Vec<String> {
        std::mem::take(&mut self.changed)
    }

    fn changed(&mut self, id: &str) {
        self.store.request();
        self.changed.push(id.to_string());
    }

    pub fn permissions_of(&self, id: &str) -> Vec<String> {
        self.grants.get(id).map(|g| g.permissions.clone()).unwrap_or_default()
    }

    pub fn origins_of(&self, id: &str) -> Vec<String> {
        self.grants.get(id).map(|g| g.origins.clone()).unwrap_or_default()
    }

    /// Manifest requirements plus runtime grants.
    pub fn effective(&self, manifest: &Value, id: &str) -> PermissionSet {
        let d = declared_of(manifest);
        PermissionSet {
            permissions: uniq(d.required.permissions.into_iter().chain(self.permissions_of(id)).collect()),
            origins: uniq(d.required.origins.into_iter().chain(self.origins_of(id)).collect()),
        }
    }

    /// The part of `wanted` the extension does not hold yet.
    fn missing(&self, manifest: &Value, id: &str, wanted: &PermissionSet) -> PermissionSet {
        let held = self.effective(manifest, id);
        PermissionSet {
            permissions: wanted.permissions.iter().filter(|p| !held.permissions.contains(p)).cloned().collect(),
            origins: wanted.origins.iter().filter(|o| !covered_by_any(&held.origins, o)).cloned().collect(),
        }
    }

    pub fn contains(&self, manifest: &Value, id: &str, raw: &Value) -> ApiResult<bool> {
        Ok(self.missing(manifest, id, &parse_request(raw)?).is_empty())
    }

    pub fn get_all(&self, manifest: &Value, id: &str) -> PermissionSet {
        self.effective(manifest, id)
    }

    /// Validates, queues, and returns the ticket with the first steps. Requests of one extension are served one at a time.
    pub fn request(&mut self, extension_id: &str, manifest: &Value, raw: &Value) -> ApiResult<(u64, Vec<PermEvent>)> {
        let req = parse_request(raw)?;
        let d = declared_of(manifest);
        let declared_origins: Vec<String> = d.required.origins.iter().chain(d.optional.origins.iter()).cloned().collect();
        if req.permissions.iter().any(|p| !d.required.permissions.contains(p) && !d.optional.permissions.contains(p)) {
            return Err(ApiError::new(NOT_DECLARED));
        }
        if req.origins.iter().any(|o| !covered_by_any(&declared_origins, o)) {
            return Err(ApiError::new(NOT_DECLARED));
        }
        self.next_ticket += 1;
        let ticket = self.next_ticket;
        self.queues.entry(extension_id.to_string()).or_default().push_back(Pending { ticket, manifest: manifest.clone(), req });
        let events = self.advance(extension_id);
        Ok((ticket, events))
    }

    /// Runs the queue of an extension until a dialog is needed.
    fn advance(&mut self, id: &str) -> Vec<PermEvent> {
        let mut out = Vec::new();
        if self.open.contains_key(id) {
            return out;
        }
        while let Some(p) = self.queues.get_mut(id).and_then(|q| q.front()) {
            let (ticket, manifest, req) = (p.ticket, p.manifest.clone(), p.req.clone());
            // Recomputed here: an earlier request in the queue may have granted part of it.
            let need = self.missing(&manifest, id, &req);
            if need.is_empty() {
                self.queues.get_mut(id).map(|q| q.pop_front());
                out.push(PermEvent::Resolved { ticket, granted: true });
                continue;
            }
            self.open.insert(id.to_string(), (ticket, need.clone()));
            out.push(PermEvent::Prompt { ticket, extension_id: id.to_string(), need });
            break;
        }
        if self.queues.get(id).is_some_and(|q| q.is_empty()) {
            self.queues.remove(id);
        }
        out
    }

    /// The user's answer to a `Prompt`. Unknown tickets are ignored.
    pub fn answer(&mut self, ticket: u64, allowed: bool) -> Vec<PermEvent> {
        let Some(id) = self.open.iter().find(|(_, (t, _))| *t == ticket).map(|(id, _)| id.clone()) else { return vec![] };
        let (_, need) = self.open.remove(&id).expect("open prompt");
        if let Some(q) = self.queues.get_mut(&id) {
            q.pop_front();
        }
        let mut out = vec![PermEvent::Resolved { ticket, granted: allowed }];
        if allowed {
            let mut set = self.grants.remove(&id).unwrap_or_default();
            set.permissions = uniq(set.permissions.into_iter().chain(need.permissions.iter().cloned()).collect());
            set.origins = uniq(set.origins.into_iter().chain(need.origins.iter().cloned()).collect());
            self.grants.insert(id.clone(), set);
            self.changed(&id);
            self.emitter.emit(&id, "permissions.onAdded", vec![need.to_json()], true, true);
        }
        out.extend(self.advance(&id));
        out
    }

    /// The dialog could not be shown (failure): same as a denial, the queue keeps going.
    pub fn abort(&mut self, ticket: u64) -> Vec<PermEvent> {
        self.answer(ticket, false)
    }

    /// Required items cannot be removed; optional grants are dropped. Removing what is not held succeeds, as in Chrome.
    pub fn remove(&mut self, manifest: &Value, id: &str, raw: &Value) -> ApiResult<bool> {
        let req = parse_request(raw)?;
        let d = declared_of(manifest);
        if req.permissions.iter().any(|p| d.required.permissions.contains(p)) || req.origins.iter().any(|o| covered_by_any(&d.required.origins, o)) {
            return Err(ApiError::new(REQUIRED));
        }
        let Some(set) = self.grants.get_mut(id) else { return Ok(true) };
        let removed = PermissionSet {
            permissions: set.permissions.iter().filter(|p| req.permissions.contains(p)).cloned().collect(),
            origins: set.origins.iter().filter(|o| req.origins.contains(o)).cloned().collect(),
        };
        if removed.is_empty() {
            return Ok(true);
        }
        set.permissions.retain(|p| !removed.permissions.contains(p));
        set.origins.retain(|o| !removed.origins.contains(o));
        if set.is_empty() {
            self.grants.remove(id);
        }
        self.changed(id);
        self.emitter.emit(id, "permissions.onRemoved", vec![removed.to_json()], true, true);
        Ok(true)
    }

    /// The extension (re)loaded: grants for items its manifest no longer lists as optional are dropped.
    pub fn reconcile(&mut self, id: &str, manifest: &Value) {
        let Some(set) = self.grants.get(id) else { return };
        let d = declared_of(manifest);
        let keep = PermissionSet {
            permissions: set.permissions.iter().filter(|p| d.optional.permissions.contains(p) && !d.required.permissions.contains(p)).cloned().collect(),
            origins: set
                .origins
                .iter()
                .filter(|o| covered_by_any(&d.optional.origins, o) && !covered_by_any(&d.required.origins, o))
                .cloned()
                .collect(),
        };
        if keep.permissions.len() == set.permissions.len() && keep.origins.len() == set.origins.len() {
            return;
        }
        if keep.is_empty() {
            self.grants.remove(id);
        } else {
            self.grants.insert(id.to_string(), keep);
        }
        self.changed(id);
    }

    /// Ids with grants.
    pub fn known_ids(&self) -> Vec<String> {
        self.grants.keys().cloned().collect()
    }

    /// The extension is uninstalled.
    pub fn drop_extension(&mut self, id: &str) {
        if self.grants.remove(id).is_some() {
            self.changed(id);
        }
    }
}

// ---- install dialog ----

/// Permissions with no particular risk: not shown in the install dialog.
pub const QUIET_PERMISSIONS: [&str; 28] = [
    "storage", "unlimitedStorage", "activeTab", "alarms", "contextMenus", "scripting", "idle", "offscreen", "sidePanel", "webNavigation",
    "fontSettings", "search", "tts", "ttsEngine", "power", "system.display", "system.cpu", "system.memory", "favicon", "readingList",
    "sessions", "declarativeContent", "webAuthenticationProxy", "gcm", "background", "action", "commands", "i18n",
];

const ALL_SITES: [&str; 7] = ["<all_urls>", "*://*/*", "http://*/*", "https://*/*", "*://*/", "http://*/", "https://*/"];

/// One line of the install / request dialog. The UI maps the keys to translated sentences
/// (`dialog.extension.perm.<key>`), the way the Electron app did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionLine {
    /// "Read and change your data on all websites".
    AllSites,
    /// Hosts named (at most three), and how many more there are.
    Hosts { hosts: Vec<String>, more: usize },
    /// A known permission: its translation key suffix (`tabs`, `history`, `webRequest`...).
    Known(&'static str),
    /// A permission with no sentence: shown as is.
    Unknown(String),
}

fn known_key(name: &str) -> Option<&'static str> {
    Some(match name {
        "tabs" => "tabs",
        "history" => "history",
        "topSites" => "topSites",
        "bookmarks" => "bookmarks",
        "cookies" => "cookies",
        "webRequest" | "webRequestBlocking" => "webRequest",
        "declarativeNetRequest" | "declarativeNetRequestWithHostAccess" => "declarativeNetRequest",
        "downloads" => "downloads",
        "clipboardRead" => "clipboardRead",
        "clipboardWrite" => "clipboardWrite",
        "notifications" => "notifications",
        "nativeMessaging" => "nativeMessaging",
        "geolocation" => "geolocation",
        "management" => "management",
        "proxy" => "proxy",
        "debugger" => "debugger",
        "privacy" => "privacy",
        "identity" => "identity",
        "desktopCapture" | "tabCapture" => "desktopCapture",
        "pageCapture" => "pageCapture",
        "browsingData" => "browsingData",
        _ => return None,
    })
}

/// Readable lines for what an extension asks: site access first, then known permissions, then unknown names as is.
pub fn describe_access(names: &[String], patterns: &[String]) -> Vec<PermissionLine> {
    let mut out = Vec::new();
    let mut uniq_patterns: Vec<&String> = Vec::new();
    for p in patterns {
        if !uniq_patterns.contains(&p) {
            uniq_patterns.push(p);
        }
    }
    if uniq_patterns.iter().any(|p| ALL_SITES.contains(&p.as_str())) {
        out.push(PermissionLine::AllSites);
    } else if !uniq_patterns.is_empty() {
        let mut hosts: Vec<String> = Vec::new();
        for p in &uniq_patterns {
            let rest = p.split_once("://").map(|(_, r)| r).unwrap_or(p);
            let host = rest.split('/').next().unwrap_or("").trim_start_matches("*.").to_string();
            if !host.is_empty() && !hosts.contains(&host) {
                hosts.push(host);
            }
        }
        let more = hosts.len().saturating_sub(3);
        hosts.truncate(3);
        out.push(PermissionLine::Hosts { hosts, more });
    }
    let mut seen: Vec<&'static str> = Vec::new();
    let mut unknown = Vec::new();
    for name in names {
        if name.contains("://") || name == "<all_urls>" || QUIET_PERMISSIONS.contains(&name.as_str()) {
            continue;
        }
        match known_key(name) {
            Some(k) => {
                if !seen.contains(&k) {
                    seen.push(k);
                    out.push(PermissionLine::Known(k));
                }
            }
            None => unknown.push(PermissionLine::Unknown(name.clone())),
        }
    }
    out.extend(unknown);
    out
}

/// The lines of the install dialog for a manifest (permissions, host permissions and content script matches).
pub fn describe_manifest(manifest: &Value) -> Vec<PermissionLine> {
    let names = strings(manifest.get("permissions"));
    let mut patterns: Vec<String> = strings(manifest.get("host_permissions"));
    patterns.extend(names.iter().filter(|p| p.contains("://") || *p == "<all_urls>").cloned());
    for cs in manifest.get("content_scripts").and_then(|c| c.as_array()).into_iter().flatten() {
        patterns.extend(strings(cs.get("matches")));
    }
    describe_access(&names, &patterns)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::Outbox;
    use serde_json::json;

    const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    fn manifest() -> Value {
        json!({
            "permissions": ["storage", "alarms"],
            "optional_permissions": ["cookies", "nativeMessaging"],
            "host_permissions": ["https://required.example/*"],
            "optional_host_permissions": ["*://*/*"]
        })
    }

    fn make(dir: &Path) -> (PermissionGrants, Arc<Outbox>) {
        let out = Arc::new(Outbox::new());
        (PermissionGrants::new(dir.join("extension-permissions.json"), out.clone()), out)
    }

    /// Request then allow the dialog if there is one, returning the final answer.
    fn ask(g: &mut PermissionGrants, id: &str, m: &Value, req: Value, allow: bool) -> bool {
        let (ticket, mut events) = g.request(id, m, &req).unwrap();
        loop {
            let mut next = Vec::new();
            for e in events {
                match e {
                    PermEvent::Resolved { ticket: t, granted } if t == ticket => return granted,
                    PermEvent::Prompt { ticket: t, .. } => next.extend(g.answer(t, allow)),
                    _ => {}
                }
            }
            events = next;
        }
    }

    #[test]
    fn declared_of_splits_hosts_from_permissions() {
        let d = declared_of(&json!({ "permissions": ["tabs", "https://x.example/*"], "optional_permissions": ["<all_urls>", "cookies"] }));
        assert_eq!(d.required, PermissionSet { permissions: vec!["tabs".into()], origins: vec!["https://x.example/*".into()] });
        assert_eq!(d.optional, PermissionSet { permissions: vec!["cookies".into()], origins: vec!["<all_urls>".into()] });
    }

    #[test]
    fn parse_request_rejects_malformed_requests() {
        assert!(parse_request(&Value::Null).unwrap_err().0.contains("Invalid arguments"));
        assert!(parse_request(&json!({ "permissions": "tabs" })).unwrap_err().0.contains("Invalid arguments"));
        assert!(parse_request(&json!({ "origins": ["nonsense"] })).unwrap_err().0.contains("Invalid host permission pattern"));
        assert_eq!(parse_request(&json!({ "permissions": ["a", "a"] })).unwrap(), PermissionSet { permissions: vec!["a".into()], origins: vec![] });
    }

    #[test]
    fn request_refuses_what_the_manifest_does_not_declare() {
        let dir = tempfile::tempdir().unwrap();
        let (mut g, _) = make(dir.path());
        assert_eq!(g.request(A, &manifest(), &json!({ "permissions": ["history"] })).unwrap_err().0, NOT_DECLARED);
        let narrow = json!({ "optional_host_permissions": ["https://only.example/*"] });
        assert!(g.request(A, &narrow, &json!({ "origins": ["https://other.example/*"] })).unwrap_err().0.contains("Only permissions specified"));
    }

    #[test]
    fn request_resolves_true_without_a_dialog_when_held() {
        let dir = tempfile::tempdir().unwrap();
        let (mut g, _) = make(dir.path());
        let (t, events) = g.request(A, &manifest(), &json!({ "permissions": ["storage"], "origins": ["https://required.example/page"] })).unwrap();
        assert_eq!(events, vec![PermEvent::Resolved { ticket: t, granted: true }]);
    }

    #[test]
    fn request_asks_for_new_items_only_stores_and_emits() {
        let dir = tempfile::tempdir().unwrap();
        let (mut g, out) = make(dir.path());
        let (t, events) = g
            .request(A, &manifest(), &json!({ "permissions": ["storage", "cookies"], "origins": ["https://required.example/*", "https://new.example/*"] }))
            .unwrap();
        let need = PermissionSet { permissions: vec!["cookies".into()], origins: vec!["https://new.example/*".into()] };
        assert_eq!(events, vec![PermEvent::Prompt { ticket: t, extension_id: A.into(), need: need.clone() }]);
        assert_eq!(g.answer(t, true), vec![PermEvent::Resolved { ticket: t, granted: true }]);
        let ev = out.drain();
        assert_eq!(ev.len(), 1);
        assert_eq!((ev[0].name.as_str(), ev[0].wake), ("permissions.onAdded", true));
        assert_eq!(ev[0].args, vec![need.to_json()]);
        assert_eq!(g.permissions_of(A), vec!["cookies"]);
        assert_eq!(g.origins_of(A), vec!["https://new.example/*"]);
    }

    #[test]
    fn denied_request_stores_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let (mut g, out) = make(dir.path());
        assert!(!ask(&mut g, A, &manifest(), json!({ "permissions": ["cookies"] }), false));
        assert!(g.permissions_of(A).is_empty());
        assert!(out.drain().is_empty());
    }

    #[test]
    fn requests_of_one_extension_are_queued() {
        let dir = tempfile::tempdir().unwrap();
        let (mut g, _) = make(dir.path());
        let (first, e1) = g.request(A, &manifest(), &json!({ "permissions": ["cookies"] })).unwrap();
        let (second, e2) = g.request(A, &manifest(), &json!({ "permissions": ["cookies", "nativeMessaging"] })).unwrap();
        assert_eq!(e1.len(), 1);
        assert!(e2.is_empty(), "no second dialog while the first is open");
        let after = g.answer(first, true);
        assert_eq!(after[0], PermEvent::Resolved { ticket: first, granted: true });
        // The second only asks for what is still missing.
        assert_eq!(
            after[1],
            PermEvent::Prompt { ticket: second, extension_id: A.into(), need: PermissionSet { permissions: vec!["nativeMessaging".into()], origins: vec![] } }
        );
        assert_eq!(g.answer(second, true)[0], PermEvent::Resolved { ticket: second, granted: true });
    }

    #[test]
    fn an_aborted_dialog_does_not_block_the_queue() {
        let dir = tempfile::tempdir().unwrap();
        let (mut g, _) = make(dir.path());
        let (t, _) = g.request(A, &manifest(), &json!({ "permissions": ["cookies"] })).unwrap();
        assert_eq!(g.abort(t), vec![PermEvent::Resolved { ticket: t, granted: false }]);
        assert!(ask(&mut g, A, &manifest(), json!({ "permissions": ["cookies"] }), true));
    }

    #[test]
    fn contains_and_get_all_count_required_grants_and_covered_origins() {
        let dir = tempfile::tempdir().unwrap();
        let (mut g, _) = make(dir.path());
        let m = manifest();
        assert!(!g.contains(&m, A, &json!({ "permissions": ["cookies"] })).unwrap());
        assert!(g.contains(&m, A, &json!({ "permissions": ["storage"], "origins": ["https://required.example/x"] })).unwrap());
        ask(&mut g, A, &m, json!({ "permissions": ["cookies"], "origins": ["*://*/*"] }), true);
        assert!(g.contains(&m, A, &json!({ "permissions": ["cookies", "alarms"], "origins": ["https://anything.example/*"] })).unwrap());
        assert_eq!(
            g.get_all(&m, A),
            PermissionSet { permissions: vec!["storage".into(), "alarms".into(), "cookies".into()], origins: vec!["https://required.example/*".into(), "*://*/*".into()] }
        );
        assert_eq!(g.get_all(&m, B).permissions, vec!["storage", "alarms"]);
    }

    #[test]
    fn remove_rejects_required_and_deletes_optional() {
        let dir = tempfile::tempdir().unwrap();
        let (mut g, out) = make(dir.path());
        let m = manifest();
        ask(&mut g, A, &m, json!({ "permissions": ["cookies", "nativeMessaging"] }), true);
        out.drain();
        assert_eq!(g.remove(&m, A, &json!({ "permissions": ["storage"] })).unwrap_err().0, REQUIRED);
        assert_eq!(g.remove(&m, A, &json!({ "origins": ["https://required.example/*"] })).unwrap_err().0, REQUIRED);
        assert_eq!(g.remove(&m, A, &json!({ "permissions": ["cookies", "alarms"] })).unwrap_err().0, REQUIRED);
        assert_eq!(g.permissions_of(A), vec!["cookies", "nativeMessaging"]);
        assert!(out.drain().is_empty());
        assert!(g.remove(&m, A, &json!({ "permissions": ["cookies", "history"] })).unwrap());
        assert_eq!(g.permissions_of(A), vec!["nativeMessaging"]);
        let ev = out.drain();
        assert_eq!(ev[0].name, "permissions.onRemoved");
        assert_eq!(ev[0].args, vec![json!({ "permissions": ["cookies"], "origins": [] })]);
        assert!(g.remove(&m, A, &json!({ "permissions": ["cookies"] })).unwrap());
        assert!(out.drain().is_empty());
    }

    #[test]
    fn persistence_writes_only_runtime_grants_and_reads_them_back() {
        let dir = tempfile::tempdir().unwrap();
        let (mut g, _) = make(dir.path());
        ask(&mut g, A, &manifest(), json!({ "permissions": ["cookies"], "origins": ["https://new.example/*"] }), true);
        assert!(g.flush());
        let file = dir.path().join("extension-permissions.json");
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(v, json!({ A: { "permissions": ["cookies"], "origins": ["https://new.example/*"] } }));
        let (again, _) = make(dir.path());
        assert_eq!(again.permissions_of(A), vec!["cookies"]);
        assert_eq!(again.origins_of(A), vec!["https://new.example/*"]);
    }

    #[test]
    fn a_corrupt_file_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("extension-permissions.json"), "{nope").unwrap();
        let (g, _) = make(dir.path());
        assert!(g.permissions_of(A).is_empty());
    }

    #[test]
    fn drop_removes_grants_and_notifies_once() {
        let dir = tempfile::tempdir().unwrap();
        let (mut g, _) = make(dir.path());
        ask(&mut g, A, &manifest(), json!({ "permissions": ["cookies"] }), true);
        g.take_changed();
        g.drop_extension(A);
        assert_eq!(g.take_changed(), vec![A]);
        assert!(g.permissions_of(A).is_empty());
        g.flush();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(dir.path().join("extension-permissions.json")).unwrap()).unwrap();
        assert_eq!(v, json!({}));
        g.drop_extension(A);
        assert!(g.take_changed().is_empty());
    }

    #[test]
    fn reconcile_drops_grants_no_longer_optional() {
        let dir = tempfile::tempdir().unwrap();
        let (mut g, _) = make(dir.path());
        let m = manifest();
        ask(&mut g, A, &m, json!({ "permissions": ["cookies", "nativeMessaging"], "origins": ["https://new.example/*"] }), true);
        g.take_changed();
        g.reconcile(A, &m);
        assert!(g.take_changed().is_empty());
        g.reconcile(A, &json!({ "permissions": ["nativeMessaging"], "optional_permissions": ["cookies"], "optional_host_permissions": ["https://other.example/*"] }));
        assert_eq!(g.take_changed(), vec![A]);
        assert_eq!(g.permissions_of(A), vec!["cookies"]);
        assert!(g.origins_of(A).is_empty());
        g.reconcile(A, &json!({}));
        assert!(g.permissions_of(A).is_empty());
        g.flush();
        assert!(dir.path().join("extension-permissions.json").exists());
    }

    fn names(l: &[&str]) -> Vec<String> {
        l.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn install_dialog_lines() {
        assert_eq!(describe_access(&names(&["storage", "alarms"]), &names(&[])), vec![]);
        assert_eq!(describe_access(&names(&["tabs", "webRequestBlocking", "webRequest", "weird"]), &names(&["<all_urls>"])), vec![
            PermissionLine::AllSites, PermissionLine::Known("tabs"), PermissionLine::Known("webRequest"), PermissionLine::Unknown("weird".into())
        ]);
        let l = describe_access(&names(&[]), &names(&["https://*.a.com/*", "https://b.org/x", "http://c.net/*", "https://d.io/*", "https://a.com/*"]));
        assert_eq!(l, vec![PermissionLine::Hosts { hosts: vec!["a.com".into(), "b.org".into(), "c.net".into()], more: 1 }]);
        let m = json!({ "permissions": ["cookies", "https://x.test/*"], "host_permissions": ["https://y.test/*"], "content_scripts": [{ "matches": ["https://z.test/*"] }] });
        assert_eq!(describe_manifest(&m), vec![PermissionLine::Hosts { hosts: vec!["y.test".into(), "x.test".into(), "z.test".into()], more: 0 }, PermissionLine::Known("cookies")]);
    }
}

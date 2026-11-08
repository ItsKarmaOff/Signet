//! Call routing and event delivery. Port of `apis/router.ts`.
//!
//! Every call arrives with a sender (a service worker or an extension page); the web layer derives the extension id from
//! it ([`extension_of_scope`], [`extension_of_page`]) and never from the arguments. [`PermissionTable`] says which manifest
//! permission each method needs and [`check_call`] applies it. [`EventRouter`] decides how an event reaches an extension:
//! to its running contexts at once, or by starting its worker first, retrying with backoff when the worker will not start.

use crate::common::{ApiError, ApiResult, OutEvent};
use regex::Regex;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

fn scope_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^chrome-extension://([a-p]{32})/$").expect("static regex"))
}

fn page_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^chrome-extension://([a-p]{32})/").expect("static regex"))
}

/// Extension id of a service worker scope, or None for anything else.
pub fn extension_of_scope(scope: &str) -> Option<String> {
    scope_re().captures(scope).map(|c| c[1].to_string())
}

/// Extension id of a page URL, or None if it is not an extension page.
pub fn extension_of_page(url: &str) -> Option<String> {
    page_re().captures(url).map(|c| c[1].to_string())
}

/// Declared in the manifest `permissions`, or among the `granted` ones (optional permissions the user allowed).
pub fn has_permission(manifest: &Value, permission: &str, granted: &[String]) -> bool {
    manifest.get("permissions").and_then(|p| p.as_array()).is_some_and(|l| l.iter().any(|v| v.as_str() == Some(permission))) || granted.iter().any(|g| g == permission)
}

/// Which manifest permission unlocks each method (any of them; empty = no permission needed).
#[derive(Default)]
pub struct PermissionTable {
    entries: HashMap<String, Vec<String>>,
}

impl PermissionTable {
    /// Declares `method` (e.g. `alarms.create`), usable by extensions with one of `permissions`.
    pub fn register(&mut self, permissions: &[&str], method: &str) {
        debug_assert!(!self.entries.contains_key(method), "Extension API {method} already registered");
        self.entries.insert(method.to_string(), permissions.iter().filter(|p| !p.is_empty()).map(|p| p.to_string()).collect());
    }

    /// Every registered method name, sorted.
    pub fn methods(&self) -> Vec<String> {
        let mut v: Vec<String> = self.entries.keys().cloned().collect();
        v.sort();
        v
    }

    pub fn knows(&self, method: &str) -> bool {
        self.entries.contains_key(method)
    }

    pub fn permissions_of(&self, method: &str) -> Option<&[String]> {
        self.entries.get(method).map(|v| v.as_slice())
    }
}

/// Checks a call: the method exists, the extension is loaded and holds a permission that unlocks it, the arguments are a list.
pub fn check_call(table: &PermissionTable, method: &Value, args: &Value, manifest: Option<&Value>, granted: &[String]) -> ApiResult<()> {
    let name = method.as_str().ok_or_else(|| ApiError::new(format!("Unknown Extension API - {method}")))?;
    let Some(perms) = table.permissions_of(name) else { return Err(ApiError::new(format!("Unknown Extension API - {name}"))) };
    let Some(manifest) = manifest else { return Err(ApiError::new("Extension is not loaded.")) };
    if !perms.is_empty() && !perms.iter().any(|p| has_permission(manifest, p, granted)) {
        return Err(ApiError::new(format!("Access to extension API denied: the '{}' permission is required.", perms.join("' or '"))));
    }
    if !args.is_array() {
        return Err(ApiError::new("Invalid arguments."));
    }
    Ok(())
}

/// Wait before retrying to start a worker that failed (then the event stays queued until it runs by itself).
pub const WAKE_BACKOFF_MS: [u64; 3] = [1000, 2000, 5000];
/// A freshly running worker needs a moment to evaluate its script and register listeners.
pub const SETTLE_MS: u64 = 300;
pub const MAX_QUEUED: usize = 50;

/// What the web layer must do.
#[derive(Debug, Clone, PartialEq)]
pub enum RouterAction {
    /// Send the event to every running context of the extension (worker, pages, extension subframes).
    DeliverRunning { extension_id: String, name: String, args: Vec<Value> },
    /// Start the extension's worker, then call `wake_result(ticket, ok)`.
    StartWorker { extension_id: String, ticket: u64 },
    /// The worker is up: send these events to it, in order.
    DeliverToWorker { extension_id: String, events: Vec<(String, Vec<Value>)> },
}

#[derive(Default)]
struct Queued {
    events: Vec<(String, Vec<Value>)>,
    attempt: usize,
    retry_at: Option<u64>,
    flush_at: Option<u64>,
}

/// Events for extensions. Time is a parameter (`now_ms`).
#[derive(Default)]
pub struct EventRouter {
    /// Running worker versions per extension.
    workers: HashMap<String, HashSet<String>>,
    queued: HashMap<String, Queued>,
    /// Events waiting for a worker that was asked to start: ticket to (extension, event, queue allowed).
    waking: HashMap<u64, (String, String, Vec<Value>, bool)>,
    next_ticket: u64,
    log: Vec<String>,
}

impl EventRouter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn take_log(&mut self) -> Vec<String> {
        std::mem::take(&mut self.log)
    }

    /// A worker of the extension started (`worker_id` is the engine's version id).
    pub fn worker_running(&mut self, extension_id: &str, worker_id: &str, now_ms: u64) {
        self.workers.entry(extension_id.to_string()).or_default().insert(worker_id.to_string());
        // A worker that is up can take the events it missed (after it has had time to register its listeners).
        if let Some(q) = self.queued.get_mut(extension_id) {
            q.flush_at = Some(now_ms + SETTLE_MS);
        }
    }

    pub fn worker_stopped(&mut self, extension_id: &str, worker_id: &str) {
        if let Some(s) = self.workers.get_mut(extension_id) {
            s.remove(worker_id);
            if s.is_empty() {
                self.workers.remove(extension_id);
            }
        }
    }

    /// Running service worker contexts of an extension (for `runtime.getContexts`).
    pub fn running_workers(&self, extension_id: &str) -> usize {
        self.workers.get(extension_id).map_or(0, |s| s.len())
    }

    /// The extension is unloaded everywhere: nothing is kept for it.
    pub fn forget(&mut self, extension_id: &str) {
        self.workers.remove(extension_id);
        self.queued.remove(extension_id);
        self.waking.retain(|_, (e, ..)| e != extension_id);
    }

    /// Delivers an event: to running contexts at once, and when `wake` is set by starting a stopped worker first.
    pub fn emit(&mut self, ev: OutEvent) -> Vec<RouterAction> {
        let mut out = vec![RouterAction::DeliverRunning { extension_id: ev.extension_id.clone(), name: ev.name.clone(), args: ev.args.clone() }];
        if self.running_workers(&ev.extension_id) == 0 && ev.wake {
            self.next_ticket += 1;
            let ticket = self.next_ticket;
            self.waking.insert(ticket, (ev.extension_id.clone(), ev.name, ev.args, ev.queue));
            out.push(RouterAction::StartWorker { extension_id: ev.extension_id, ticket });
        }
        out
    }

    /// The web layer's answer to `StartWorker`.
    pub fn wake_result(&mut self, ticket: u64, ok: bool, now_ms: u64) -> Vec<RouterAction> {
        let Some((ext, name, args, queue)) = self.waking.remove(&ticket) else { return vec![] };
        if ok {
            let mut events = self.queued.remove(&ext).map(|q| q.events).unwrap_or_default();
            events.push((name, args));
            return vec![RouterAction::DeliverToWorker { extension_id: ext, events }];
        }
        if queue {
            self.enqueue(&ext, name, args, now_ms);
        } else {
            self.log.push(format!("Extension {ext}: cannot wake the worker for {name}"));
        }
        vec![]
    }

    /// The worker would not start (typical at launch, right after the extension loads): keep the event and try again.
    fn enqueue(&mut self, ext: &str, name: String, args: Vec<Value>, now_ms: u64) {
        let fresh = !self.queued.contains_key(ext);
        if fresh {
            self.log.push(format!("[extension-apis] {ext} worker not up yet, queueing {name} (will retry)"));
        }
        let q = self.queued.entry(ext.to_string()).or_default();
        if q.events.len() < MAX_QUEUED {
            q.events.push((name, args));
        }
        Self::schedule_retry(q, now_ms);
    }

    fn schedule_retry(q: &mut Queued, now_ms: u64) {
        if q.retry_at.is_some() || q.attempt >= WAKE_BACKOFF_MS.len() {
            return;
        }
        q.retry_at = Some(now_ms + WAKE_BACKOFF_MS[q.attempt]);
    }

    /// Runs the retries and the flushes that are due. `loaded` says whether an extension is still loaded.
    pub fn tick(&mut self, now_ms: u64, loaded: &dyn Fn(&str) -> bool) -> Vec<RouterAction> {
        let mut out = Vec::new();
        let ids: Vec<String> = self.queued.keys().cloned().collect();
        for id in ids {
            if !loaded(&id) {
                self.queued.remove(&id);
                continue;
            }
            let Some(q) = self.queued.get_mut(&id) else { continue };
            if q.retry_at.is_some_and(|t| t <= now_ms) {
                q.retry_at = None;
                q.attempt += 1;
                if self.workers.contains_key(&id) {
                    q.flush_at = Some(now_ms + SETTLE_MS);
                } else {
                    // The retry asks for the worker again; the answer re-enters through `retry_result`.
                    self.next_ticket += 1;
                    let ticket = self.next_ticket;
                    self.waking.insert(ticket, (id.clone(), String::new(), vec![], true));
                    out.push(RouterAction::StartWorker { extension_id: id.clone(), ticket });
                }
            }
            let Some(q) = self.queued.get_mut(&id) else { continue };
            if q.flush_at.is_some_and(|t| t <= now_ms) && self.workers.contains_key(&id) {
                let q = self.queued.remove(&id).expect("queued");
                self.log.push(format!("[extension-apis] {id} worker is up, delivered {} queued event(s)", q.events.len()));
                out.push(RouterAction::DeliverToWorker { extension_id: id, events: q.events });
            }
        }
        out
    }

    /// The answer to a retry `StartWorker`: a worker that started takes the queue after the settle delay; a failure
    /// schedules the next attempt (until the backoff list is used up).
    pub fn retry_result(&mut self, ticket: u64, ok: bool, now_ms: u64) {
        let Some((ext, name, ..)) = self.waking.remove(&ticket) else { return };
        if !name.is_empty() {
            return;
        }
        if let Some(q) = self.queued.get_mut(&ext) {
            if ok {
                q.flush_at = Some(now_ms + SETTLE_MS);
            } else {
                Self::schedule_retry(q, now_ms);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    fn ev(id: &str, name: &str, wake: bool, queue: bool) -> OutEvent {
        OutEvent { extension_id: id.into(), name: name.into(), args: vec![json!({ "name": "x" })], wake, queue }
    }

    #[test]
    fn extracts_ids_only_from_well_formed_extension_urls() {
        assert_eq!(extension_of_scope(&format!("chrome-extension://{A}/")).as_deref(), Some(A));
        assert_eq!(extension_of_scope(&format!("chrome-extension://{A}/sub/")), None);
        assert_eq!(extension_of_scope("https://example.com/"), None);
        assert_eq!(extension_of_page(&format!("chrome-extension://{A}/popup.html")).as_deref(), Some(A));
        assert_eq!(extension_of_page(&format!("https://evil.com/chrome-extension://{A}/")), None);
        assert_eq!(extension_of_page(""), None);
    }

    #[test]
    fn has_permission_reads_the_manifest_and_the_grants() {
        assert!(has_permission(&json!({ "permissions": ["alarms"] }), "alarms", &[]));
        assert!(!has_permission(&json!({ "host_permissions": ["alarms"] }), "alarms", &[]));
        assert!(!has_permission(&Value::Null, "alarms", &[]));
        assert!(!has_permission(&json!({ "optional_permissions": ["alarms"] }), "alarms", &[]));
        assert!(has_permission(&json!({ "optional_permissions": ["alarms"] }), "alarms", &["alarms".to_string()]));
    }

    fn table() -> PermissionTable {
        let mut t = PermissionTable::default();
        t.register(&["alarms"], "alarms.get");
        t.register(&["declarativeNetRequest", "declarativeNetRequestWithHostAccess"], "dnr.x");
        t.register(&[""], "open.x");
        t
    }

    #[test]
    fn check_call_enforces_the_permission() {
        let t = table();
        let ok = json!({ "permissions": ["alarms"] });
        assert!(check_call(&t, &json!("alarms.get"), &json!([]), Some(&ok), &[]).is_ok());
        let other = json!({ "permissions": ["storage"] });
        let err = check_call(&t, &json!("alarms.get"), &json!([]), Some(&other), &[]).unwrap_err();
        assert!(err.0.contains("denied") && err.0.contains("'alarms'"));
    }

    #[test]
    fn any_of_several_permissions_and_none_when_the_list_is_empty() {
        let t = table();
        let host = json!({ "permissions": ["declarativeNetRequestWithHostAccess"] });
        assert!(check_call(&t, &json!("dnr.x"), &json!([]), Some(&host), &[]).is_ok());
        assert!(check_call(&t, &json!("dnr.x"), &json!([]), Some(&json!({ "permissions": [] })), &[]).is_err());
        assert!(check_call(&t, &json!("open.x"), &json!([]), Some(&json!({ "permissions": [] })), &[]).is_ok());
    }

    #[test]
    fn refuses_unknown_methods_unloaded_extensions_and_bad_args() {
        let t = table();
        let m = json!({ "permissions": ["alarms"] });
        assert!(check_call(&t, &json!("nope"), &json!([]), Some(&m), &[]).is_err());
        assert!(check_call(&t, &json!(5), &json!([]), Some(&m), &[]).is_err());
        assert!(check_call(&t, &json!("alarms.get"), &json!([]), None, &[]).unwrap_err().0.contains("not loaded"));
        assert!(check_call(&t, &json!("alarms.get"), &json!("x"), Some(&m), &[]).is_err());
    }

    #[test]
    fn runtime_grants_unlock_an_optional_permission_without_a_reload() {
        let t = table();
        let m = json!({ "permissions": [], "optional_permissions": ["alarms"] });
        assert!(check_call(&t, &json!("alarms.get"), &json!([]), Some(&m), &[]).unwrap_err().0.contains("'alarms' permission is required"));
        assert!(check_call(&t, &json!("alarms.get"), &json!([]), Some(&m), &["alarms".to_string()]).is_ok());
    }

    #[test]
    fn running_contexts_get_the_event_and_a_stopped_worker_is_woken_only_when_asked() {
        let mut r = EventRouter::new();
        let a = r.emit(ev(A, "alarms.onAlarm", false, true));
        assert_eq!(a.len(), 1);
        assert!(matches!(&a[0], RouterAction::DeliverRunning { name, .. } if name == "alarms.onAlarm"));
        let b = r.emit(ev(A, "alarms.onAlarm", true, true));
        assert!(matches!(&b[1], RouterAction::StartWorker { extension_id, .. } if extension_id == A));
        // Once a worker runs, no wake is needed.
        r.worker_running(A, "7", 0);
        let c = r.emit(ev(A, "alarms.onAlarm", true, true));
        assert_eq!(c.len(), 1);
        assert_eq!(r.running_workers(A), 1);
        r.worker_stopped(A, "7");
        assert_eq!(r.running_workers(A), 0);
    }

    #[test]
    fn a_woken_worker_receives_the_event_with_what_was_queued() {
        let mut r = EventRouter::new();
        let RouterAction::StartWorker { ticket, .. } = r.emit(ev(A, "e", true, true))[1].clone() else { panic!() };
        let out = r.wake_result(ticket, true, 0);
        assert_eq!(out, vec![RouterAction::DeliverToWorker { extension_id: A.into(), events: vec![("e".into(), vec![json!({ "name": "x" })])] }]);
    }

    #[test]
    fn keeps_an_event_when_the_worker_will_not_start_retries_with_backoff_and_delivers_once_it_runs() {
        let mut r = EventRouter::new();
        let loaded = |_: &str| true;
        let RouterAction::StartWorker { ticket, .. } = r.emit(ev(A, "alarms.onAlarm", true, true))[1].clone() else { panic!() };
        assert!(r.wake_result(ticket, false, 0).is_empty());
        assert!(r.take_log()[0].contains("queueing"));
        assert!(r.tick(999, &loaded).is_empty());
        // First retry after 1 s.
        let out = r.tick(1000, &loaded);
        let RouterAction::StartWorker { ticket: retry, .. } = out[0].clone() else { panic!() };
        r.retry_result(retry, false, 1000);
        // It runs by itself: the queued event reaches it after the settle delay.
        r.worker_running(A, "3", 1500);
        assert!(r.tick(1799, &loaded).is_empty());
        let out = r.tick(1800, &loaded);
        assert_eq!(out, vec![RouterAction::DeliverToWorker { extension_id: A.into(), events: vec![("alarms.onAlarm".into(), vec![json!({ "name": "x" })])] }]);
        // Delivered once only.
        assert!(r.tick(20_000, &loaded).is_empty());
    }

    #[test]
    fn time_sensitive_events_are_not_queued() {
        let mut r = EventRouter::new();
        let RouterAction::StartWorker { ticket, .. } = r.emit(ev(A, "webRequest.onAuthRequired", true, false))[1].clone() else { panic!() };
        assert!(r.wake_result(ticket, false, 0).is_empty());
        assert!(r.tick(10_000, &|_| true).is_empty());
    }

    #[test]
    fn retries_stop_after_the_backoff_list_and_unloaded_extensions_are_dropped() {
        let mut r = EventRouter::new();
        let RouterAction::StartWorker { ticket, .. } = r.emit(ev(A, "e", true, true))[1].clone() else { panic!() };
        r.wake_result(ticket, false, 0);
        let mut now = 0;
        let mut starts = 0;
        for _ in 0..10 {
            now += 6000;
            for a in r.tick(now, &|_| true) {
                if let RouterAction::StartWorker { ticket, .. } = a {
                    starts += 1;
                    r.retry_result(ticket, false, now);
                }
            }
        }
        assert_eq!(starts, WAKE_BACKOFF_MS.len());
        // An extension that is gone loses its queue.
        let mut r = EventRouter::new();
        let RouterAction::StartWorker { ticket, .. } = r.emit(ev(B, "e", true, true))[1].clone() else { panic!() };
        r.wake_result(ticket, false, 0);
        assert!(r.tick(1000, &|_| false).is_empty());
        r.worker_running(B, "1", 1000);
        assert!(r.tick(5000, &|_| true).is_empty());
    }

    #[test]
    fn the_queue_is_capped() {
        let mut r = EventRouter::new();
        for _ in 0..(MAX_QUEUED + 10) {
            let RouterAction::StartWorker { ticket, .. } = r.emit(ev(A, "e", true, true))[1].clone() else { panic!() };
            r.wake_result(ticket, false, 0);
        }
        r.worker_running(A, "1", 0);
        let out = r.tick(SETTLE_MS, &|_| true);
        let RouterAction::DeliverToWorker { events, .. } = &out[0] else { panic!() };
        assert_eq!(events.len(), MAX_QUEUED);
    }
}

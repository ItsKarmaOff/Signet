//! Proxy authentication: answers 407 challenges with the extension that controls `proxy.settings`, through its
//! `onAuthRequired` listeners. Port of `apis/auth.ts`.
//!
//! An extension can have several contexts with a listener (its worker, a popup, an offscreen page) and only some know the
//! credentials. Like Chrome merging blocking responses, credentials from ANY context win; an empty or cancelling answer is
//! only final once every context that listens has answered. Credentials that keep being refused are not retried forever.
//!
//! The Electron code awaits a promise; here `handle` returns a key and the outcome arrives from [`ProxyAuthBroker::respond`],
//! [`ProxyAuthBroker::context_gone`] or [`ProxyAuthBroker::tick`] as `(key, AuthOutcome)` pairs.

use crate::common::{ApiContext, ContextKind, Emitter};
use crate::shared::AUTH_EVENT;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthOutcome {
    Skip,
    Cancel { reason: String },
    Credentials { username: String, password: String },
}

#[derive(Debug, Clone)]
pub struct ProxyChallenge {
    pub url: String,
    pub host: String,
    pub port: u16,
    pub scheme: String,
    pub realm: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Handled {
    /// Not ours (no extension controls the proxy): keep the engine's default.
    Skip,
    /// Decided at once (too many attempts).
    Done(AuthOutcome),
    /// Waiting for an answer; the outcome comes with this key.
    Pending(String),
}

struct Pending {
    extension_id: String,
    /// Contexts whose answer we still wait for; `worker:*` stands for a worker that is being woken.
    expected: HashSet<String>,
    saw_cancel: bool,
    deadline: u64,
}

const WAKING_WORKER: &str = "worker:*";

pub struct ProxyAuthBroker {
    emitter: Arc<dyn Emitter>,
    /// Extension controlling the proxy that may answer (has the webRequest permission), if any.
    responder: Box<dyn Fn() -> Option<String> + Send + Sync>,
    pending: HashMap<String, Pending>,
    recent: HashMap<String, Vec<u64>>,
    /// Extension, then context id, then its listener count.
    listening: HashMap<String, HashMap<String, usize>>,
    last_credentials: HashMap<String, u64>,
    counter: u64,
    timeout_ms: u64,
    max_attempts: usize,
    window_ms: u64,
    log: Vec<String>,
}

impl ProxyAuthBroker {
    pub fn new(emitter: Arc<dyn Emitter>, responder: Box<dyn Fn() -> Option<String> + Send + Sync>) -> Self {
        Self::with_limits(emitter, responder, 30_000, 5, 60_000)
    }

    pub fn with_limits(emitter: Arc<dyn Emitter>, responder: Box<dyn Fn() -> Option<String> + Send + Sync>, timeout_ms: u64, max_attempts: usize, window_ms: u64) -> Self {
        ProxyAuthBroker {
            emitter,
            responder,
            pending: HashMap::new(),
            recent: HashMap::new(),
            listening: HashMap::new(),
            last_credentials: HashMap::new(),
            counter: 0,
            timeout_ms,
            max_attempts,
            window_ms,
            log: vec![],
        }
    }

    pub fn take_log(&mut self) -> Vec<String> {
        std::mem::take(&mut self.log)
    }

    /// A context reports how many onAuthRequired listeners it has now.
    pub fn set_listeners(&mut self, extension_id: &str, context: &ApiContext, count: usize) -> Vec<(String, AuthOutcome)> {
        if count == 0 {
            return self.context_gone(extension_id, &context.id);
        }
        self.listening.entry(extension_id.to_string()).or_default().insert(context.id.clone(), count);
        vec![]
    }

    /// A context went away (page closed, worker stopped): it will never answer.
    pub fn context_gone(&mut self, extension_id: &str, context_id: &str) -> Vec<(String, AuthOutcome)> {
        if let Some(m) = self.listening.get_mut(extension_id) {
            m.remove(context_id);
        }
        let mut out = Vec::new();
        let keys: Vec<String> = self.pending.iter().filter(|(_, p)| p.extension_id == extension_id).map(|(k, _)| k.clone()).collect();
        for k in keys {
            if let Some(p) = self.pending.get_mut(&k) {
                if p.expected.remove(context_id) {
                    if let Some(o) = self.settle(&k) {
                        out.push((k, o));
                    }
                }
            }
        }
        out
    }

    /// Synchronous: should the engine's login event be taken over?
    pub fn can_handle(&self, is_proxy: bool) -> bool {
        // Even before the worker registered a listener (after a restart): it is woken and registers when it starts.
        is_proxy && (self.responder)().is_some()
    }

    pub fn handle(&mut self, c: &ProxyChallenge, now: u64) -> Handled {
        let Some(extension_id) = (self.responder)() else { return Handled::Skip };
        let attempt_key = format!("{extension_id}|{}:{}|{}", c.host, c.port, c.realm);
        let window = self.window_ms;
        let mut recent: Vec<u64> = self.recent.get(&attempt_key).cloned().unwrap_or_default().into_iter().filter(|t| now.saturating_sub(*t) < window).collect();
        let last_ok = self.last_credentials.get(&attempt_key).copied();
        if recent.len() >= self.max_attempts {
            self.log.push(format!("[extension-apis] {extension_id} proxy auth refused {} times in a row for {}:{}, cancelling", recent.len(), c.host, c.port));
            return Handled::Done(AuthOutcome::Cancel { reason: "too many attempts".into() });
        }
        recent.push(now);
        let attempt = recent.len();
        self.recent.insert(attempt_key, recent);

        self.counter += 1;
        let key = format!("signet-auth-{}", self.counter);
        let mut expected: HashSet<String> = self.listening.get(&extension_id).map(|m| m.keys().cloned().collect()).unwrap_or_default();
        // No worker among the known listeners: it was stopped, wake it (it registers its listener again when it starts).
        if !expected.iter().any(|id| id.starts_with("worker-")) {
            expected.insert(WAKING_WORKER.into());
        }
        let again = last_ok.is_some_and(|t| now.saturating_sub(t) < 10_000);
        self.log.push(format!(
            "[extension-apis] {extension_id} proxy auth requested by {}:{} ({}, realm \"{}\", attempt {attempt}, url {}){}",
            c.host,
            c.port,
            c.scheme,
            c.realm,
            c.url.chars().take(120).collect::<String>(),
            if again { " - asked again right after credentials were supplied: the proxy may be refusing them" } else { "" }
        ));
        let details = json!({
            "requestId": key, "url": c.url, "method": "GET", "frameId": 0, "parentFrameId": -1, "tabId": -1, "type": "other",
            "timeStamp": now, "scheme": c.scheme, "realm": c.realm, "challenger": { "host": c.host, "port": c.port },
            "isProxy": true, "responseHeaders": [], "statusCode": 407, "statusLine": "HTTP/1.1 407 Proxy Authentication Required"
        });
        self.pending.insert(key.clone(), Pending { extension_id: extension_id.clone(), expected, saw_cancel: false, deadline: now + self.timeout_ms });
        // Waking the worker is the point; the event is time-sensitive, so it is not queued for later.
        self.emitter.emit(&extension_id, AUTH_EVENT, vec![json!(key), details], true, false);
        Handled::Pending(key)
    }

    /// A context's answer (`webRequest.authResponse`); only the extension the request was sent to counts.
    pub fn respond(&mut self, extension_id: &str, context: &ApiContext, key: &Value, response: &Value, now: u64) -> Vec<(String, AuthOutcome)> {
        let Some(key) = key.as_str().map(String::from) else { return vec![] };
        let Some(p) = self.pending.get_mut(&key) else { return vec![] };
        if p.extension_id != extension_id {
            return vec![];
        }
        let cancel = response.get("cancel").and_then(|c| c.as_bool()) == Some(true);
        let creds = response.get("authCredentials");
        let user = creds.and_then(|c| c.get("username")).and_then(|u| u.as_str());
        let pass = creds.and_then(|c| c.get("password")).and_then(|u| u.as_str());
        let who = format!("{} {}", if context.kind == ContextKind::Worker { "worker" } else { "page" }, context.url);
        let _ = now;
        if let (false, Some(u), Some(pw)) = (cancel, user, pass) {
            self.log.push(format!("[extension-apis] {extension_id} proxy auth answered by {who}: credentials supplied"));
            let outcome = AuthOutcome::Credentials { username: u.into(), password: pw.into() };
            return self.finish(&key, outcome);
        }
        self.log.push(format!("[extension-apis] {extension_id} proxy auth answered by {who}: {}", if cancel { "cancelled" } else { "empty" }));
        if cancel {
            p.saw_cancel = true;
        }
        p.expected.remove(&context.id);
        // A worker answering satisfies the placeholder for the worker we were waking.
        if context.kind == ContextKind::Worker {
            p.expected.remove(WAKING_WORKER);
        }
        match self.settle(&key) {
            Some(o) => vec![(key, o)],
            None => vec![],
        }
    }

    /// Nobody left to wait for and nobody supplied credentials.
    fn settle(&mut self, key: &str) -> Option<AuthOutcome> {
        let p = self.pending.get(key)?;
        if !p.expected.is_empty() {
            return None;
        }
        let reason = if p.saw_cancel { "extension cancelled" } else { "no listener supplied credentials" };
        self.finish(key, AuthOutcome::Cancel { reason: reason.into() }).into_iter().next().map(|(_, o)| o)
    }

    fn finish(&mut self, key: &str, outcome: AuthOutcome) -> Vec<(String, AuthOutcome)> {
        let Some(p) = self.pending.remove(key) else { return vec![] };
        if let AuthOutcome::Cancel { reason } = &outcome {
            self.log.push(format!("[extension-apis] {} proxy auth cancelled: {reason}", p.extension_id));
        }
        vec![(key.to_string(), outcome)]
    }

    /// Records credentials used for a challenge (for the "asked again" diagnostic) and expires silent requests (30 s).
    pub fn tick(&mut self, now: u64) -> Vec<(String, AuthOutcome)> {
        let mut out = Vec::new();
        let late: Vec<String> = self.pending.iter().filter(|(_, p)| p.deadline <= now).map(|(k, _)| k.clone()).collect();
        for k in late {
            if let Some(p) = self.pending.remove(&k) {
                let mut waiting: Vec<_> = p.expected.iter().cloned().collect();
                waiting.sort();
                self.log.push(format!("[extension-apis] {} proxy auth: no answer from {}", p.extension_id, waiting.join(", ")));
                out.push((k, AuthOutcome::Cancel { reason: "no answer".into() }));
            }
        }
        out
    }

    /// Call when credentials were handed to the engine, to feed the "asked again" diagnostic.
    pub fn credentials_used(&mut self, extension_id: &str, c: &ProxyChallenge, now: u64) {
        self.last_credentials.insert(format!("{extension_id}|{}:{}|{}", c.host, c.port, c.realm), now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::{ContextKind, Outbox};

    const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn challenge(port: u16) -> ProxyChallenge {
        ProxyChallenge { url: "https://example.com/".into(), host: "proxy.example".into(), port, scheme: "basic".into(), realm: "r".into() }
    }
    fn creds() -> Value {
        json!({ "authCredentials": { "username": "u", "password": "p" } })
    }
    fn ctx(kind: ContextKind, id: &str) -> ApiContext {
        ApiContext { kind, id: id.into(), url: format!("chrome-extension://{A}/{id}.html") }
    }
    fn make(responder: Option<&str>) -> (ProxyAuthBroker, Arc<Outbox>) {
        let out = Arc::new(Outbox::new());
        let r = responder.map(String::from);
        (ProxyAuthBroker::with_limits(out.clone(), Box::new(move || r.clone()), 30_000, 3, 60_000), out)
    }
    fn key_of(h: Handled) -> String {
        match h {
            Handled::Pending(k) => k,
            other => panic!("{other:?}"),
        }
    }
    fn only(mut v: Vec<(String, AuthOutcome)>) -> AuthOutcome {
        assert_eq!(v.len(), 1);
        v.remove(0).1
    }

    #[test]
    fn only_takes_over_proxy_challenges_when_an_extension_can_answer() {
        assert!(make(Some(A)).0.can_handle(true));
        assert!(!make(Some(A)).0.can_handle(false));
        assert!(!make(None).0.can_handle(true));
    }

    #[test]
    fn asks_the_extension_waking_its_worker_without_queueing() {
        let (mut b, out) = make(Some(A));
        let worker = ctx(ContextKind::Worker, "worker-1");
        b.set_listeners(A, &worker, 1);
        let key = key_of(b.handle(&challenge(89), 0));
        let ev = out.drain();
        assert_eq!((ev[0].extension_id.as_str(), ev[0].name.as_str(), ev[0].wake, ev[0].queue), (A, AUTH_EVENT, true, false));
        assert_eq!(ev[0].args[1]["isProxy"], true);
        assert_eq!(ev[0].args[1]["challenger"], json!({ "host": "proxy.example", "port": 89 }));
        let o = only(b.respond(A, &worker, &json!(key), &creds(), 1));
        assert_eq!(o, AuthOutcome::Credentials { username: "u".into(), password: "p".into() });
    }

    #[test]
    fn an_empty_answer_first_does_not_cancel_credentials_of_the_other_win() {
        let (mut b, _) = make(Some(A));
        let worker = ctx(ContextKind::Worker, "worker-1");
        let page = ctx(ContextKind::Page, "page-7");
        b.set_listeners(A, &worker, 1);
        b.set_listeners(A, &page, 1);
        let key = key_of(b.handle(&challenge(89), 0));
        assert!(b.respond(A, &page, &json!(key), &json!({}), 1).is_empty());
        let o = only(b.respond(A, &worker, &json!(key), &creds(), 2));
        assert_eq!(o, AuthOutcome::Credentials { username: "u".into(), password: "p".into() });
        let log = b.take_log().join("\n");
        assert!(log.contains("answered by page") && log.contains(": empty") && log.contains("credentials supplied"));
    }

    #[test]
    fn cancels_once_everybody_answered_without_credentials_saying_why() {
        let (mut b, _) = make(Some(A));
        let worker = ctx(ContextKind::Worker, "worker-1");
        let page = ctx(ContextKind::Page, "page-7");
        b.set_listeners(A, &worker, 1);
        b.set_listeners(A, &page, 1);
        let k1 = key_of(b.handle(&challenge(89), 0));
        assert!(b.respond(A, &worker, &json!(k1), &json!({ "cancel": true }), 1).is_empty());
        assert_eq!(only(b.respond(A, &page, &json!(k1), &json!({}), 1)), AuthOutcome::Cancel { reason: "extension cancelled".into() });
        let k2 = key_of(b.handle(&challenge(90), 2));
        b.respond(A, &worker, &json!(k2), &json!({}), 3);
        assert_eq!(only(b.respond(A, &page, &json!(k2), &Value::Null, 3)), AuthOutcome::Cancel { reason: "no listener supplied credentials".into() });
    }

    #[test]
    fn a_context_that_goes_away_stops_being_waited_for() {
        let (mut b, _) = make(Some(A));
        let worker = ctx(ContextKind::Worker, "worker-1");
        let popup = ctx(ContextKind::Page, "page-9");
        b.set_listeners(A, &worker, 1);
        b.set_listeners(A, &popup, 1);
        let key = key_of(b.handle(&challenge(89), 0));
        assert!(b.respond(A, &worker, &json!(key), &json!({}), 1).is_empty());
        assert!(matches!(only(b.context_gone(A, "page-9")), AuthOutcome::Cancel { .. }));
    }

    #[test]
    fn a_woken_worker_answering_satisfies_the_placeholder() {
        let (mut b, _) = make(Some(A));
        let key = key_of(b.handle(&challenge(89), 0));
        let o = only(b.respond(A, &ctx(ContextKind::Worker, "worker-5"), &json!(key), &json!({}), 1));
        assert_eq!(o, AuthOutcome::Cancel { reason: "no listener supplied credentials".into() });
    }

    #[test]
    fn silence_times_out_and_another_extension_cannot_answer() {
        let (mut b, _) = make(Some(A));
        let worker = ctx(ContextKind::Worker, "worker-1");
        b.set_listeners(A, &worker, 1);
        let key = key_of(b.handle(&challenge(89), 0));
        assert!(b.respond(&"b".repeat(32), &worker, &json!(key), &creds(), 1).is_empty());
        assert!(b.tick(29_999).is_empty());
        assert_eq!(only(b.tick(30_000)), AuthOutcome::Cancel { reason: "no answer".into() });
    }

    #[test]
    fn stops_after_repeated_challenges_and_recovers_after_the_window() {
        let (mut b, _) = make(Some(A));
        let worker = ctx(ContextKind::Worker, "worker-1");
        b.set_listeners(A, &worker, 1);
        for i in 0..3 {
            let key = key_of(b.handle(&challenge(89), i));
            assert!(matches!(only(b.respond(A, &worker, &json!(key), &creds(), i)), AuthOutcome::Credentials { .. }));
        }
        assert_eq!(b.handle(&challenge(89), 10), Handled::Done(AuthOutcome::Cancel { reason: "too many attempts".into() }));
        let key = key_of(b.handle(&challenge(89), 61_000));
        assert!(matches!(only(b.respond(A, &worker, &json!(key), &creds(), 61_001)), AuthOutcome::Credentials { .. }));
    }

    #[test]
    fn skips_when_nobody_controls_the_proxy() {
        assert_eq!(make(None).0.handle(&challenge(89), 0), Handled::Skip);
    }
}

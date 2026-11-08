//! Enforcement of `chrome.declarativeNetRequest`. Port of `apis/dnr-engine.ts`.
//!
//! Rules come from three places: the static rulesets of the manifest (only the enabled ones), the extension's dynamic
//! rules and its session rules. They are compiled into one [`RuleIndex`], rebuilt (debounced 50 ms through [`DnrEngine::tick`])
//! when anything changes, and consulted by the request hooks the web layer calls: [`DnrEngine::before_request`],
//! [`DnrEngine::send_headers`] and [`DnrEngine::received_headers`]. [`DnrEngine::wanted_events`] says which of these hooks
//! must be wired, so the web layer only intercepts what an active rule needs.
//!
//! Approximations: Chrome gives precedence to the extension installed most recently; here the order in which extensions
//! were added since launch is used. A winning `allow` of one extension ends the decision for lower-precedence extensions,
//! as in Chrome. `allowAllRequests` on a sub frame is remembered under the frame the web layer reports for the request.
//!
//! Static ruleset files are read synchronously during a rebuild (once per ruleset): run `tick` off the UI thread when
//! rulesets are large.

use crate::common::{ApiError, ApiResult};
use crate::dnr::{validate_rule, DnrStore};
use crate::dnr_match::{
    apply_header_ops, compile_rule, dnr_type, make_request, redirect_target, resolve, upgraded_url, Action, CompiledRule, DnrRequest, RequestInput,
    ResolveContext, RuleIndex,
};
use crate::shared::dnr_constants;
use crate::urlmatch::{extension_file_path, MatchPattern};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

/// Chrome's global pool of static rules on top of the guaranteed minimum.
const GLOBAL_STATIC_RULES: i64 = 300_000;
const LOG_WINDOW_MS: u64 = 5 * 60_000;
const LOG_MAX_PER_TAB: usize = 1000;
pub const REBUILD_DELAY_MS: u64 = 50;
/// Longest delay between two pushes of the action counts to the toolbar.
pub const COUNTS_PUSH_MS: u64 = 250;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DnrEvent {
    OnBeforeRequest,
    OnBeforeSendHeaders,
    OnHeadersReceived,
}

/// Action counts: tab, then extension.
pub type ActionCounts = BTreeMap<i64, BTreeMap<String, u32>>;

/// Permissions and host patterns the user granted at runtime, on top of the manifest's.
pub trait GrantSource: Send + Sync {
    fn permissions_of(&self, id: &str) -> Vec<String>;
    fn origins_of(&self, id: &str) -> Vec<String>;
}

struct NoGrants;
impl GrantSource for NoGrants {
    fn permissions_of(&self, _: &str) -> Vec<String> {
        vec![]
    }
    fn origins_of(&self, _: &str) -> Vec<String> {
        vec![]
    }
}

struct StaticRuleset {
    id: String,
    path: String,
    enabled_by_default: bool,
    rules: Option<Vec<CompiledRule>>,
}

struct ExtState {
    dir: PathBuf,
    version: String,
    seq: u64,
    /// Has `declarativeNetRequest` or `declarativeNetRequestWithHostAccess`: without one, its rules do nothing.
    active: bool,
    /// Only the "with host access" permission: every action needs host access.
    host_only: bool,
    feedback: bool,
    hosts: Vec<MatchPattern>,
    rulesets: Vec<StaticRuleset>,
    manifest: Value,
}

/// The fields of a network request the engine reads (the web layer fills them from CEF).
#[derive(Debug, Clone, Default)]
pub struct RequestDetails {
    pub url: String,
    pub method: String,
    /// `mainFrame`, `subFrame`, `xhr`... (see `dnr_type`).
    pub resource_type: Option<String>,
    pub tab_id: Option<i64>,
    /// Origin of the frame that made the request.
    pub frame_origin: Option<String>,
    /// Ids of the frame and its ancestors, innermost first.
    pub frame_chain: Vec<i64>,
    pub referrer: Option<String>,
    pub request_headers: Vec<(String, String)>,
    pub response_headers: Vec<(String, Vec<String>)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequestOutcome {
    /// Let it through.
    Pass,
    Cancel,
    Redirect(String),
}

#[derive(Debug, Clone)]
struct MatchLog {
    ext: String,
    set: String,
    id: i64,
    time: u64,
}

fn has_permission(manifest: &Value, permission: &str, granted: &[String]) -> bool {
    manifest.get("permissions").and_then(|p| p.as_array()).is_some_and(|l| l.iter().any(|v| v.as_str() == Some(permission))) || granted.iter().any(|g| g == permission)
}

fn strings(v: Option<&Value>) -> Vec<String> {
    v.and_then(|l| l.as_array()).map(|a| a.iter().map(|s| s.as_str().map(String::from).unwrap_or_else(|| s.to_string())).collect()).unwrap_or_default()
}

/// What a decision needs after the index borrow ends.
struct Decision {
    action: Action,
    rule: Option<(String, String, i64)>,
    target: Option<String>,
    frame_allows: Vec<(String, i64)>,
    request_headers: Vec<crate::dnr_match::HeaderOp>,
    response_headers: Vec<crate::dnr_match::HeaderOp>,
    used: Vec<(String, String, i64, bool, bool)>,
}

pub struct DnrEngine {
    pub store: DnrStore,
    grants: Arc<dyn GrantSource>,
    exts: HashMap<String, ExtState>,
    seq: u64,
    index: RuleIndex,
    wanted: HashSet<DnrEvent>,
    rebuild_due: Option<u64>,
    /// `allowAllRequests` grants: tab, then frame key (`main` or `f<id>`), then extension, then priority.
    frames: HashMap<i64, HashMap<String, HashMap<String, i64>>>,
    matched_log: HashMap<i64, Vec<MatchLog>>,
    last_sweep: u64,
    counts: BTreeMap<i64, BTreeMap<String, u32>>,
    counts_due: Option<u64>,
    now_ms: u64,
    log: Vec<String>,
}

impl DnrEngine {
    pub fn new(store: DnrStore) -> Self {
        DnrEngine {
            store,
            grants: Arc::new(NoGrants),
            exts: HashMap::new(),
            seq: 0,
            index: RuleIndex::new(vec![]),
            wanted: HashSet::new(),
            rebuild_due: None,
            frames: HashMap::new(),
            matched_log: HashMap::new(),
            last_sweep: 0,
            counts: BTreeMap::new(),
            counts_due: None,
            now_ms: 0,
            log: vec![],
        }
    }

    pub fn set_grants(&mut self, grants: Arc<dyn GrantSource>) {
        self.grants = grants;
    }

    /// Sets the clock (milliseconds, any origin as long as it is consistent).
    pub fn set_now(&mut self, now_ms: u64) {
        self.now_ms = now_ms;
    }

    /// Log lines produced since the last call (development).
    pub fn take_log(&mut self) -> Vec<String> {
        let mut out = self.store.take_log();
        out.append(&mut self.log);
        out
    }

    // ---- extensions ----

    pub fn add_extension(&mut self, id: &str, dir: impl Into<PathBuf>, version: &str, manifest: &Value) {
        let dir = dir.into();
        let known = self.exts.get(id).is_some_and(|x| x.version == version && x.dir == dir);
        if !known {
            let seq = self.exts.get(id).map(|x| x.seq).unwrap_or_else(|| {
                self.seq += 1;
                self.seq
            });
            let mut rulesets = Vec::new();
            for r in manifest.get("declarative_net_request").and_then(|d| d.get("rule_resources")).and_then(|r| r.as_array()).into_iter().flatten() {
                if let (Some(rid), Some(path)) = (r.get("id").and_then(|i| i.as_str()), r.get("path").and_then(|p| p.as_str())) {
                    rulesets.push(StaticRuleset { id: rid.into(), path: path.into(), enabled_by_default: r.get("enabled").and_then(|e| e.as_bool()) == Some(true), rules: None });
                }
            }
            self.exts.insert(
                id.to_string(),
                ExtState { dir, version: version.into(), seq, active: false, host_only: false, feedback: false, hosts: vec![], rulesets, manifest: manifest.clone() },
            );
        }
        if let Some(x) = self.exts.get_mut(id) {
            x.manifest = manifest.clone();
        }
        self.derive_access(id);
        self.schedule();
    }

    /// Permissions and host access from the manifest plus the runtime grants.
    fn derive_access(&mut self, id: &str) {
        let granted = self.grants.permissions_of(id);
        let origins = self.grants.origins_of(id);
        let Some(x) = self.exts.get_mut(id) else { return };
        let m = &x.manifest;
        let has = |p: &str| has_permission(m, p, &granted);
        x.active = has("declarativeNetRequest") || has("declarativeNetRequestWithHostAccess");
        x.host_only = !has("declarativeNetRequest");
        x.feedback = has("declarativeNetRequestFeedback");
        // Optional host permissions only count once the user granted them (chrome.permissions.request).
        let mut patterns = strings(m.get("host_permissions"));
        patterns.extend(origins);
        patterns.extend(strings(m.get("permissions")).into_iter().filter(|p| p.contains("://") || p == "<all_urls>"));
        x.hosts = patterns.iter().filter_map(|p| MatchPattern::parse(p)).collect();
    }

    /// The extension's grants changed: its access is recomputed and the rules rebuilt.
    pub fn refresh_access(&mut self, id: &str) {
        if self.exts.contains_key(id) {
            self.derive_access(id);
            self.schedule();
        }
    }

    pub fn remove_extension(&mut self, id: &str) {
        if self.exts.remove(id).is_none() {
            return;
        }
        let mut had = false;
        self.counts.retain(|_, per| {
            had |= per.remove(id).is_some();
            !per.is_empty()
        });
        if had {
            self.push_counts();
        }
        self.schedule();
    }

    // ---- index ----

    fn schedule(&mut self) {
        if self.rebuild_due.is_none() {
            self.rebuild_due = Some(self.now_ms + REBUILD_DELAY_MS);
        }
    }

    /// Advances the clock; rebuilds when due (also after a store change) and returns the counts to push to the toolbar
    /// when the 250 ms throttle elapsed.
    pub fn tick(&mut self, now_ms: u64) -> Option<ActionCounts> {
        self.now_ms = now_ms;
        if self.store.take_changed() {
            self.schedule();
        }
        if self.rebuild_due.is_some_and(|d| d <= now_ms) {
            self.rebuild();
        }
        if self.counts_due.is_some_and(|d| d <= now_ms) {
            self.counts_due = None;
            return Some(self.action_counts());
        }
        None
    }

    fn enabled_ids(&self, id: &str) -> Vec<String> {
        let Some(x) = self.exts.get(id) else { return vec![] };
        let list = self
            .store
            .enabled_rulesets(id, &x.version)
            .unwrap_or_else(|| x.rulesets.iter().filter(|r| r.enabled_by_default).map(|r| r.id.clone()).collect());
        list.into_iter().filter(|r| x.rulesets.iter().any(|s| s.id == *r)).collect()
    }

    /// Recompiles every active rule into a fresh index. Returns true when the set of wanted request hooks changed.
    pub fn rebuild(&mut self) -> bool {
        self.rebuild_due = None;
        self.store.take_changed();
        let mut rules: Vec<CompiledRule> = Vec::new();
        let mut ids: Vec<String> = self.exts.keys().cloned().collect();
        ids.sort_by_key(|i| self.exts[i].seq);
        for id in ids {
            if !self.exts[&id].active {
                continue;
            }
            for rs_id in self.enabled_ids(&id) {
                self.ensure_loaded(&id, &rs_id);
                if let Some(rs) = self.exts[&id].rulesets.iter().find(|r| r.id == rs_id) {
                    // Rules are cheap to recompile from the raw JSON; the compiled form holds lazy matchers.
                    for r in rs.rules.iter().flatten() {
                        if let Some(c) = compile_rule(&id, &rs.id, &r.raw) {
                            rules.push(c);
                        }
                    }
                }
            }
            for (kind, set) in [(crate::dnr::RuleKind::Dynamic, "_dynamic"), (crate::dnr::RuleKind::Session, "_session")] {
                for rule in self.store.get(kind, &id, &Value::Null) {
                    if let Some(c) = compile_rule(&id, set, &rule) {
                        rules.push(c);
                    }
                }
            }
        }
        let mut wanted = HashSet::new();
        for r in &rules {
            wanted.insert(DnrEvent::OnBeforeRequest);
            if r.kind == "modifyHeaders" {
                let a = r.raw.get("action");
                let has = |k: &str| a.and_then(|a| a.get(k)).and_then(|l| l.as_array()).is_some_and(|l| !l.is_empty());
                if has("requestHeaders") {
                    wanted.insert(DnrEvent::OnBeforeSendHeaders);
                }
                if has("responseHeaders") {
                    wanted.insert(DnrEvent::OnHeadersReceived);
                }
            }
        }
        self.index = RuleIndex::new(rules);
        let changed = wanted != self.wanted;
        self.wanted = wanted;
        changed
    }

    /// Request hooks that must be wired now. Intercepting `onBeforeSendHeaders` / `onHeadersReceived` only while an active
    /// rule changes those headers keeps the extensions' own `webRequest` listeners working otherwise.
    pub fn wanted_events(&self) -> Vec<DnrEvent> {
        let mut v: Vec<DnrEvent> = self.wanted.iter().copied().collect();
        v.sort();
        v
    }

    // ---- static rulesets ----

    /// Reads and compiles a ruleset file; invalid rules are skipped with one log line for the whole ruleset.
    fn ensure_loaded(&mut self, ext: &str, ruleset: &str) {
        let Some(x) = self.exts.get(ext) else { return };
        let Some(rs) = x.rulesets.iter().find(|r| r.id == ruleset) else { return };
        if rs.rules.is_some() {
            return;
        }
        let result: Result<(Vec<CompiledRule>, usize), String> = (|| {
            let file = extension_file_path(&x.dir, ext, &rs.path).ok_or_else(|| "path outside the extension".to_string())?;
            let text = std::fs::read_to_string(&file).map_err(|e| e.to_string())?;
            let data: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
            let list = data.as_array().ok_or_else(|| "not a list of rules".to_string())?;
            let mut compiled = Vec::new();
            let mut skipped = 0;
            for raw in list {
                match validate_rule(raw).ok().and_then(|r| compile_rule(ext, ruleset, &r)) {
                    Some(c) => compiled.push(c),
                    None => skipped += 1,
                }
            }
            Ok((compiled, skipped))
        })();
        let rules = match result {
            Ok((compiled, skipped)) => {
                if skipped > 0 {
                    self.log.push(format!("[extension-apis] {ext} declarativeNetRequest: ruleset \"{ruleset}\" skipped {skipped} invalid rule(s)"));
                }
                compiled
            }
            Err(e) => {
                self.log.push(format!("[extension-apis] {ext} declarativeNetRequest: ruleset \"{ruleset}\" cannot be loaded: {e}"));
                vec![]
            }
        };
        if let Some(rs) = self.exts.get_mut(ext).and_then(|x| x.rulesets.iter_mut().find(|r| r.id == ruleset)) {
            rs.rules = Some(rules);
        }
    }

    fn state(&self, ext: &str) -> ApiResult<&ExtState> {
        self.exts.get(ext).ok_or_else(|| ApiError::new("Extension is not loaded."))
    }

    // ---- requests ----

    fn request(d: &RequestDetails) -> Option<DnrRequest> {
        let ty = dnr_type(d.resource_type.as_deref());
        let mut initiator: Option<String> = None;
        if ty != "main_frame" {
            initiator = d.frame_origin.clone();
            if initiator.as_deref().is_none_or(|i| i.is_empty() || i == "null") {
                initiator = d.referrer.clone().filter(|r| !r.is_empty());
            }
            if initiator.is_none() {
                initiator = d.request_headers.iter().find(|(k, _)| k == "Origin").map(|(_, v)| v.clone());
            }
        }
        make_request(RequestInput { url: &d.url, resource_type: Some(ty), method: Some(&d.method), initiator: initiator.as_deref(), tab_id: Some(d.tab_id.unwrap_or(-1)) })
    }

    fn can_access(x: &ExtState, q: &DnrRequest) -> bool {
        if !x.hosts.iter().any(|t| t.matches(&q.url)) {
            return false;
        }
        match &q.initiator {
            None => true,
            Some(i) if !["http:", "https:", "ws:", "wss:", "ftp:"].iter().any(|s| i.starts_with(s)) => true,
            Some(i) => x.hosts.iter().any(|t| t.matches(&format!("{i}/"))),
        }
    }

    fn can_act(&self, r: &CompiledRule, q: &DnrRequest) -> bool {
        let Some(x) = self.exts.get(&r.ext) else { return false };
        if !x.active {
            return false;
        }
        // Requests of an extension are only affected by its own rules.
        if q.initiator.as_deref().is_some_and(|i| i.starts_with("chrome-extension://")) && q.initiator_host.as_deref() != Some(r.ext.as_str()) {
            return false;
        }
        if r.kind == "redirect" || r.kind == "modifyHeaders" || x.host_only {
            return Self::can_access(x, q);
        }
        true
    }

    fn frame_keys(chain: &[i64]) -> Vec<String> {
        std::iter::once("main".to_string()).chain(chain.iter().take(32).map(|i| format!("f{i}"))).collect()
    }

    fn inherited_allows(&self, q: &DnrRequest, chain: &[i64]) -> Option<HashMap<String, i64>> {
        let per = self.frames.get(&q.tab_id)?;
        let mut out: Option<HashMap<String, i64>> = None;
        for key in Self::frame_keys(chain) {
            for (ext, priority) in per.get(&key).into_iter().flatten() {
                let m = out.get_or_insert_with(HashMap::new);
                let e = m.entry(ext.clone()).or_insert(0);
                *e = (*e).max(*priority);
            }
        }
        out
    }

    fn decide(&mut self, q: &DnrRequest, chain: &[i64], start_of_request: bool) -> Option<Decision> {
        if start_of_request && q.resource_type == "main_frame" {
            self.frames.remove(&q.tab_id);
            if self.counts.remove(&q.tab_id).is_some() {
                self.push_counts();
            }
        }
        let matches = self.index.matching(q);
        let inherited = self.inherited_allows(q, chain);
        if matches.is_empty() && inherited.is_none() {
            return None;
        }
        let rank = |e: &str| -(self.exts.get(e).map(|x| x.seq as i64).unwrap_or(0));
        let can_act = |r: &CompiledRule| self.can_act(r, q);
        let res = resolve(&matches, q, &ResolveContext { rank: &rank, frame_allows: inherited.as_ref(), can_act: &can_act });
        let target = match (res.action, res.rule) {
            (Action::Redirect, Some(r)) => redirect_target(r, q),
            (Action::Upgrade, _) => upgraded_url(&q.url),
            _ => None,
        };
        let has = |r: &CompiledRule, k: &str| r.raw.get("action").and_then(|a| a.get(k)).and_then(|l| l.as_array()).is_some_and(|l| !l.is_empty());
        Some(Decision {
            action: res.action,
            rule: res.rule.map(|r| (r.ext.clone(), r.set.clone(), r.id)),
            target,
            frame_allows: res.frame_allows.clone(),
            request_headers: res.request_headers.clone(),
            response_headers: res.response_headers.clone(),
            used: res.used.iter().map(|r| (r.ext.clone(), r.set.clone(), r.id, has(r, "requestHeaders"), has(r, "responseHeaders"))).collect(),
        })
    }

    fn remember(&mut self, q: &DnrRequest, frame_id: Option<i64>, allows: &[(String, i64)]) {
        if allows.is_empty() || q.tab_id < 0 {
            return;
        }
        let key = if q.resource_type == "main_frame" { Some("main".to_string()) } else { frame_id.map(|f| format!("f{f}")) };
        let Some(key) = key else { return };
        let grants = self.frames.entry(q.tab_id).or_default().entry(key).or_default();
        for (ext, priority) in allows {
            let e = grants.entry(ext.clone()).or_insert(0);
            *e = (*e).max(*priority);
        }
    }

    /// Hook for the start of a request: block, redirect or upgrade.
    pub fn before_request(&mut self, d: &RequestDetails) -> RequestOutcome {
        let Some(q) = Self::request(d) else { return RequestOutcome::Pass };
        let Some(res) = self.decide(&q, &d.frame_chain, true) else { return RequestOutcome::Pass };
        self.remember(&q, d.frame_chain.first().copied(), &res.frame_allows);
        if let Some((ext, set, id)) = &res.rule {
            self.record(&q, &[(ext.clone(), set.clone(), *id)]);
        }
        let ext = res.rule.as_ref().map(|r| r.0.clone());
        match res.action {
            Action::Block => {
                self.count(q.tab_id, ext.as_deref(), 1);
                RequestOutcome::Cancel
            }
            Action::Redirect | Action::Upgrade => match res.target {
                Some(url) => {
                    self.count(q.tab_id, ext.as_deref(), 1);
                    RequestOutcome::Redirect(url)
                }
                None => RequestOutcome::Pass,
            },
            _ => RequestOutcome::Pass,
        }
    }

    /// Hook for outgoing headers: the new request headers, or None to keep them.
    pub fn send_headers(&mut self, d: &RequestDetails) -> Option<Vec<(String, String)>> {
        let q = Self::request(d)?;
        let res = self.decide(&q, &d.frame_chain, false)?;
        if res.request_headers.is_empty() || d.request_headers.is_empty() {
            return None;
        }
        let used: Vec<_> = res.used.iter().filter(|u| u.3).map(|u| (u.0.clone(), u.1.clone(), u.2)).collect();
        self.record(&q, &used);
        let input: Vec<(String, Vec<String>)> = d.request_headers.iter().map(|(k, v)| (k.clone(), vec![v.clone()])).collect();
        Some(apply_header_ops(&input, &res.request_headers, false).into_iter().map(|(k, v)| (k, v.join(", "))).collect())
    }

    /// Hook for incoming headers: the new response headers, or None to keep them.
    pub fn received_headers(&mut self, d: &RequestDetails) -> Option<Vec<(String, Vec<String>)>> {
        let q = Self::request(d)?;
        let res = self.decide(&q, &d.frame_chain, false)?;
        if res.response_headers.is_empty() || d.response_headers.is_empty() {
            return None;
        }
        let used: Vec<_> = res.used.iter().filter(|u| u.4).map(|u| (u.0.clone(), u.1.clone(), u.2)).collect();
        self.record(&q, &used);
        Some(apply_header_ops(&d.response_headers, &res.response_headers, true))
    }

    /// A tab was destroyed: forget what the engine kept for it.
    pub fn tab_gone(&mut self, tab: i64) {
        self.frames.remove(&tab);
        if self.counts.remove(&tab).is_some() {
            self.push_counts();
        }
    }

    // ---- action count ----

    /// Adds `by` (possibly negative, never below 0) to the count of a tab for an extension that displays it.
    fn count(&mut self, tab: i64, ext: Option<&str>, by: i64) {
        let Some(ext) = ext else { return };
        if tab < 0 || !self.store.display_action_count(ext) {
            return;
        }
        let current = self.counts.get(&tab).and_then(|p| p.get(ext)).copied().unwrap_or(0) as i64;
        let next = (current + by).max(0);
        if next == current {
            return;
        }
        let per = self.counts.entry(tab).or_default();
        if next > 0 {
            per.insert(ext.to_string(), next as u32);
        } else {
            per.remove(ext);
        }
        if per.is_empty() {
            self.counts.remove(&tab);
        }
        self.push_counts();
    }

    /// Current counts, only for the extensions that display them: tab, then extension.
    pub fn action_counts(&self) -> ActionCounts {
        let mut out = ActionCounts::new();
        for (tab, per) in &self.counts {
            for (ext, n) in per {
                if *n > 0 && self.store.display_action_count(ext) {
                    out.entry(*tab).or_default().insert(ext.clone(), *n);
                }
            }
        }
        out
    }

    /// Tells the toolbar at most every 250 ms (trailing), so a page with hundreds of blocked requests sends few messages.
    fn push_counts(&mut self) {
        if self.counts_due.is_none() {
            self.counts_due = Some(self.now_ms + COUNTS_PUSH_MS);
        }
    }

    // ---- API ----

    pub fn get_enabled_rulesets(&self, ext: &str) -> ApiResult<Vec<String>> {
        self.state(ext)?;
        Ok(self.enabled_ids(ext))
    }

    pub fn update_enabled_rulesets(&mut self, ext: &str, options: &Value) -> ApiResult<()> {
        let x = self.state(ext)?;
        let version = x.version.clone();
        let enable = strings(options.get("enableRulesetIds"));
        let disable = strings(options.get("disableRulesetIds"));
        for id in enable.iter().chain(disable.iter()) {
            if !x.rulesets.iter().any(|r| r.id == *id) {
                return Err(ApiError::new(format!("Invalid ruleset id: {id}.")));
            }
        }
        let mut next: Vec<String> = self.enabled_ids(ext);
        next.retain(|i| !disable.contains(i));
        for id in &enable {
            if !next.contains(id) {
                next.push(id.clone());
            }
        }
        if next.len() > dnr_constants()["MAX_NUMBER_OF_ENABLED_STATIC_RULESETS"].as_u64().unwrap_or(50) as usize {
            return Err(ApiError::new("The number of enabled static rulesets exceeds the limit."));
        }
        self.store.set_enabled_rulesets(ext, &version, next);
        for id in &enable {
            self.ensure_loaded(ext, id);
        }
        self.rebuild();
        Ok(())
    }

    pub fn available_static_rules(&self, ext: &str) -> ApiResult<i64> {
        let x = self.state(ext)?;
        let used: i64 = self.enabled_ids(ext).iter().filter_map(|id| x.rulesets.iter().find(|r| r.id == *id)).map(|r| r.rules.as_ref().map_or(0, |l| l.len() as i64)).sum();
        let guaranteed = dnr_constants()["GUARANTEED_MINIMUM_STATIC_RULES"].as_i64().unwrap_or(30000);
        Ok((guaranteed + GLOBAL_STATIC_RULES - used).max(0))
    }

    fn record(&mut self, q: &DnrRequest, rules: &[(String, String, i64)]) {
        let time = self.now_ms;
        for (ext, set, id) in rules {
            if !self.exts.get(ext).is_some_and(|x| x.feedback) {
                continue;
            }
            let list = self.matched_log.entry(q.tab_id).or_default();
            list.push(MatchLog { ext: ext.clone(), set: set.clone(), id: *id, time });
            if list.len() > LOG_MAX_PER_TAB {
                let excess = list.len() - LOG_MAX_PER_TAB;
                list.drain(..excess);
            }
        }
        if time.saturating_sub(self.last_sweep) > 60_000 {
            self.last_sweep = time;
            self.matched_log.retain(|_, list| {
                list.retain(|e| e.time + LOG_WINDOW_MS >= time);
                !list.is_empty()
            });
        }
    }

    /// `getMatchedRules`.
    pub fn matched(&self, ext: &str, filter: &Value) -> ApiResult<Value> {
        let x = self.state(ext)?;
        if !x.feedback {
            return Err(ApiError::new("The 'declarativeNetRequestFeedback' permission is required."));
        }
        let min = filter.get("minTimeStamp").and_then(|m| m.as_f64()).unwrap_or(0.0);
        let since = min.max(self.now_ms.saturating_sub(LOG_WINDOW_MS) as f64);
        let tabs: Vec<i64> = match filter.get("tabId").and_then(|t| t.as_i64()) {
            Some(t) => vec![t],
            None => {
                let mut k: Vec<i64> = self.matched_log.keys().copied().collect();
                k.sort();
                k
            }
        };
        let mut out = Vec::new();
        for tab in tabs {
            for e in self.matched_log.get(&tab).into_iter().flatten() {
                if e.ext == ext && e.time as f64 >= since {
                    out.push(json!({ "rule": { "ruleId": e.id, "rulesetId": e.set }, "tabId": tab, "timeStamp": e.time }));
                }
            }
        }
        Ok(json!({ "rulesMatchedInfo": out }))
    }

    /// `testMatchOutcome`: which of the extension's rules decide for a described request.
    pub fn test_outcome(&self, ext: &str, request: &Value) -> ApiResult<Value> {
        self.state(ext)?;
        let invalid = || ApiError::new("Invalid arguments.");
        let url = request.get("url").and_then(|u| u.as_str()).ok_or_else(invalid)?;
        let ty = request.get("type").and_then(|u| u.as_str()).ok_or_else(invalid)?;
        let q = make_request(RequestInput {
            url,
            resource_type: Some(ty),
            method: Some(request.get("method").and_then(|m| m.as_str()).unwrap_or("get")),
            initiator: request.get("initiator").and_then(|i| i.as_str()),
            tab_id: Some(request.get("tabId").and_then(|t| t.as_i64()).unwrap_or(-1)),
        })
        .ok_or_else(invalid)?;
        let matches: Vec<&CompiledRule> = self.index.matching(&q).into_iter().filter(|r| r.ext == ext).collect();
        let can_act = |r: &CompiledRule| self.can_act(r, &q);
        let res = resolve(&matches, &q, &ResolveContext { rank: &|_| 0, frame_allows: None, can_act: &can_act });
        let out: Vec<Value> = res.used.iter().map(|r| json!({ "ruleId": r.id, "rulesetId": r.set })).collect();
        Ok(json!({ "matchedRules": out }))
    }

    /// `setExtensionActionOptions`.
    pub fn set_action_options(&mut self, ext: &str, options: &Value) -> ApiResult<()> {
        self.state(ext)?;
        if let Some(v) = options.get("displayActionCountAsBadgeText") {
            let on = v.as_bool() == Some(true);
            self.store.set_display_action_count(ext, on);
            if !on {
                self.counts.retain(|_, per| {
                    per.remove(ext);
                    !per.is_empty()
                });
            }
            self.push_counts();
        }
        if let Some(u) = options.get("tabUpdate") {
            let invalid = || ApiError::new("Invalid arguments.");
            let tab = u.get("tabId").and_then(|t| t.as_i64()).ok_or_else(invalid)?;
            let inc = u.get("increment").and_then(|t| t.as_f64()).filter(|f| f.is_finite()).ok_or_else(invalid)?;
            self.count(tab, Some(ext), inc.trunc() as i64);
        }
        Ok(())
    }

    /// Router dispatch of the `declarativeNetRequest.*` methods that live in the engine and the store.
    pub fn call(&mut self, ext: &str, method: &str, args: &[Value]) -> ApiResult<Value> {
        use crate::dnr::{regex_support, RuleKind};
        let arg = |i: usize| args.get(i).cloned().unwrap_or(Value::Null);
        match method {
            "declarativeNetRequest.updateDynamicRules" => self.store.update(RuleKind::Dynamic, ext, &arg(0)).map(|_| Value::Null),
            "declarativeNetRequest.getDynamicRules" => Ok(Value::Array(self.store.get(RuleKind::Dynamic, ext, &arg(0)))),
            "declarativeNetRequest.updateSessionRules" => self.store.update(RuleKind::Session, ext, &arg(0)).map(|_| Value::Null),
            "declarativeNetRequest.getSessionRules" => Ok(Value::Array(self.store.get(RuleKind::Session, ext, &arg(0)))),
            "declarativeNetRequest.isRegexSupported" => Ok(regex_support(&arg(0))),
            "declarativeNetRequest.getEnabledRulesets" => self.get_enabled_rulesets(ext).map(|v| json!(v)),
            "declarativeNetRequest.updateEnabledRulesets" => self.update_enabled_rulesets(ext, &arg(0)).map(|_| Value::Null),
            "declarativeNetRequest.getAvailableStaticRuleCount" => self.available_static_rules(ext).map(|n| json!(n)),
            "declarativeNetRequest.getMatchedRules" => self.matched(ext, &arg(0)),
            "declarativeNetRequest.testMatchOutcome" => self.test_outcome(ext, &arg(0)),
            "declarativeNetRequest.setExtensionActionOptions" => self.set_action_options(ext, &arg(0)).map(|_| Value::Null),
            _ => Err(ApiError::new(format!("Unknown Extension API - {method}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dnr::RuleKind;
    use std::fs;
    use std::path::Path;

    const ID: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const OTHER: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    struct Ext {
        id: String,
        path: PathBuf,
        manifest: Value,
    }

    fn write_ext(dir: &Path, name: &str, extra: Value, rulesets: &[(&str, Value)]) -> Ext {
        let path = dir.join(name);
        fs::create_dir_all(path.join("rules")).unwrap();
        for (file, content) in rulesets {
            let text = match content {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            fs::write(path.join("rules").join(file), text).unwrap();
        }
        let mut manifest = json!({ "permissions": ["declarativeNetRequest"] });
        for (k, v) in extra.as_object().unwrap() {
            manifest[k] = v.clone();
        }
        fs::write(path.join("manifest.json"), manifest.to_string()).unwrap();
        Ext { id: if name == "one" { ID.into() } else { OTHER.into() }, path, manifest }
    }

    fn block(id: i64, filter: &str) -> Value {
        json!({ "id": id, "priority": 1, "action": { "type": "block" }, "condition": { "urlFilter": filter } })
    }
    fn block_x(id: i64, filter: &str, extra: Value) -> Value {
        let mut r = block(id, filter);
        for (k, v) in extra.as_object().unwrap() {
            r["condition"][k] = v.clone();
        }
        r
    }

    fn details(url: &str) -> RequestDetails {
        RequestDetails { url: url.into(), method: "GET".into(), resource_type: Some("script".into()), tab_id: Some(1), referrer: Some("https://site.org/".into()), ..Default::default() }
    }

    struct T {
        engine: DnrEngine,
        ext: Ext,
    }

    fn setup(dir: &Path, ext: Ext) -> T {
        let store = DnrStore::new(dir.join("dnr.json"));
        let mut engine = DnrEngine::new(store);
        engine.set_now(1_000_000);
        engine.add_extension(&ext.id, &ext.path, "1.0", &ext.manifest);
        T { engine, ext }
    }

    impl T {
        fn call(&mut self, method: &str, args: Vec<Value>) -> ApiResult<Value> {
            let id = self.ext.id.clone();
            self.engine.call(&id, &format!("declarativeNetRequest.{method}"), &args)
        }
        fn add(&mut self, kind: RuleKind, rules: Vec<Value>) {
            let id = self.ext.id.clone();
            self.engine.store.update(kind, &id, &json!({ "addRules": rules })).unwrap();
        }
    }

    fn bare(dir: &Path) -> Ext {
        write_ext(dir, "one", json!({ "permissions": ["declarativeNetRequest", "declarativeNetRequestFeedback"], "host_permissions": ["*://*.site.org/*", "*://*.example.com/*"] }), &[])
    }

    fn ruleset_ext(dir: &Path) -> Ext {
        write_ext(
            dir,
            "one",
            json!({ "declarative_net_request": { "rule_resources": [{ "id": "ads", "enabled": true, "path": "rules/ads.json" }, { "id": "extra", "enabled": false, "path": "rules/extra.json" }] } }),
            &[
                ("ads.json", json!([block(1, "||ads.example^"), { "id": 2, "action": { "type": "NOPE" }, "condition": {} }, block_x(3, "x", json!({ "regexFilter": "y" }))])),
                ("extra.json", json!([block(10, "||extra.example^")])),
            ],
        )
    }

    #[test]
    fn static_rulesets_load_skip_invalid_rules_with_one_log_line_and_block() {
        let dir = tempfile::tempdir().unwrap();
        let mut t = setup(dir.path(), ruleset_ext(dir.path()));
        t.engine.rebuild();
        assert_eq!(t.engine.before_request(&details("https://ads.example/a.js")), RequestOutcome::Cancel);
        assert_eq!(t.engine.before_request(&details("https://extra.example/a.js")), RequestOutcome::Pass);
        assert_eq!(t.engine.before_request(&details("https://ok.example/a.js")), RequestOutcome::Pass);
        let logs = t.engine.take_log();
        assert_eq!(logs.iter().filter(|l| l.contains("skipped 2 invalid rule(s)")).count(), 1);
    }

    #[test]
    fn enabled_set_is_reported_updated_persisted_and_applied_at_once() {
        let dir = tempfile::tempdir().unwrap();
        let mut t = setup(dir.path(), ruleset_ext(dir.path()));
        t.engine.rebuild();
        assert_eq!(t.call("getEnabledRulesets", vec![]).unwrap(), json!(["ads"]));
        t.call("updateEnabledRulesets", vec![json!({ "enableRulesetIds": ["extra"], "disableRulesetIds": ["ads"] })]).unwrap();
        assert_eq!(t.call("getEnabledRulesets", vec![]).unwrap(), json!(["extra"]));
        assert_eq!(t.engine.before_request(&details("https://extra.example/a.js")), RequestOutcome::Cancel);
        assert_eq!(t.engine.before_request(&details("https://ads.example/a.js")), RequestOutcome::Pass);
        t.engine.store.flush();
        let again = DnrStore::new(dir.path().join("dnr.json"));
        assert_eq!(again.enabled_rulesets(ID, "1.0"), Some(vec!["extra".to_string()]));
        assert_eq!(again.enabled_rulesets(ID, "2.0"), None);
    }

    #[test]
    fn unknown_rulesets_are_rejected_and_the_static_count_shrinks() {
        let dir = tempfile::tempdir().unwrap();
        let mut t = setup(dir.path(), ruleset_ext(dir.path()));
        t.engine.rebuild();
        assert!(t.call("updateEnabledRulesets", vec![json!({ "enableRulesetIds": ["nope"] })]).unwrap_err().0.contains("Invalid ruleset id"));
        assert_eq!(t.call("getAvailableStaticRuleCount", vec![]).unwrap(), json!(330_000 - 1));
    }

    #[test]
    fn manifest_path_with_a_leading_slash_is_inside_the_extension() {
        let dir = tempfile::tempdir().unwrap();
        let e = write_ext(dir.path(), "slash", json!({ "declarative_net_request": { "rule_resources": [{ "id": "ads", "enabled": true, "path": "/rules/ads.json" }] } }), &[("ads.json", json!([block(1, "||ads.example^")]))]);
        let mut t = setup(dir.path(), e);
        t.engine.rebuild();
        assert_eq!(t.engine.before_request(&details("https://ads.example/a.js")), RequestOutcome::Cancel);
    }

    #[test]
    fn path_outside_the_extension_and_broken_files_are_survived() {
        let dir = tempfile::tempdir().unwrap();
        let e = write_ext(
            dir.path(),
            "one",
            json!({ "declarative_net_request": { "rule_resources": [{ "id": "out", "enabled": true, "path": "../escape.json" }, { "id": "bad", "enabled": true, "path": "rules/bad.json" }] } }),
            &[("bad.json", json!("{ not json"))],
        );
        let mut t = setup(dir.path(), e);
        t.engine.rebuild();
        let logs = t.engine.take_log();
        assert!(logs.iter().any(|l| l.contains("outside the extension")));
        assert!(logs.iter().any(|l| l.contains("\"bad\" cannot be loaded")));
    }

    #[test]
    fn stops_enforcing_when_unloaded() {
        let dir = tempfile::tempdir().unwrap();
        let mut t = setup(dir.path(), ruleset_ext(dir.path()));
        t.engine.rebuild();
        assert!(!t.engine.wanted_events().is_empty());
        t.engine.remove_extension(ID);
        t.engine.rebuild();
        assert!(t.engine.wanted_events().is_empty());
        assert_eq!(t.engine.before_request(&details("https://ads.example/a.js")), RequestOutcome::Pass);
    }

    #[test]
    fn does_nothing_without_the_permission() {
        let dir = tempfile::tempdir().unwrap();
        let mut e = ruleset_ext(dir.path());
        e.manifest["permissions"] = json!([]);
        let mut t = setup(dir.path(), e);
        t.engine.rebuild();
        assert!(t.engine.wanted_events().is_empty());
    }

    fn header_rule(id: i64, key: &str) -> Value {
        json!({ "id": id, "priority": 1, "action": { "type": "modifyHeaders", key: [{ "header": "X-Test", "operation": "set", "value": "yes" }] }, "condition": { "urlFilter": "||example.com^" } })
    }

    #[test]
    fn hooks_are_wanted_only_while_an_active_rule_needs_them() {
        let dir = tempfile::tempdir().unwrap();
        let mut t = setup(dir.path(), bare(dir.path()));
        t.engine.rebuild();
        assert!(t.engine.wanted_events().is_empty());
        t.add(RuleKind::Session, vec![block(1, "||ads.example^")]);
        t.engine.rebuild();
        assert_eq!(t.engine.wanted_events(), [DnrEvent::OnBeforeRequest]);
        t.add(RuleKind::Dynamic, vec![header_rule(2, "requestHeaders")]);
        t.engine.rebuild();
        assert_eq!(t.engine.wanted_events(), [DnrEvent::OnBeforeRequest, DnrEvent::OnBeforeSendHeaders]);
        t.engine.store.update(RuleKind::Dynamic, ID, &json!({ "removeRuleIds": [2] })).unwrap();
        t.engine.rebuild();
        assert_eq!(t.engine.wanted_events(), [DnrEvent::OnBeforeRequest]);
    }

    #[test]
    fn rebuilds_by_itself_after_a_rule_change_debounced() {
        let dir = tempfile::tempdir().unwrap();
        let mut t = setup(dir.path(), bare(dir.path()));
        t.add(RuleKind::Session, vec![block(1, "||ads.example^")]);
        t.engine.tick(1_000_010);
        assert_eq!(t.engine.before_request(&details("https://ads.example/x")), RequestOutcome::Pass, "not rebuilt yet");
        t.engine.tick(1_000_120);
        assert_eq!(t.engine.before_request(&details("https://ads.example/x")), RequestOutcome::Cancel);
    }

    #[test]
    fn changes_request_and_response_headers() {
        let dir = tempfile::tempdir().unwrap();
        let mut t = setup(dir.path(), bare(dir.path()));
        t.add(RuleKind::Dynamic, vec![header_rule(1, "requestHeaders"), header_rule(2, "responseHeaders")]);
        t.engine.rebuild();
        let mut d = details("https://example.com/a");
        d.request_headers = vec![("Accept".into(), "*/*".into())];
        assert_eq!(t.engine.send_headers(&d).unwrap(), vec![("Accept".to_string(), "*/*".to_string()), ("X-Test".to_string(), "yes".to_string())]);
        let mut d = details("https://example.com/a");
        d.response_headers = vec![("Content-Type".into(), vec!["text/html".into()])];
        assert_eq!(t.engine.received_headers(&d).unwrap(), vec![("Content-Type".to_string(), vec!["text/html".to_string()]), ("X-Test".to_string(), vec!["yes".to_string()])]);
        let mut d = details("https://other.net/a");
        d.request_headers = vec![("A".into(), "1".into())];
        assert!(t.engine.send_headers(&d).is_none());
    }

    #[test]
    fn redirects_need_host_access_to_the_url_and_the_initiator() {
        let dir = tempfile::tempdir().unwrap();
        let redirect = json!({ "id": 1, "priority": 1, "action": { "type": "redirect", "redirect": { "url": "https://safe.example/" } }, "condition": { "urlFilter": "||example.com^" } });
        let mut with_hosts = setup(dir.path(), bare(dir.path()));
        with_hosts.add(RuleKind::Dynamic, vec![redirect.clone()]);
        with_hosts.engine.rebuild();
        assert_eq!(with_hosts.engine.before_request(&details("https://example.com/a")), RequestOutcome::Redirect("https://safe.example/".into()));
        let mut elsewhere = details("https://example.com/a");
        elsewhere.referrer = Some("https://elsewhere.net/".into());
        assert_eq!(with_hosts.engine.before_request(&elsewhere), RequestOutcome::Pass);
        let dir2 = tempfile::tempdir().unwrap();
        let mut no_hosts = setup(dir2.path(), write_ext(dir2.path(), "one", json!({ "permissions": ["declarativeNetRequest"] }), &[]));
        no_hosts.add(RuleKind::Dynamic, vec![redirect, block(2, "||blocked.example^")]);
        no_hosts.engine.rebuild();
        assert_eq!(no_hosts.engine.before_request(&details("https://example.com/a")), RequestOutcome::Pass);
        assert_eq!(no_hosts.engine.before_request(&details("https://blocked.example/a")), RequestOutcome::Cancel);
    }

    #[test]
    fn upgrades_the_scheme_and_applies_main_frame_rules() {
        let dir = tempfile::tempdir().unwrap();
        let mut t = setup(dir.path(), bare(dir.path()));
        t.add(
            RuleKind::Session,
            vec![
                json!({ "id": 1, "priority": 1, "action": { "type": "upgradeScheme" }, "condition": { "urlFilter": "||example.com^", "resourceTypes": ["main_frame"] } }),
                block_x(2, "||ads.example^", json!({ "resourceTypes": ["main_frame"] })),
            ],
        );
        t.engine.rebuild();
        let main = |u: &str| RequestDetails { resource_type: Some("mainFrame".into()), referrer: Some(String::new()), ..details(u) };
        assert_eq!(t.engine.before_request(&main("http://example.com/")), RequestOutcome::Redirect("https://example.com/".into()));
        assert_eq!(t.engine.before_request(&main("https://ads.example/")), RequestOutcome::Cancel);
        assert_eq!(t.engine.before_request(&details("https://ads.example/")), RequestOutcome::Pass);
    }

    #[test]
    fn ignores_requests_made_by_another_extension() {
        let dir = tempfile::tempdir().unwrap();
        let mut t = setup(dir.path(), bare(dir.path()));
        t.add(RuleKind::Session, vec![block(1, "||ads.example^")]);
        t.engine.rebuild();
        let from = |origin: String| RequestDetails { referrer: Some(String::new()), frame_origin: Some(origin), ..details("https://ads.example/") };
        assert_eq!(t.engine.before_request(&from(format!("chrome-extension://{ID}"))), RequestOutcome::Cancel);
        assert_eq!(t.engine.before_request(&from(format!("chrome-extension://{OTHER}"))), RequestOutcome::Pass);
    }

    #[test]
    fn allow_all_requests_lasts_for_the_tab_until_the_next_navigation() {
        let dir = tempfile::tempdir().unwrap();
        let mut t = setup(dir.path(), bare(dir.path()));
        t.add(
            RuleKind::Session,
            vec![
                json!({ "id": 1, "priority": 10, "action": { "type": "allowAllRequests" }, "condition": { "urlFilter": "||trusted.org^", "resourceTypes": ["main_frame"] } }),
                block(2, "||ads.example^"),
            ],
        );
        t.engine.rebuild();
        let nav = |t: &mut T, url: &str, tab: i64| t.engine.before_request(&RequestDetails { resource_type: Some("mainFrame".into()), referrer: Some(String::new()), tab_id: Some(tab), ..details(url) });
        let sub = |t: &mut T, tab: i64| t.engine.before_request(&RequestDetails { tab_id: Some(tab), ..details("https://ads.example/a.js") });
        assert_eq!(sub(&mut t, 1), RequestOutcome::Cancel);
        nav(&mut t, "https://trusted.org/", 1);
        assert_eq!(sub(&mut t, 1), RequestOutcome::Pass);
        assert_eq!(sub(&mut t, 2), RequestOutcome::Cancel);
        nav(&mut t, "https://other.org/", 1);
        assert_eq!(sub(&mut t, 1), RequestOutcome::Cancel);
    }

    #[test]
    fn logs_matches_for_extensions_with_feedback() {
        let dir = tempfile::tempdir().unwrap();
        let mut t = setup(dir.path(), bare(dir.path()));
        t.add(RuleKind::Session, vec![block(7, "||ads.example^")]);
        t.engine.rebuild();
        t.engine.before_request(&RequestDetails { tab_id: Some(3), ..details("https://ads.example/a.js") });
        t.engine.before_request(&RequestDetails { tab_id: Some(3), ..details("https://fine.example/a.js") });
        let info = t.call("getMatchedRules", vec![json!({ "tabId": 3 })]).unwrap();
        assert_eq!(info["rulesMatchedInfo"], json!([{ "rule": { "ruleId": 7, "rulesetId": "_session" }, "tabId": 3, "timeStamp": 1_000_000 }]));
        assert_eq!(t.call("getMatchedRules", vec![json!({ "tabId": 4 })]).unwrap()["rulesMatchedInfo"], json!([]));
        assert_eq!(t.call("getMatchedRules", vec![json!({ "minTimeStamp": 2_000_000 })]).unwrap()["rulesMatchedInfo"], json!([]));
        let dir2 = tempfile::tempdir().unwrap();
        let mut plain = setup(dir2.path(), write_ext(dir2.path(), "one", json!({ "permissions": ["declarativeNetRequest"] }), &[]));
        assert!(plain.call("getMatchedRules", vec![]).unwrap_err().0.contains("declarativeNetRequestFeedback"));
    }

    #[test]
    fn answers_test_match_outcome() {
        let dir = tempfile::tempdir().unwrap();
        let mut t = setup(dir.path(), bare(dir.path()));
        t.add(RuleKind::Session, vec![block(7, "||ads.example^")]);
        t.engine.rebuild();
        assert_eq!(t.call("testMatchOutcome", vec![json!({ "url": "https://ads.example/x", "type": "script" })]).unwrap(), json!({ "matchedRules": [{ "ruleId": 7, "rulesetId": "_session" }] }));
        assert_eq!(t.call("testMatchOutcome", vec![json!({ "url": "https://fine.example/x", "type": "script" })]).unwrap(), json!({ "matchedRules": [] }));
    }

    struct Grants(std::sync::Mutex<(Vec<String>, Vec<String>)>);
    impl GrantSource for Grants {
        fn permissions_of(&self, _: &str) -> Vec<String> {
            self.0.lock().unwrap().0.clone()
        }
        fn origins_of(&self, _: &str) -> Vec<String> {
            self.0.lock().unwrap().1.clone()
        }
    }

    #[test]
    fn optional_permissions_only_count_once_granted() {
        let dir = tempfile::tempdir().unwrap();
        let grants = Arc::new(Grants(Default::default()));
        let e = write_ext(dir.path(), "one", json!({ "permissions": [], "optional_permissions": ["declarativeNetRequestWithHostAccess"], "optional_host_permissions": ["<all_urls>"] }), &[]);
        let store = DnrStore::new(dir.path().join("dnr.json"));
        let mut engine = DnrEngine::new(store);
        engine.set_grants(grants.clone());
        engine.add_extension(&e.id, &e.path, "1.0", &e.manifest);
        engine.store.update(RuleKind::Session, ID, &json!({ "addRules": [block(1, "||ads.example^")] })).unwrap();
        let outcome = |engine: &DnrEngine| engine.test_outcome(ID, &json!({ "url": "https://ads.example/x", "type": "script" })).unwrap();
        engine.rebuild();
        assert!(engine.action_counts().is_empty());
        assert_eq!(outcome(&engine), json!({ "matchedRules": [] }));
        grants.0.lock().unwrap().0 = vec!["declarativeNetRequestWithHostAccess".into()];
        engine.refresh_access(ID);
        engine.rebuild();
        assert_eq!(outcome(&engine), json!({ "matchedRules": [] }));
        grants.0.lock().unwrap().1 = vec!["https://ads.example/*".into()];
        engine.refresh_access(ID);
        engine.rebuild();
        assert_eq!(outcome(&engine), json!({ "matchedRules": [{ "ruleId": 1, "rulesetId": "_session" }] }));
    }

    fn counting(dir: &Path) -> T {
        let e = write_ext(dir, "one", json!({ "host_permissions": ["<all_urls>"] }), &[]);
        let mut t = setup(dir, e);
        t.add(
            RuleKind::Session,
            vec![
                block(1, "||ads.example^"),
                json!({ "id": 2, "priority": 1, "action": { "type": "redirect", "redirect": { "url": "https://safe.example/x" } }, "condition": { "urlFilter": "||redir.example^" } }),
                json!({ "id": 3, "priority": 1, "action": { "type": "upgradeScheme" }, "condition": { "urlFilter": "||up.example^" } }),
                json!({ "id": 4, "priority": 5, "action": { "type": "allow" }, "condition": { "urlFilter": "||fine.example^" } }),
            ],
        );
        t.engine.rebuild();
        t
    }
    fn req(t: &mut T, url: &str, tab: i64) {
        t.engine.before_request(&RequestDetails { tab_id: Some(tab), ..details(url) });
    }
    fn counts(pairs: &[(i64, u32)]) -> ActionCounts {
        pairs.iter().map(|(tab, n)| (*tab, BTreeMap::from([(ID.to_string(), *n)]))).collect()
    }

    #[test]
    fn counts_blocked_redirected_and_upgraded_requests_only_when_the_option_is_on() {
        let dir = tempfile::tempdir().unwrap();
        let mut t = counting(dir.path());
        req(&mut t, "https://ads.example/a.js", 1);
        assert!(t.engine.action_counts().is_empty());
        t.call("setExtensionActionOptions", vec![json!({ "displayActionCountAsBadgeText": true })]).unwrap();
        for u in ["https://ads.example/a.js", "https://redir.example/a.js", "http://up.example/a.js", "https://fine.example/a.js", "https://ok.example/a.js"] {
            req(&mut t, u, 1);
        }
        assert_eq!(t.engine.action_counts(), counts(&[(1, 3)]));
        req(&mut t, "https://ads.example/a.js", 2);
        assert_eq!(t.engine.action_counts(), counts(&[(1, 3), (2, 1)]));
    }

    #[test]
    fn a_main_frame_request_resets_a_tab_and_destroying_it_drops_it() {
        let dir = tempfile::tempdir().unwrap();
        let mut t = counting(dir.path());
        t.call("setExtensionActionOptions", vec![json!({ "displayActionCountAsBadgeText": true })]).unwrap();
        req(&mut t, "https://ads.example/a.js", 1);
        req(&mut t, "https://ads.example/b.js", 2);
        t.engine.before_request(&RequestDetails { resource_type: Some("mainFrame".into()), referrer: Some(String::new()), tab_id: Some(1), ..details("https://ok.example/") });
        assert_eq!(t.engine.action_counts(), counts(&[(2, 1)]));
        t.engine.tab_gone(2);
        assert!(t.engine.action_counts().is_empty());
    }

    #[test]
    fn tab_update_adds_increments_floors_at_zero_and_validates() {
        let dir = tempfile::tempdir().unwrap();
        let mut t = counting(dir.path());
        t.call("setExtensionActionOptions", vec![json!({ "displayActionCountAsBadgeText": true })]).unwrap();
        t.call("setExtensionActionOptions", vec![json!({ "tabUpdate": { "tabId": 7, "increment": 5 } })]).unwrap();
        t.call("setExtensionActionOptions", vec![json!({ "tabUpdate": { "tabId": 7, "increment": -2 } })]).unwrap();
        assert_eq!(t.engine.action_counts(), counts(&[(7, 3)]));
        t.call("setExtensionActionOptions", vec![json!({ "tabUpdate": { "tabId": 7, "increment": -10 } })]).unwrap();
        assert!(t.engine.action_counts().is_empty());
        assert!(t.call("setExtensionActionOptions", vec![json!({ "tabUpdate": { "tabId": "x", "increment": 1 } })]).unwrap_err().0.contains("Invalid arguments"));
    }

    #[test]
    fn turning_the_option_off_hides_the_counts() {
        let dir = tempfile::tempdir().unwrap();
        let mut t = counting(dir.path());
        t.call("setExtensionActionOptions", vec![json!({ "displayActionCountAsBadgeText": true })]).unwrap();
        req(&mut t, "https://ads.example/a.js", 1);
        t.call("setExtensionActionOptions", vec![json!({ "displayActionCountAsBadgeText": false })]).unwrap();
        assert!(t.engine.action_counts().is_empty());
        req(&mut t, "https://ads.example/a.js", 1);
        assert!(t.engine.action_counts().is_empty());
    }

    #[test]
    fn the_option_persists() {
        let dir = tempfile::tempdir().unwrap();
        let mut t = counting(dir.path());
        t.call("setExtensionActionOptions", vec![json!({ "displayActionCountAsBadgeText": true })]).unwrap();
        t.engine.store.flush();
        assert!(DnrStore::new(dir.path().join("dnr.json")).display_action_count(ID));
        t.call("setExtensionActionOptions", vec![json!({ "displayActionCountAsBadgeText": false })]).unwrap();
        t.engine.store.flush();
        assert!(!DnrStore::new(dir.path().join("dnr.json")).display_action_count(ID));
    }

    #[test]
    fn counts_are_pushed_at_most_every_250_ms() {
        let dir = tempfile::tempdir().unwrap();
        let mut t = counting(dir.path());
        t.call("setExtensionActionOptions", vec![json!({ "displayActionCountAsBadgeText": true })]).unwrap();
        assert!(t.engine.tick(1_000_300).is_some());
        for i in 0..50 {
            req(&mut t, &format!("https://ads.example/{i}.js"), 1);
        }
        assert!(t.engine.tick(1_000_300 + 249).is_none());
        assert_eq!(t.engine.tick(1_000_300 + 250), Some(counts(&[(1, 50)])));
        assert!(t.engine.tick(1_000_300 + 1250).is_none());
    }
}

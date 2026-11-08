//! Pure `declarativeNetRequest` matching: rule compilation, the domain index, Chrome's resolution between matching rules,
//! redirect transforms and header operations. Port of `apis/dnr-match.ts`.
//!
//! A rule is kept as the JSON value the extension gave (`Rule = serde_json::Value`); only the parts the matcher needs
//! are compiled.

use percent_encoding::{percent_decode_str, utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use regex::{Regex, RegexBuilder};
use serde_json::Value;
use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;
use url::Url;

pub type Rule = Value;

/// Resource types in Chrome's enum order (one bit each).
pub const RESOURCE_TYPES: [&str; 15] = [
    "main_frame", "sub_frame", "stylesheet", "script", "image", "font", "object", "xmlhttprequest", "ping", "csp_report", "media",
    "websocket", "webtransport", "webbundle", "other",
];

fn type_bit(t: &str) -> u32 {
    RESOURCE_TYPES.iter().position(|x| *x == t).map(|i| 1u32 << i).unwrap_or(0)
}

const ALL_TYPES: u32 = (1 << RESOURCE_TYPES.len()) - 1;
/// What a rule matches when it names no resource type: everything but navigations of the top frame.
const DEFAULT_TYPES: u32 = ALL_TYPES & !1;

/// DNR resource type of the web layer's `resourceType` (CEF reports its own enum; the web layer maps it to these names).
pub fn dnr_type(engine_type: Option<&str>) -> &'static str {
    match engine_type {
        Some("mainFrame") => "main_frame",
        Some("subFrame") => "sub_frame",
        Some("stylesheet") => "stylesheet",
        Some("script") => "script",
        Some("image") => "image",
        Some("font") => "font",
        Some("object") => "object",
        Some("xhr") => "xmlhttprequest",
        Some("ping") => "ping",
        Some("cspReport") => "csp_report",
        Some("media") => "media",
        Some("webSocket") => "websocket",
        _ => "other",
    }
}

#[derive(Debug, Clone)]
pub struct DnrRequest {
    pub url: String,
    /// Lowercase host name of the URL.
    pub host: String,
    /// Origin of the page or frame that made the request, if known.
    pub initiator: Option<String>,
    pub initiator_host: Option<String>,
    /// DNR resource type (`main_frame`, `xmlhttprequest`...).
    pub resource_type: String,
    /// Lowercase HTTP method.
    pub method: String,
    /// Tab id or -1.
    pub tab_id: i64,
    lower: OnceLock<String>,
    third_party: Cell<Option<bool>>,
}

#[derive(Default)]
pub struct RequestInput<'a> {
    pub url: &'a str,
    pub resource_type: Option<&'a str>,
    pub method: Option<&'a str>,
    pub initiator: Option<&'a str>,
    pub tab_id: Option<i64>,
}

/// Builds a request, or None for anything DNR does not see (data:, blob:, chrome-extension:...).
pub fn make_request(input: RequestInput) -> Option<DnrRequest> {
    let u = Url::parse(input.url).ok()?;
    if !matches!(u.scheme(), "http" | "https" | "ws" | "wss" | "ftp") {
        return None;
    }
    let mut initiator_host = None;
    let mut initiator = input.initiator.map(String::from);
    if let Some(raw) = input.initiator {
        match Url::parse(raw) {
            Ok(i) => {
                initiator_host = i.host_str().filter(|h| !h.is_empty()).map(|h| h.to_lowercase());
                let origin = i.origin().ascii_serialization();
                initiator = Some(if origin == "null" { raw.to_string() } else { origin });
            }
            Err(_) => initiator = None,
        }
    }
    Some(DnrRequest {
        url: u.as_str().to_string(),
        host: u.host_str().unwrap_or("").to_lowercase(),
        initiator,
        initiator_host,
        resource_type: input.resource_type.unwrap_or("other").to_string(),
        method: input.method.unwrap_or("get").to_lowercase(),
        tab_id: input.tab_id.unwrap_or(-1),
        lower: OnceLock::new(),
        third_party: Cell::new(None),
    })
}

impl DnrRequest {
    fn lower_url(&self) -> &str {
        self.lower.get_or_init(|| self.url.to_lowercase())
    }
}

// ---- domains ----

const SLD: [&str; 12] = ["co", "com", "org", "net", "gov", "edu", "ac", "or", "ne", "go", "gob", "mil"];
const SUFFIXES: [&str; 9] = ["github.io", "gitlab.io", "herokuapp.com", "blogspot.com", "appspot.com", "pages.dev", "netlify.app", "vercel.app", "web.app"];

/// Approximate eTLD+1 (no public suffix list): enough to tell first from third party.
pub fn registrable_domain(host: &str) -> String {
    if host.chars().all(|c| c.is_ascii_digit() || c == '.') || host.contains(':') {
        return host.to_string();
    }
    let labels: Vec<&str> = host.split('.').collect();
    if labels.len() <= 2 {
        return host.to_string();
    }
    let last2 = labels[labels.len() - 2..].join(".");
    if SUFFIXES.contains(&last2.as_str()) {
        return labels[labels.len() - 3..].join(".");
    }
    let tld = labels[labels.len() - 1];
    let keep = if tld.len() == 2 && SLD.contains(&labels[labels.len() - 2]) { 3 } else { 2 };
    labels[labels.len() - keep..].join(".")
}

/// `host` is one of `domains` or a subdomain of one.
fn in_domains(host: &str, domains: &[String]) -> bool {
    domains.iter().any(|d| host == d || host.ends_with(&format!(".{d}")))
}

fn is_third_party(q: &DnrRequest) -> bool {
    if q.third_party.get().is_none() {
        let v = q.initiator_host.as_ref().is_some_and(|ih| registrable_domain(&q.host) != registrable_domain(ih));
        q.third_party.set(Some(v));
    }
    q.third_party.get().unwrap_or(false)
}

// ---- urlFilter ----

pub type UrlTest = Box<dyn Fn(&str, &str) -> bool + Send + Sync>;

/// Compiles Chrome's urlFilter syntax: `||` domain anchor, `|` anchors, `^` separator, `*` wildcard.
pub fn url_filter_test(filter: &str, case_sensitive: bool) -> UrlTest {
    let mut body = filter;
    let (mut domain_anchor, mut start_anchor, mut end_anchor) = (false, false, false);
    if let Some(rest) = body.strip_prefix("||") {
        domain_anchor = true;
        body = rest;
    } else if let Some(rest) = body.strip_prefix('|') {
        start_anchor = true;
        body = rest;
    }
    if let Some(rest) = body.strip_suffix('|') {
        end_anchor = true;
        body = rest;
    }
    let plain = !body.contains(['*', '^']);
    let text = if case_sensitive { body.to_string() } else { body.to_lowercase() };
    if plain && !domain_anchor {
        return Box::new(move |u, l| {
            let s = if case_sensitive { u } else { l };
            match (start_anchor, end_anchor) {
                (true, true) => s == text,
                (true, false) => s.starts_with(&text),
                (false, true) => s.ends_with(&text),
                (false, false) => s.contains(&text),
            }
        });
    }
    let mut source = String::new();
    for c in body.chars() {
        match c {
            '*' => source.push_str(".*"),
            '^' => source.push_str("(?:[^a-zA-Z0-9_.%-]|$)"),
            other => source.push_str(&regex::escape(&other.to_string())),
        }
    }
    let prefix = if domain_anchor { "^[a-z][a-z0-9+.-]*://(?:[^/?#]*\\.)?" } else if start_anchor { "^" } else { "" };
    let pattern = format!("{prefix}{source}{}", if end_anchor { "$" } else { "" });
    let re = RegexBuilder::new(&pattern).case_insensitive(!case_sensitive).build().ok();
    // Cheap rejection before the regex: the longest literal run must be present.
    let literal = body.split(['*', '^']).fold("", |a, b| if b.chars().count() > a.chars().count() { b } else { a }).to_string();
    let lit = if case_sensitive { literal } else { literal.to_lowercase() };
    Box::new(move |u, l| {
        let Some(re) = &re else { return false };
        if lit.is_empty() {
            return re.is_match(u);
        }
        let s = if case_sensitive { u } else { l };
        s.contains(&lit) && re.is_match(u)
    })
}

/// Host that a `||host^`-style filter requires, to file the rule under it; None when the filter is not that specific.
pub fn anchored_host(filter: &str) -> Option<String> {
    let rest = filter.strip_prefix("||")?;
    let end = rest.find(|c: char| !(c.is_ascii_alphanumeric() || c == '.' || c == '-')).unwrap_or(rest.len());
    let host = &rest[..end];
    if host.is_empty() {
        return None;
    }
    let after = &rest[end..];
    let ok = matches!(after.chars().next(), Some('^' | '/' | ':' | '?')) || after == "|";
    if !ok {
        return None;
    }
    let host = host.to_lowercase();
    if host.starts_with('.') || host.ends_with('.') {
        return None;
    }
    Some(host)
}

// ---- compiled rules ----

fn lower_list(v: Option<&Value>) -> Option<Vec<String>> {
    match v {
        Some(Value::Array(a)) if !a.is_empty() => Some(a.iter().map(|d| value_string(d).to_lowercase()).collect()),
        _ => None,
    }
}

fn value_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn string_set(v: Option<&Value>) -> Option<HashSet<String>> {
    lower_list(v).map(|l| l.into_iter().collect())
}

fn numbers(v: Option<&Value>) -> Option<Vec<i64>> {
    match v {
        Some(Value::Array(a)) if !a.is_empty() => Some(a.iter().filter_map(|n| n.as_i64()).collect()),
        _ => None,
    }
}

pub struct CompiledRule {
    pub ext: String,
    /// Ruleset id: a static one, `_dynamic` or `_session`.
    pub set: String,
    pub id: i64,
    pub priority: i64,
    /// Action type (`block`, `allow`, `redirect`...).
    pub kind: String,
    pub raw: Rule,
    type_mask: u32,
    methods: Option<HashSet<String>>,
    excluded_methods: Option<HashSet<String>>,
    request_domains: Option<Vec<String>>,
    excluded_request_domains: Option<Vec<String>>,
    initiator_domains: Option<Vec<String>>,
    excluded_initiator_domains: Option<Vec<String>>,
    domain_type: Option<String>,
    tab_ids: Option<Vec<i64>>,
    excluded_tab_ids: Option<Vec<i64>>,
    url_filter: Option<String>,
    pub regex_filter: Option<String>,
    case_sensitive: bool,
    /// Index buckets (hosts the request must be on or below); empty = generic.
    keys: Vec<String>,
    url_test: OnceLock<UrlTest>,
    regex: OnceLock<Option<Regex>>,
}

impl std::fmt::Debug for CompiledRule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "CompiledRule({}:{} {} p{})", self.ext, self.id, self.kind, self.priority)
    }
}

fn non_empty_str(v: Option<&Value>) -> Option<String> {
    v.and_then(|x| x.as_str()).filter(|s| !s.is_empty()).map(String::from)
}

/// Compiles one rule; None when its condition cannot be satisfied or is invalid (skipped like Chrome).
pub fn compile_rule(ext: &str, ruleset_id: &str, rule: &Rule) -> Option<CompiledRule> {
    let c = rule.get("condition").and_then(|c| c.as_object()).cloned().unwrap_or_default();
    let url_filter = non_empty_str(c.get("urlFilter"));
    let regex_filter = non_empty_str(c.get("regexFilter"));
    if url_filter.is_some() && regex_filter.is_some() {
        return None;
    }
    let mut type_mask = DEFAULT_TYPES;
    if let Some(Value::Array(a)) = c.get("resourceTypes").filter(|v| v.as_array().is_some_and(|a| !a.is_empty())) {
        type_mask = a.iter().filter_map(|t| t.as_str()).fold(0, |m, t| m | type_bit(t));
    }
    if let Some(Value::Array(a)) = c.get("excludedResourceTypes") {
        for t in a.iter().filter_map(|t| t.as_str()) {
            type_mask &= !type_bit(t);
        }
    }
    let request_domains = lower_list(c.get("requestDomains"));
    let case_sensitive = c.get("isUrlFilterCaseSensitive").and_then(|v| v.as_bool()) == Some(true);
    let session_only = ruleset_id == "_session";
    let host = if request_domains.is_some() { None } else { url_filter.as_deref().and_then(anchored_host) };
    let priority = rule.get("priority").and_then(|p| p.as_i64().or_else(|| p.as_f64().map(|f| f as i64))).unwrap_or(1);
    Some(CompiledRule {
        ext: ext.to_string(),
        set: ruleset_id.to_string(),
        id: rule.get("id").and_then(|i| i.as_i64()).unwrap_or(0),
        priority,
        kind: rule.get("action").and_then(|a| a.get("type")).and_then(|t| t.as_str()).unwrap_or("").to_string(),
        raw: rule.clone(),
        type_mask,
        methods: string_set(c.get("requestMethods")),
        excluded_methods: string_set(c.get("excludedRequestMethods")),
        keys: request_domains.clone().unwrap_or_else(|| host.into_iter().collect()),
        request_domains,
        excluded_request_domains: lower_list(c.get("excludedRequestDomains")),
        initiator_domains: lower_list(c.get("initiatorDomains").or_else(|| c.get("domains"))),
        excluded_initiator_domains: lower_list(c.get("excludedInitiatorDomains").or_else(|| c.get("excludedDomains"))),
        domain_type: c.get("domainType").and_then(|d| d.as_str()).map(String::from),
        tab_ids: if session_only { numbers(c.get("tabIds")) } else { None },
        excluded_tab_ids: if session_only { numbers(c.get("excludedTabIds")) } else { None },
        url_filter,
        regex_filter,
        case_sensitive,
        url_test: OnceLock::new(),
        regex: OnceLock::new(),
    })
}

impl CompiledRule {
    /// The compiled regex; None when invalid (the rule never matches).
    pub fn regex(&self) -> Option<&Regex> {
        self.regex
            .get_or_init(|| {
                let pattern = self.regex_filter.as_deref()?;
                RegexBuilder::new(pattern).case_insensitive(!self.case_sensitive).build().ok()
            })
            .as_ref()
    }
}

/// Whether the rule's condition holds for the request.
pub fn rule_matches(r: &CompiledRule, q: &DnrRequest) -> bool {
    let bit = match type_bit(&q.resource_type) {
        0 => type_bit("other"),
        b => b,
    };
    if r.type_mask & bit == 0 {
        return false;
    }
    if r.methods.as_ref().is_some_and(|m| !m.contains(&q.method)) {
        return false;
    }
    if r.excluded_methods.as_ref().is_some_and(|m| m.contains(&q.method)) {
        return false;
    }
    if r.request_domains.as_ref().is_some_and(|d| !in_domains(&q.host, d)) {
        return false;
    }
    if r.excluded_request_domains.as_ref().is_some_and(|d| in_domains(&q.host, d)) {
        return false;
    }
    if let Some(d) = &r.initiator_domains {
        match &q.initiator_host {
            Some(h) if in_domains(h, d) => {}
            _ => return false,
        }
    }
    if let (Some(d), Some(h)) = (&r.excluded_initiator_domains, &q.initiator_host) {
        if in_domains(h, d) {
            return false;
        }
    }
    if let Some(dt) = &r.domain_type {
        if is_third_party(q) != (dt == "thirdParty") {
            return false;
        }
    }
    if r.tab_ids.as_ref().is_some_and(|t| !t.contains(&q.tab_id)) {
        return false;
    }
    if r.excluded_tab_ids.as_ref().is_some_and(|t| t.contains(&q.tab_id)) {
        return false;
    }
    if let Some(f) = &r.url_filter {
        let test = r.url_test.get_or_init(|| url_filter_test(f, r.case_sensitive));
        return test(&q.url, q.lower_url());
    }
    if r.regex_filter.is_some() {
        return r.regex().is_some_and(|re| re.is_match(&q.url));
    }
    true
}

// ---- index ----

/// Rules filed by the host they require (`requestDomains`, or the host of a leading `||host^`), plus the rest in a generic
/// list. A request looks up its host and every parent domain, so the cost does not grow with the number of rules about
/// other sites. Built once per rule change, never per request.
pub struct RuleIndex {
    rules: Vec<CompiledRule>,
    by_host: HashMap<String, Vec<usize>>,
    generic: Vec<usize>,
}

impl RuleIndex {
    pub fn new(rules: Vec<CompiledRule>) -> Self {
        let mut by_host: HashMap<String, Vec<usize>> = HashMap::new();
        let mut generic = Vec::new();
        for (i, r) in rules.iter().enumerate() {
            if r.keys.is_empty() {
                generic.push(i);
            } else {
                for k in &r.keys {
                    by_host.entry(k.clone()).or_default().push(i);
                }
            }
        }
        RuleIndex { rules, by_host, generic }
    }

    pub fn size(&self) -> usize {
        self.rules.len()
    }

    pub fn rules(&self) -> &[CompiledRule] {
        &self.rules
    }

    /// Number of rules that have to be looked at for a host (index tests).
    pub fn candidates(&self, host: &str) -> usize {
        let mut n = self.generic.len();
        let mut h = host;
        loop {
            n += self.by_host.get(h).map_or(0, |l| l.len());
            match h.find('.') {
                Some(dot) => h = &h[dot + 1..],
                None => return n,
            }
        }
    }

    pub fn matching(&self, q: &DnrRequest) -> Vec<&CompiledRule> {
        let mut out = Vec::new();
        let mut seen: HashSet<usize> = HashSet::new();
        let mut visit = |list: Option<&Vec<usize>>| {
            for &i in list.into_iter().flatten() {
                if seen.insert(i) && rule_matches(&self.rules[i], q) {
                    out.push(&self.rules[i]);
                }
            }
        };
        let mut h = q.host.as_str();
        loop {
            visit(self.by_host.get(h));
            match h.find('.') {
                Some(dot) => h = &h[dot + 1..],
                None => break,
            }
        }
        visit(Some(&self.generic));
        out
    }
}

// ---- resolution ----

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeaderOp {
    pub header: String,
    pub operation: String,
    pub value: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    None,
    Block,
    Allow,
    Redirect,
    Upgrade,
}

#[derive(Debug)]
pub struct Resolution<'a> {
    pub action: Action,
    /// The rule that decided (absent for an allowance inherited from the frame).
    pub rule: Option<&'a CompiledRule>,
    /// `allowAllRequests` rules that matched a frame request: remember them for the frame.
    pub frame_allows: Vec<(String, i64)>,
    pub request_headers: Vec<HeaderOp>,
    pub response_headers: Vec<HeaderOp>,
    /// Every rule that took effect, for the matched-rules log.
    pub used: Vec<&'a CompiledRule>,
}

pub struct ResolveContext<'a> {
    /// Lower comes first: Chrome gives precedence to the extension installed most recently.
    pub rank: &'a dyn Fn(&str) -> i64,
    /// Priority of the `allowAllRequests` already granted to the request's frame, per extension.
    pub frame_allows: Option<&'a HashMap<String, i64>>,
    /// Whether the extension may apply this rule here (permissions).
    pub can_act: &'a dyn Fn(&CompiledRule) -> bool,
}

fn action_rank(kind: &str) -> i64 {
    match kind {
        "allow" => 0,
        "allowAllRequests" => 1,
        "block" => 2,
        "upgradeScheme" => 3,
        "redirect" => 4,
        _ => 9,
    }
}

#[derive(Clone, Copy)]
struct Candidate<'a> {
    priority: i64,
    rank: i64,
    rule: Option<&'a CompiledRule>,
    kind: Action,
}

fn better(a: &Candidate, b: &Option<Candidate>) -> bool {
    match b {
        None => true,
        Some(b) => a.priority > b.priority || (a.priority == b.priority && a.rank < b.rank),
    }
}

/// Chrome's rule resolution; across extensions the best-ranked one with a decision wins, an `allow` ends it.
pub fn resolve<'a>(matches: &[&'a CompiledRule], q: &DnrRequest, ctx: &ResolveContext) -> Resolution<'a> {
    let mut result = Resolution { action: Action::None, rule: None, frame_allows: vec![], request_headers: vec![], response_headers: vec![], used: vec![] };
    let mut by_ext: Vec<(String, Vec<&'a CompiledRule>)> = Vec::new();
    for r in matches {
        if !(ctx.can_act)(r) {
            continue;
        }
        match by_ext.iter_mut().find(|(e, _)| *e == r.ext) {
            Some((_, l)) => l.push(r),
            None => by_ext.push((r.ext.clone(), vec![r])),
        }
    }
    let mut exts: Vec<String> = by_ext.iter().map(|(e, _)| e.clone()).collect();
    if let Some(fa) = ctx.frame_allows {
        let mut inherited: Vec<&String> = fa.keys().collect();
        inherited.sort();
        for e in inherited {
            if !exts.contains(e) {
                exts.push(e.clone());
            }
        }
    }
    exts.sort_by_key(|e| (ctx.rank)(e));
    let mut allowed_at: HashMap<String, i64> = HashMap::new();
    let mut decided = false;
    let is_frame = q.resource_type == "main_frame" || q.resource_type == "sub_frame";
    let empty: Vec<&CompiledRule> = vec![];
    for ext in &exts {
        let rules = by_ext.iter().find(|(e, _)| e == ext).map(|(_, l)| l).unwrap_or(&empty);
        let mut best: Option<Candidate> = None;
        for r in rules {
            if r.kind == "modifyHeaders" {
                continue;
            }
            let kind = match r.kind.as_str() {
                "allow" | "allowAllRequests" => Action::Allow,
                "block" => Action::Block,
                "upgradeScheme" => Action::Upgrade,
                _ => Action::Redirect,
            };
            let c = Candidate { priority: r.priority, rank: action_rank(&r.kind), rule: Some(r), kind };
            if better(&c, &best) {
                best = Some(c);
            }
        }
        if let Some(p) = ctx.frame_allows.and_then(|f| f.get(ext)) {
            let c = Candidate { priority: *p, rank: action_rank("allowAllRequests"), rule: None, kind: Action::Allow };
            if better(&c, &best) {
                best = Some(c);
            }
        }
        let Some(best) = best else { continue };
        if best.kind == Action::Allow {
            allowed_at.insert(ext.clone(), best.priority);
            if best.rule.is_some_and(|r| r.kind == "allowAllRequests") && is_frame {
                result.frame_allows.push((ext.clone(), best.priority));
            }
        }
        if decided {
            continue;
        }
        decided = true;
        result.action = best.kind;
        result.rule = best.rule;
        if let Some(r) = best.rule {
            result.used.push(r);
        }
    }
    if matches!(result.action, Action::Block | Action::Redirect | Action::Upgrade) {
        return result;
    }
    // Header changes: those with a priority above the extension's own allowance.
    let mut header: Vec<(&'a CompiledRule, i64)> = Vec::new();
    for ext in &exts {
        let floor = allowed_at.get(ext).copied().unwrap_or(0);
        let rules = by_ext.iter().find(|(e, _)| e == ext).map(|(_, l)| l).unwrap_or(&empty);
        for r in rules {
            if r.kind == "modifyHeaders" && r.priority > floor {
                header.push((r, (ctx.rank)(ext)));
            }
        }
    }
    header.sort_by(|a, b| a.1.cmp(&b.1).then(b.0.priority.cmp(&a.0.priority)));
    result.request_headers = collect_ops(&header, "requestHeaders");
    result.response_headers = collect_ops(&header, "responseHeaders");
    for (r, _) in &header {
        let a = r.raw.get("action");
        let has = |k: &str| a.and_then(|a| a.get(k)).and_then(|v| v.as_array()).is_some_and(|l| !l.is_empty());
        if has("requestHeaders") || has("responseHeaders") {
            result.used.push(r);
        }
    }
    result
}

/// Operations in precedence order. A `set` or `remove` of a header locks it against anything from a lower priority or
/// another extension; `append` and equal-priority operations combine.
fn collect_ops(rules: &[(&CompiledRule, i64)], key: &str) -> Vec<HeaderOp> {
    let mut ops = Vec::new();
    let mut locked: HashMap<String, (i64, i64)> = HashMap::new();
    for (rule, rank) in rules {
        let Some(list) = rule.raw.get("action").and_then(|a| a.get(key)).and_then(|l| l.as_array()) else { continue };
        for op in list {
            let Some(header) = op.get("header").and_then(|h| h.as_str()) else { continue };
            let operation = op.get("operation").and_then(|o| o.as_str()).unwrap_or("").to_string();
            let value = op.get("value").and_then(|v| v.as_str()).map(String::from);
            let name = header.to_lowercase();
            let lock = locked.get(&name).copied();
            if let Some((lrank, lprio)) = lock {
                if lrank != *rank || lprio > rule.priority {
                    continue;
                }
            }
            if operation != "remove" && value.is_none() {
                continue;
            }
            ops.push(HeaderOp { header: header.to_string(), operation: operation.clone(), value });
            if operation != "append" && lock.is_none() {
                locked.insert(name, (*rank, rule.priority));
            }
        }
    }
    ops
}

/// Applies header operations to a copy. Values are lists (a single-valued header has one item); `multi` false joins
/// appended values with ", " like request headers.
pub fn apply_header_ops(headers: &[(String, Vec<String>)], ops: &[HeaderOp], multi: bool) -> Vec<(String, Vec<String>)> {
    let mut out: Vec<(String, Vec<String>)> = headers.to_vec();
    for op in ops {
        let name = op.header.to_lowercase();
        let value = op.value.clone().unwrap_or_default();
        match op.operation.as_str() {
            "remove" => out.retain(|(k, _)| k.to_lowercase() != name),
            "set" => {
                out.retain(|(k, _)| k.to_lowercase() != name);
                out.push((op.header.clone(), vec![value]));
            }
            "append" => match out.iter_mut().find(|(k, _)| k.to_lowercase() == name) {
                None => out.push((op.header.clone(), vec![value])),
                Some((_, v)) => {
                    if multi {
                        v.push(value);
                    } else {
                        let joined = format!("{}, {}", v.join(", "), value);
                        *v = vec![joined];
                    }
                }
            },
            _ => {}
        }
    }
    out
}

// ---- redirects ----

const URI_COMPONENT: &AsciiSet = &NON_ALPHANUMERIC.remove(b'-').remove(b'_').remove(b'.').remove(b'!').remove(b'~').remove(b'*').remove(b'\'').remove(b'(').remove(b')');

fn encode_component(s: &str) -> String {
    utf8_percent_encode(s, URI_COMPONENT).to_string()
}

fn safe_decode(s: &str) -> String {
    let plus = s.replace('+', " ");
    percent_decode_str(&plus).decode_utf8().map(|c| c.into_owned()).unwrap_or(plus)
}

fn transform_query(search: &str, t: &Value) -> String {
    let mut parts: Vec<String> = search.trim_start_matches('?').split('&').filter(|p| !p.is_empty()).map(String::from).collect();
    let key_of = |p: &str| safe_decode(p.split('=').next().unwrap_or(""));
    if let Some(remove) = t.get("removeParams").and_then(|r| r.as_array()).filter(|r| !r.is_empty()) {
        let remove: HashSet<String> = remove.iter().filter_map(|k| k.as_str().map(String::from)).collect();
        parts.retain(|p| !remove.contains(&key_of(p)));
    }
    if let Some(adds) = t.get("addOrReplaceParams").and_then(|a| a.as_array()) {
        for add in adds {
            let key = add.get("key").and_then(|k| k.as_str()).unwrap_or("");
            let value = add.get("value").and_then(|k| k.as_str()).unwrap_or("");
            let entry = format!("{}={}", encode_component(key), encode_component(value));
            if parts.iter().any(|p| key_of(p) == key) {
                parts = parts.into_iter().map(|p| if key_of(&p) == key { entry.clone() } else { p }).collect();
            } else if add.get("replaceOnly").and_then(|r| r.as_bool()) != Some(true) {
                parts.push(entry);
            }
        }
    }
    if parts.is_empty() { String::new() } else { format!("?{}", parts.join("&")) }
}

/// Result of `redirect.transform` on a URL; None if it cannot be applied.
pub fn apply_transform(url: &str, t: &Value) -> Option<String> {
    let mut u = Url::parse(url).ok()?;
    let s = |k: &str| t.get(k).and_then(|v| v.as_str());
    if let Some(scheme) = s("scheme") {
        let _ = u.set_scheme(scheme.trim_end_matches(':'));
    }
    if let Some(v) = s("username") {
        let _ = u.set_username(v);
    }
    if let Some(v) = s("password") {
        let _ = u.set_password(if v.is_empty() { None } else { Some(v) });
    }
    if let Some(v) = s("host") {
        let _ = u.set_host(Some(v));
    }
    if let Some(v) = s("port") {
        let _ = u.set_port(if v.is_empty() { None } else { v.parse().ok() });
    }
    if let Some(v) = s("path") {
        u.set_path(v);
    }
    if let Some(v) = s("query") {
        let q = v.trim_start_matches('?');
        u.set_query(if q.is_empty() { None } else { Some(q) });
    }
    if let Some(qt) = t.get("queryTransform").filter(|q| q.is_object()) {
        let current = u.query().map(|q| format!("?{q}")).unwrap_or_default();
        let next = transform_query(&current, qt);
        let next = next.trim_start_matches('?');
        u.set_query(if next.is_empty() { None } else { Some(next) });
    }
    if let Some(v) = s("fragment") {
        let f = v.trim_start_matches('#');
        u.set_fragment(if f.is_empty() { None } else { Some(f) });
    }
    Some(u.as_str().to_string())
}

/// `regexSubstitution`: `\0`..`\9` are the groups of the match, `\\` a backslash.
pub fn substitute(re: &Regex, url: &str, replacement: &str) -> Option<String> {
    let m = re.captures(url)?;
    let chars: Vec<char> = replacement.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\\' && i + 1 < chars.len() && (chars[i + 1].is_ascii_digit() || chars[i + 1] == '\\') {
            if chars[i + 1] == '\\' {
                out.push('\\');
            } else {
                let n = chars[i + 1].to_digit(10).unwrap_or(0) as usize;
                out.push_str(m.get(n).map_or("", |g| g.as_str()));
            }
            i += 2;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    Some(out)
}

/// The URL a redirect rule sends the request to; None when there is none (invalid, or the same URL).
pub fn redirect_target(rule: &CompiledRule, q: &DnrRequest) -> Option<String> {
    let a = rule.raw.get("action")?.get("redirect")?.as_object()?;
    let target: Option<String> = if let Some(u) = a.get("url").and_then(|u| u.as_str()) {
        let parsed = Url::parse(u).ok()?;
        if parsed.scheme() == "javascript" { None } else { Some(parsed.as_str().to_string()) }
    } else if let Some(p) = a.get("extensionPath").and_then(|p| p.as_str()) {
        if p.starts_with('/') { Some(format!("chrome-extension://{}{}", rule.ext, p)) } else { None }
    } else if let Some(t) = a.get("transform").filter(|t| t.is_object()) {
        apply_transform(&q.url, t)
    } else if let Some(sub) = a.get("regexSubstitution").and_then(|s| s.as_str()) {
        let re = if rule.regex_filter.is_some() { rule.regex() } else { None };
        match re.and_then(|re| substitute(re, &q.url, sub)) {
            Some(replaced) => Some(Url::parse(&replaced).ok()?.as_str().to_string()),
            None => None,
        }
    } else {
        None
    };
    target.filter(|t| *t != q.url)
}

/// http becomes https and ws becomes wss (port 80 goes with it); None when the URL is already secure.
pub fn upgraded_url(url: &str) -> Option<String> {
    let mut u = Url::parse(url).ok()?;
    let scheme = match u.scheme() {
        "http" => "https",
        "ws" => "wss",
        _ => return None,
    };
    let port = u.port();
    u.set_scheme(scheme).ok()?;
    if port == Some(80) {
        let _ = u.set_port(None);
    }
    Some(u.as_str().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn rule_p(id: i64, condition: Value, action: Value, priority: Option<i64>, ext: &str) -> CompiledRule {
        let mut r = json!({ "id": id, "condition": condition, "action": action });
        if let Some(p) = priority {
            r["priority"] = json!(p);
        }
        compile_rule(ext, "_dynamic", &r).unwrap()
    }
    fn rule(id: i64, condition: Value) -> CompiledRule {
        rule_p(id, condition, json!({ "type": "block" }), None, "ext")
    }
    fn req(url: &str) -> DnrRequest {
        make_request(RequestInput { url, resource_type: Some("script"), ..Default::default() }).unwrap()
    }
    fn req_x(url: &str, ty: Option<&str>, initiator: Option<&str>, method: Option<&str>, tab: Option<i64>) -> DnrRequest {
        make_request(RequestInput { url, resource_type: Some(ty.unwrap_or("script")), method, initiator, tab_id: tab }).unwrap()
    }
    fn t(filter: &str, url: &str, cs: bool) -> bool {
        url_filter_test(filter, cs)(url, &url.to_lowercase())
    }

    #[test]
    fn url_filter_plain_text_case_handling() {
        assert!(t("/ads/", "https://a.com/ADS/x.js", false));
        assert!(!t("/ads/", "https://a.com/ADS/x.js", true));
        assert!(!t("banner", "https://a.com/x", false));
    }

    #[test]
    fn url_filter_anchors() {
        assert!(t("|https://a.com/", "https://a.com/x", false));
        assert!(!t("|a.com", "https://a.com/", false));
        assert!(t(".js|", "https://a.com/x.js", false));
        assert!(!t(".js|", "https://a.com/x.js?v=1", false));
        assert!(t("|https://a.com/x|", "https://a.com/x", false));
    }

    #[test]
    fn url_filter_double_bar_anchors_on_a_host_label() {
        assert!(t("||example.com^", "https://example.com/a", false));
        assert!(t("||example.com^", "https://sub.example.com/a", false));
        assert!(!t("||example.com^", "https://notexample.com/a", false));
        assert!(!t("||example.com^", "https://other.com/?u=example.com", false));
        assert!(t("||example.com/ads", "http://x.example.com/ads/1", false));
        assert!(!t("||example.com^", "https://example.com.evil.net/", false));
    }

    #[test]
    fn url_filter_caret_matches_separators() {
        assert!(t("a.com^", "https://a.com/", false));
        assert!(t("a.com^", "https://a.com", false));
        assert!(t("a.com^", "https://a.com:81/", false));
        assert!(!t("a.com^", "https://a.com.au/", false));
        assert!(!t("a.com^", "https://a.com-x/", false));
        assert!(!t("a.com^", "https://a.com_x/", false));
    }

    #[test]
    fn url_filter_star_is_a_wildcard() {
        assert!(t("/ads/*/banner", "https://a.com/ads/x/y/banner", false));
        assert!(!t("/ads/*/banner", "https://a.com/ads/banner", false));
        assert!(t("||a.com/*.gif|", "https://a.com/x/y.gif", false));
    }

    #[test]
    fn anchored_host_only_when_exact() {
        assert_eq!(anchored_host("||Ads.Example.com^").as_deref(), Some("ads.example.com"));
        assert_eq!(anchored_host("||example.com/path").as_deref(), Some("example.com"));
        assert_eq!(anchored_host("||example.com"), None);
        assert_eq!(anchored_host("||example.*"), None);
        assert_eq!(anchored_host("|https://a.com/"), None);
    }

    #[test]
    fn request_helpers() {
        assert_eq!(dnr_type(Some("mainFrame")), "main_frame");
        assert_eq!(dnr_type(Some("subFrame")), "sub_frame");
        assert_eq!(dnr_type(Some("xhr")), "xmlhttprequest");
        assert_eq!(dnr_type(Some("webSocket")), "websocket");
        assert_eq!(dnr_type(Some("cspReport")), "csp_report");
        assert_eq!(dnr_type(Some("serviceWorker")), "other");
        assert_eq!(dnr_type(None), "other");
        assert!(make_request(RequestInput { url: "data:text/plain,hi", ..Default::default() }).is_none());
        assert!(make_request(RequestInput { url: "chrome-extension://abc/x.js", ..Default::default() }).is_none());
        assert!(make_request(RequestInput { url: "not a url", ..Default::default() }).is_none());
        assert_eq!(registrable_domain("www.example.com"), "example.com");
        assert_eq!(registrable_domain("a.b.example.co.uk"), "example.co.uk");
        assert_eq!(registrable_domain("user.github.io"), "user.github.io");
        assert_eq!(registrable_domain("localhost"), "localhost");
        assert_eq!(registrable_domain("127.0.0.1"), "127.0.0.1");
    }

    #[test]
    fn default_types_exclude_main_frame() {
        let r = rule(1, json!({ "urlFilter": "a.com" }));
        assert!(rule_matches(&r, &req_x("https://a.com/", Some("script"), None, None, None)));
        assert!(rule_matches(&r, &req_x("https://a.com/", Some("sub_frame"), None, None, None)));
        assert!(!rule_matches(&r, &req_x("https://a.com/", Some("main_frame"), None, None, None)));
        let r2 = rule(2, json!({ "urlFilter": "a.com", "excludedResourceTypes": ["image"] }));
        assert!(!rule_matches(&r2, &req_x("https://a.com/", Some("main_frame"), None, None, None)));
    }

    #[test]
    fn resource_types_and_excluded() {
        let r = rule(1, json!({ "resourceTypes": ["main_frame", "xmlhttprequest"], "excludedResourceTypes": ["xmlhttprequest"] }));
        assert!(rule_matches(&r, &req_x("https://a.com/", Some("main_frame"), None, None, None)));
        assert!(!rule_matches(&r, &req_x("https://a.com/", Some("xmlhttprequest"), None, None, None)));
        assert!(!rule_matches(&r, &req_x("https://a.com/", Some("script"), None, None, None)));
    }

    #[test]
    fn request_methods() {
        let r = rule(1, json!({ "requestMethods": ["post"] }));
        assert!(rule_matches(&r, &req_x("https://a.com/", None, None, Some("POST"), None)));
        assert!(!rule_matches(&r, &req_x("https://a.com/", None, None, Some("GET"), None)));
        assert!(!rule_matches(&rule(2, json!({ "excludedRequestMethods": ["get"] })), &req_x("https://a.com/", None, None, Some("GET"), None)));
    }

    #[test]
    fn request_domains_include_subdomains() {
        let r = rule(1, json!({ "requestDomains": ["Example.com"], "excludedRequestDomains": ["no.example.com"] }));
        assert!(rule_matches(&r, &req("https://example.com/")));
        assert!(rule_matches(&r, &req("https://a.example.com/")));
        assert!(!rule_matches(&r, &req("https://no.example.com/")));
        assert!(!rule_matches(&r, &req("https://badexample.com/")));
    }

    #[test]
    fn initiator_domains() {
        let r = rule(1, json!({ "initiatorDomains": ["site.org"] }));
        assert!(rule_matches(&r, &req_x("https://x.com/", None, Some("https://www.site.org"), None, None)));
        assert!(!rule_matches(&r, &req_x("https://x.com/", None, Some("https://other.org"), None, None)));
        assert!(!rule_matches(&r, &req("https://x.com/")));
        let ex = rule(2, json!({ "excludedInitiatorDomains": ["site.org"] }));
        assert!(!rule_matches(&ex, &req_x("https://x.com/", None, Some("https://site.org"), None, None)));
        assert!(rule_matches(&ex, &req("https://x.com/")));
    }

    #[test]
    fn first_and_third_party() {
        let third = rule(1, json!({ "domainType": "thirdParty" }));
        let first = rule(2, json!({ "domainType": "firstParty" }));
        let same = req_x("https://cdn.site.org/a.js", None, Some("https://www.site.org"), None, None);
        let other = req_x("https://tracker.net/a.js", None, Some("https://www.site.org"), None, None);
        assert!(!rule_matches(&third, &same));
        assert!(rule_matches(&third, &other));
        assert!(rule_matches(&first, &same));
        assert!(!rule_matches(&first, &other));
    }

    #[test]
    fn tab_ids_for_session_rules_only() {
        let s = compile_rule("e", "_session", &json!({ "id": 1, "condition": { "tabIds": [5] }, "action": { "type": "block" } })).unwrap();
        assert!(rule_matches(&s, &req_x("https://a.com/", None, None, None, Some(5))));
        assert!(!rule_matches(&s, &req_x("https://a.com/", None, None, None, Some(6))));
        assert!(rule_matches(&rule(2, json!({ "tabIds": [5] })), &req_x("https://a.com/", None, None, None, Some(6))));
    }

    #[test]
    fn regex_filter() {
        let r = rule(1, json!({ "regexFilter": "^https://a\\.com/(ad|banner)s?/" }));
        assert!(rule_matches(&r, &req("https://a.com/ads/1")));
        assert!(!rule_matches(&r, &req("https://a.com/news/1")));
        assert!(rule_matches(&rule(2, json!({ "regexFilter": "A\\.COM" })), &req("https://a.com/")));
        assert!(!rule_matches(&rule(3, json!({ "regexFilter": "A\\.COM", "isUrlFilterCaseSensitive": true })), &req("https://a.com/")));
        assert!(!rule_matches(&rule(4, json!({ "regexFilter": "(" })), &req("https://a.com/")));
        assert!(compile_rule("e", "s", &json!({ "id": 5, "condition": { "urlFilter": "a", "regexFilter": "b" }, "action": { "type": "block" } })).is_none());
    }

    fn ids(index: &RuleIndex, url: &str) -> Vec<i64> {
        let mut v: Vec<i64> = index.matching(&req(url)).iter().map(|r| r.id).collect();
        v.sort();
        v
    }

    #[test]
    fn index_looks_up_host_parents_and_generic_rules() {
        let index = RuleIndex::new(vec![
            rule(1, json!({ "urlFilter": "||ads.example.com^" })),
            rule(2, json!({ "requestDomains": ["tracker.net", "metrics.io"] })),
            rule(3, json!({ "urlFilter": "/banner/" })),
            rule(4, json!({ "urlFilter": "||other.org^" })),
            rule(5, json!({ "urlFilter": "||example.com" })),
        ]);
        assert_eq!(ids(&index, "https://ads.example.com/x"), [1, 5]);
        assert_eq!(ids(&index, "https://deep.ads.example.com/x"), [1, 5]);
        assert_eq!(ids(&index, "https://www.tracker.net/p"), [2]);
        assert_eq!(ids(&index, "https://site.org/banner/1"), [3]);
        assert_eq!(ids(&index, "https://example.com/"), [5]);
    }

    #[test]
    fn index_only_considers_rules_of_the_host() {
        let mut many: Vec<CompiledRule> = (0..100_000).map(|i| rule(i + 1, json!({ "urlFilter": format!("||site{i}.example^") }))).collect();
        many.push(rule(200_000, json!({ "urlFilter": "/ads/" })));
        let big = RuleIndex::new(many);
        assert_eq!(big.size(), 100_001);
        assert_eq!(big.candidates("www.site42.example"), 2);
        assert_eq!(big.candidates("unrelated.org"), 1);
        assert_eq!(big.matching(&req("https://site7.example/x")).iter().map(|r| r.id).collect::<Vec<_>>(), [8]);
    }

    #[test]
    fn index_reports_a_rule_filed_under_several_buckets_once() {
        let idx = RuleIndex::new(vec![rule(1, json!({ "requestDomains": ["b.c", "c"] }))]);
        assert_eq!(idx.matching(&req("https://a.b.c/")).len(), 1);
    }

    struct Run {
        action: Action,
        rule_id: Option<i64>,
        frame_allows: Vec<(String, i64)>,
        request_headers: Vec<HeaderOp>,
    }

    fn run_with(rules: Vec<CompiledRule>, url: &str, ty: &str, rank: &dyn Fn(&str) -> i64, frame_allows: Option<&HashMap<String, i64>>, can_act: &dyn Fn(&CompiledRule) -> bool) -> Run {
        let q = req_x(url, Some(ty), None, None, None);
        let index = RuleIndex::new(rules);
        let m = index.matching(&q);
        let res = resolve(&m, &q, &ResolveContext { rank, frame_allows, can_act });
        Run { action: res.action, rule_id: res.rule.map(|r| r.id), frame_allows: res.frame_allows.clone(), request_headers: res.request_headers.clone() }
    }
    fn run(rules: Vec<CompiledRule>) -> Run {
        run_with(rules, "https://a.com/x", "script", &|_| 0, None, &|_| true)
    }
    fn mk(r: &str, id: i64) -> CompiledRule {
        rule_p(id, json!({ "urlFilter": "a.com" }), json!({ "type": r, "redirect": { "url": "https://b.com/" } }), None, "ext")
    }

    #[test]
    fn highest_priority_wins() {
        let a = |t: &str, id, p| rule_p(id, json!({ "urlFilter": "a.com" }), json!({ "type": t }), Some(p), "ext");
        assert_eq!(run(vec![a("block", 1, 1), a("allow", 2, 2)]).action, Action::Allow);
        assert_eq!(run(vec![a("block", 1, 3), a("allow", 2, 2)]).action, Action::Block);
    }

    #[test]
    fn ties_break_allow_allowall_block_upgrade_redirect() {
        assert_eq!(run(vec![mk("redirect", 1), mk("upgradeScheme", 2)]).action, Action::Upgrade);
        assert_eq!(run(vec![mk("upgradeScheme", 2), mk("block", 3)]).action, Action::Block);
        assert_eq!(run(vec![mk("block", 3), mk("allowAllRequests", 4)]).action, Action::Allow);
        assert_eq!(run(vec![mk("allowAllRequests", 4), mk("allow", 5), mk("block", 3)]).rule_id, Some(5));
    }

    #[test]
    fn priority_defaults_to_one() {
        assert_eq!(rule(1, json!({ "urlFilter": "a" })).priority, 1);
    }

    #[test]
    fn extension_rank_decides_and_allow_ends_the_decision() {
        let mk2 = || {
            vec![
                rule_p(1, json!({ "urlFilter": "a.com" }), json!({ "type": "allow" }), Some(1), "A"),
                rule_p(2, json!({ "urlFilter": "a.com" }), json!({ "type": "block" }), Some(9), "B"),
            ]
        };
        let a_first = |e: &str| if e == "A" { 0 } else { 1 };
        let b_first = |e: &str| if e == "B" { 0 } else { 1 };
        assert_eq!(run_with(mk2(), "https://a.com/x", "script", &a_first, None, &|_| true).action, Action::Allow);
        assert_eq!(run_with(mk2(), "https://a.com/x", "script", &b_first, None, &|_| true).action, Action::Block);
    }

    #[test]
    fn rules_the_extension_cannot_act_on_are_ignored() {
        let res = run_with(vec![rule(1, json!({ "urlFilter": "a.com" }))], "https://a.com/x", "script", &|_| 0, None, &|_| false);
        assert_eq!(res.action, Action::None);
    }

    fn set_header(id: i64, priority: i64, value: &str) -> CompiledRule {
        rule_p(
            id,
            json!({ "urlFilter": "a.com" }),
            json!({ "type": "modifyHeaders", "requestHeaders": [{ "header": "X-A", "operation": "set", "value": value }] }),
            Some(priority),
            "ext",
        )
    }

    #[test]
    fn modify_headers_above_the_winning_allow_never_when_blocked() {
        let allow = rule_p(9, json!({ "urlFilter": "a.com" }), json!({ "type": "allow" }), Some(5), "ext");
        let res = run(vec![allow, set_header(1, 3, "low"), set_header(2, 7, "high")]);
        assert_eq!(res.action, Action::Allow);
        assert_eq!(res.request_headers, vec![HeaderOp { header: "X-A".into(), operation: "set".into(), value: Some("high".into()) }]);
        let blocked = run(vec![rule_p(8, json!({ "urlFilter": "a.com" }), json!({ "type": "block" }), Some(1), "ext"), set_header(3, 9, "v")]);
        assert!(blocked.request_headers.is_empty());
    }

    #[test]
    fn header_combination_rules() {
        let mh = |id, priority, ops: Value, ext: &str| {
            rule_p(id, json!({ "urlFilter": "a.com" }), json!({ "type": "modifyHeaders", "requestHeaders": ops }), Some(priority), ext)
        };
        let rules = vec![
            mh(1, 5, json!([{ "header": "X-A", "operation": "set", "value": "1" }]), "A"),
            mh(2, 3, json!([{ "header": "X-A", "operation": "set", "value": "2" }]), "A"),
            mh(3, 3, json!([{ "header": "X-B", "operation": "append", "value": "b1" }]), "A"),
            mh(4, 2, json!([{ "header": "X-B", "operation": "append", "value": "b2" }]), "A"),
            mh(5, 9, json!([{ "header": "x-a", "operation": "remove" }]), "B"),
        ];
        let rank = |e: &str| if e == "A" { 0 } else { 1 };
        let res = run_with(rules, "https://a.com/x", "script", &rank, None, &|_| true);
        let ops: Vec<String> = res.request_headers.iter().map(|o| format!("{}:{}:{}", o.header, o.operation, o.value.clone().unwrap_or_default())).collect();
        assert_eq!(ops, ["X-A:set:1", "X-B:append:b1", "X-B:append:b2"]);
    }

    #[test]
    fn allow_all_requests_is_remembered_for_frames_and_inherited() {
        let allow_all = || rule_p(1, json!({ "urlFilter": "a.com", "resourceTypes": ["main_frame"] }), json!({ "type": "allowAllRequests" }), Some(4), "ext");
        let main = run_with(vec![allow_all()], "https://a.com/", "main_frame", &|_| 0, None, &|_| true);
        assert_eq!(main.action, Action::Allow);
        assert_eq!(main.frame_allows, vec![("ext".to_string(), 4)]);
        assert!(run_with(vec![allow_all()], "https://a.com/", "script", &|_| 0, None, &|_| true).frame_allows.is_empty());
        let inherited: HashMap<String, i64> = [("ext".to_string(), 4)].into();
        let block = |id, p| rule_p(id, json!({ "urlFilter": "tracker" }), json!({ "type": "block" }), Some(p), "ext");
        assert_eq!(run_with(vec![block(2, 2)], "https://tracker.net/", "script", &|_| 0, Some(&inherited), &|_| true).action, Action::Allow);
        assert_eq!(run_with(vec![block(3, 6)], "https://tracker.net/", "script", &|_| 0, Some(&inherited), &|_| true).action, Action::Block);
    }

    const EXT_ID: &str = "abcdefghijklmnopabcdefghijklmnop";

    fn redirect_rule(r: Value, condition: Value) -> CompiledRule {
        let mut cond = json!({ "urlFilter": "a.com" });
        if let (Some(c), Some(extra)) = (cond.as_object_mut(), condition.as_object()) {
            for (k, v) in extra {
                if v.is_null() {
                    c.remove(k);
                } else {
                    c.insert(k.clone(), v.clone());
                }
            }
        }
        rule_p(1, cond, json!({ "type": "redirect", "redirect": r }), Some(1), EXT_ID)
    }

    #[test]
    fn redirects_to_url_or_extension_path() {
        let q = req("https://a.com:8080/p/q?x=1&y=2#frag");
        assert_eq!(redirect_target(&redirect_rule(json!({ "url": "https://b.com/z" }), json!({})), &q).as_deref(), Some("https://b.com/z"));
        assert_eq!(redirect_target(&redirect_rule(json!({ "url": "javascript:alert(1)" }), json!({})), &q), None);
        assert_eq!(
            redirect_target(&redirect_rule(json!({ "extensionPath": "/res/noop.js" }), json!({})), &q).as_deref(),
            Some(format!("chrome-extension://{EXT_ID}/res/noop.js").as_str())
        );
        assert_eq!(redirect_target(&redirect_rule(json!({ "extensionPath": "res/noop.js" }), json!({})), &q), None);
        assert_eq!(redirect_target(&redirect_rule(json!({ "url": q.url }), json!({})), &q), None);
    }

    #[test]
    fn transforms_components() {
        let url = "https://a.com:8080/p/q?x=1&y=2#frag";
        assert_eq!(apply_transform(url, &json!({ "scheme": "http", "host": "b.com", "port": "" })).as_deref(), Some("http://b.com/p/q?x=1&y=2#frag"));
        assert_eq!(apply_transform(url, &json!({ "path": "/new", "query": "?k=v", "fragment": "#f2" })).as_deref(), Some("https://a.com:8080/new?k=v#f2"));
        assert_eq!(apply_transform(url, &json!({ "query": "", "fragment": "" })).as_deref(), Some("https://a.com:8080/p/q"));
        assert_eq!(apply_transform(url, &json!({ "username": "u", "password": "p" })).as_deref(), Some("https://u:p@a.com:8080/p/q?x=1&y=2#frag"));
    }

    #[test]
    fn query_transform() {
        let url = "https://a.com:8080/p/q?x=1&y=2#frag";
        assert_eq!(apply_transform(url, &json!({ "queryTransform": { "removeParams": ["x"] } })).as_deref(), Some("https://a.com:8080/p/q?y=2#frag"));
        assert_eq!(apply_transform(url, &json!({ "queryTransform": { "removeParams": ["x", "y"] } })).as_deref(), Some("https://a.com:8080/p/q#frag"));
        assert_eq!(
            apply_transform(url, &json!({ "queryTransform": { "addOrReplaceParams": [{ "key": "x", "value": "9" }, { "key": "z", "value": "a b" }] } })).as_deref(),
            Some("https://a.com:8080/p/q?x=9&y=2&z=a%20b#frag")
        );
        assert_eq!(
            apply_transform(url, &json!({ "queryTransform": { "addOrReplaceParams": [{ "key": "new", "value": "1", "replaceOnly": true }] } })).as_deref(),
            Some("https://a.com:8080/p/q?x=1&y=2#frag")
        );
    }

    #[test]
    fn regex_substitution() {
        let r = redirect_rule(json!({ "regexSubstitution": "https://mirror.com/\\1/\\2?o=\\0" }), json!({ "urlFilter": null, "regexFilter": "^https://a\\.com/(\\w+)/(\\w+)$" }));
        let url = req("https://a.com/foo/bar");
        assert_eq!(redirect_target(&r, &url).as_deref(), Some("https://mirror.com/foo/bar?o=https://a.com/foo/bar"));
        let r2 = redirect_rule(json!({ "regexSubstitution": "https://m.com/\\9?q=1" }), json!({ "urlFilter": null, "regexFilter": "a\\.com" }));
        assert_eq!(redirect_target(&r2, &url).as_deref(), Some("https://m.com/?q=1"));
    }

    #[test]
    fn upgrades_http_and_ws_only() {
        assert_eq!(upgraded_url("http://a.com:80/x").as_deref(), Some("https://a.com/x"));
        assert_eq!(upgraded_url("http://a.com:8080/x").as_deref(), Some("https://a.com:8080/x"));
        assert_eq!(upgraded_url("ws://a.com/s").as_deref(), Some("wss://a.com/s"));
        assert_eq!(upgraded_url("https://a.com/"), None);
    }

    fn h(items: &[(&str, &[&str])]) -> Vec<(String, Vec<String>)> {
        items.iter().map(|(k, v)| (k.to_string(), v.iter().map(|s| s.to_string()).collect())).collect()
    }
    fn op(header: &str, operation: &str, value: Option<&str>) -> HeaderOp {
        HeaderOp { header: header.into(), operation: operation.into(), value: value.map(String::from) }
    }
    fn as_map(v: Vec<(String, Vec<String>)>) -> std::collections::BTreeMap<String, Vec<String>> {
        v.into_iter().collect()
    }

    #[test]
    fn header_ops_set_append_remove_case_insensitively() {
        let out = apply_header_ops(
            &h(&[("User-Agent", &["x"]), ("Accept", &["a"])]),
            &[op("user-agent", "set", Some("y")), op("Accept", "append", Some("b")), op("X-New", "append", Some("n")), op("Missing", "remove", None)],
            false,
        );
        assert_eq!(as_map(out), as_map(h(&[("user-agent", &["y"]), ("Accept", &["a, b"]), ("X-New", &["n"])])));
    }

    #[test]
    fn header_ops_keep_arrays_for_response_headers() {
        let input = h(&[("Set-Cookie", &["a=1"]), ("X-Frame-Options", &["DENY"])]);
        let out = apply_header_ops(
            &input,
            &[op("set-cookie", "append", Some("b=2")), op("X-Frame-Options", "remove", None), op("CSP", "set", Some("default-src 'self'"))],
            true,
        );
        assert_eq!(as_map(out), as_map(h(&[("Set-Cookie", &["a=1", "b=2"]), ("CSP", &["default-src 'self'"])])));
        assert_eq!(input[1].1, ["DENY"]);
    }
}

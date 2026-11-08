//! `chrome.declarativeNetRequest` rule storage and validation. Port of `apis/dnr.ts` and `apis/lifecycle.ts`.
//!
//! Dynamic rules persist per extension (like Chrome), session rules live in memory; the enabled static rulesets are
//! remembered too. Enforcement is in `dnr_engine`, which polls [`DnrStore::take_changed`].

use crate::common::{ApiError, ApiResult, JsonStore};
use crate::dnr_match::Rule;
use crate::shared::{dnr_constants, dnr_enums};
use serde_json::{json, Value};
use std::collections::{BTreeSet, HashMap};
use std::path::Path;

fn enum_values(name: &str) -> Vec<String> {
    dnr_enums()[name].as_object().map(|o| o.values().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default()
}

fn one_of(rule_id: i64, whr: &str, list: Option<&Value>, allowed: &[String]) -> ApiResult<()> {
    let Some(list) = list else { return Ok(()) };
    let ok = list.as_array().is_some_and(|a| a.iter().all(|v| allowed.contains(&value_string(v))));
    if ok {
        Ok(())
    } else {
        Err(ApiError::new(format!("Rule with id {rule_id}: invalid {whr}, expected one of: {}.", allowed.join(", "))))
    }
}

fn value_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Returns an `ApiError` like Chrome for a rule that cannot be stored.
pub fn validate_rule(rule: &Value) -> ApiResult<Rule> {
    let obj = rule.as_object().ok_or_else(|| ApiError::new("Invalid rule."))?;
    let id = obj.get("id").and_then(|i| i.as_i64()).filter(|i| *i >= 1);
    let Some(id) = id else {
        return Err(ApiError::new(format!("Rule with id {} must have a positive integer id.", obj.get("id").map(value_string).unwrap_or_else(|| "undefined".into()))));
    };
    let actions = enum_values("RuleActionType");
    let action = obj.get("action").and_then(|a| a.as_object());
    match action.and_then(|a| a.get("type")).map(value_string) {
        Some(t) if actions.contains(&t) => {}
        _ => return Err(ApiError::new(format!("Rule with id {id}: invalid action type, expected one of: {}.", actions.join(", ")))),
    }
    let action = action.expect("checked");
    let Some(condition) = obj.get("condition").and_then(|c| c.as_object()) else {
        return Err(ApiError::new(format!("Rule with id {id}: missing condition.")));
    };
    if let Some(p) = obj.get("priority") {
        if !p.as_i64().is_some_and(|p| p >= 1) {
            return Err(ApiError::new(format!("Rule with id {id}: priority must be an integer of at least 1.")));
        }
    }
    let operations = enum_values("HeaderOperation");
    for key in ["requestHeaders", "responseHeaders"] {
        let Some(headers) = action.get(key) else { continue };
        let Some(list) = headers.as_array() else { return Err(ApiError::new(format!("Rule with id {id}: invalid {key}."))) };
        let ops = Value::Array(list.iter().map(|h| h.get("operation").cloned().unwrap_or(Value::Null)).collect());
        one_of(id, &format!("{key} operation"), Some(&ops), &operations)?;
    }
    one_of(id, "resourceTypes", condition.get("resourceTypes"), &enum_values("ResourceType"))?;
    one_of(id, "excludedResourceTypes", condition.get("excludedResourceTypes"), &enum_values("ResourceType"))?;
    one_of(id, "requestMethods", condition.get("requestMethods"), &enum_values("RequestMethod"))?;
    one_of(id, "excludedRequestMethods", condition.get("excludedRequestMethods"), &enum_values("RequestMethod"))?;
    if let Some(d) = condition.get("domainType") {
        one_of(id, "domainType", Some(&Value::Array(vec![d.clone()])), &enum_values("DomainType"))?;
    }
    Ok(rule.clone())
}

/// `isRegexSupported`: RE2 syntax, no lookarounds or backreferences. The `regex` crate is an RE2-style engine.
pub fn regex_support(o: &Value) -> Value {
    let syntax = json!({ "isSupported": false, "reason": "syntaxError" });
    let Some(re) = o.get("regex").and_then(|r| r.as_str()).filter(|r| !r.is_empty()) else { return syntax };
    if regex::Regex::new(re).is_err() {
        return syntax;
    }
    json!({ "isSupported": true })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleKind {
    Dynamic,
    Session,
}

type RuleMap = Vec<(i64, Rule)>;

fn rule_id(r: &Rule) -> i64 {
    r.get("id").and_then(|i| i.as_i64()).unwrap_or(0)
}

pub struct DnrStore {
    dynamic: HashMap<String, RuleMap>,
    session: HashMap<String, RuleMap>,
    store: JsonStore,
    enabled: HashMap<String, (String, Vec<String>)>,
    /// Extensions that asked for `displayActionCountAsBadgeText`.
    action_count: BTreeSet<String>,
    changed: bool,
    log: Vec<String>,
}

impl DnrStore {
    pub fn new(file: impl AsRef<Path>) -> Self {
        let store = JsonStore::new(file.as_ref());
        let mut s = DnrStore {
            dynamic: HashMap::new(),
            session: HashMap::new(),
            store,
            enabled: HashMap::new(),
            action_count: BTreeSet::new(),
            changed: false,
            log: vec![],
        };
        if let Some(data) = s.store.read() {
            if let Some(map) = data.get("enabled").and_then(|e| e.as_object()) {
                for (id, e) in map {
                    if let (Some(v), Some(ids)) = (e.get("version").and_then(|v| v.as_str()), e.get("ids").and_then(|i| i.as_array())) {
                        s.enabled.insert(id.clone(), (v.to_string(), ids.iter().map(value_string).collect()));
                    }
                }
            }
            if let Some(map) = data.get("actionCount").and_then(|e| e.as_object()) {
                for (id, on) in map {
                    if on.as_bool() == Some(true) {
                        s.action_count.insert(id.clone());
                    }
                }
            }
            if let Some(map) = data.get("dynamic").and_then(|e| e.as_object()) {
                for (id, rules) in map {
                    let mut out: RuleMap = Vec::new();
                    let mut dropped = 0;
                    for r in rules.as_array().into_iter().flatten() {
                        match validate_rule(r) {
                            Ok(rule) => {
                                let rid = rule_id(&rule);
                                out.retain(|(i, _)| *i != rid);
                                out.push((rid, rule));
                            }
                            Err(_) => dropped += 1, // corrupt, or written with enum names instead of Chrome's values
                        }
                    }
                    if dropped > 0 {
                        s.log.push(format!("[extension-apis] {id} declarativeNetRequest: dropped {dropped} stored rule(s) that are not valid (the extension recreates them)"));
                    }
                    if !out.is_empty() {
                        s.dynamic.insert(id.clone(), out);
                    }
                }
            }
        }
        s
    }

    /// Log lines produced since the last call.
    pub fn take_log(&mut self) -> Vec<String> {
        std::mem::take(&mut self.log)
    }

    fn snapshot(&self) -> Value {
        json!({
            "version": 1,
            "dynamic": self.dynamic.iter().map(|(id, m)| (id.clone(), Value::Array(m.iter().map(|(_, r)| r.clone()).collect()))).collect::<serde_json::Map<_, _>>(),
            "enabled": self.enabled.iter().map(|(id, (v, ids))| (id.clone(), json!({ "version": v, "ids": ids }))).collect::<serde_json::Map<_, _>>(),
            "actionCount": self.action_count.iter().map(|id| (id.clone(), json!(true))).collect::<serde_json::Map<_, _>>(),
        })
    }

    pub fn flush(&mut self) -> bool {
        let v = self.snapshot();
        self.store.flush(&v)
    }

    /// True once after any change of the rules or of the enabled rulesets.
    pub fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }

    fn map(&self, kind: RuleKind, id: &str) -> RuleMap {
        match kind {
            RuleKind::Dynamic => self.dynamic.get(id).cloned().unwrap_or_default(),
            RuleKind::Session => self.session.get(id).cloned().unwrap_or_default(),
        }
    }

    /// All-or-nothing, removals first, like Chrome.
    pub fn update(&mut self, kind: RuleKind, id: &str, options: &Value) -> ApiResult<()> {
        let invalid = || ApiError::new("Invalid arguments.");
        let empty = Value::Array(vec![]);
        let o = options.as_object();
        let remove = o.and_then(|o| o.get("removeRuleIds")).unwrap_or(&empty);
        let add = o.and_then(|o| o.get("addRules")).unwrap_or(&empty);
        let remove_ids: Vec<i64> = match remove.as_array() {
            Some(a) if a.iter().all(|n| n.is_number()) => a.iter().filter_map(|n| n.as_i64()).collect(),
            _ => return Err(invalid()),
        };
        let add = add.as_array().ok_or_else(invalid)?;
        let mut next = self.map(kind, id);
        next.retain(|(i, _)| !remove_ids.contains(i));
        for raw in add {
            let rule = validate_rule(raw)?;
            let rid = rule_id(&rule);
            if next.iter().any(|(i, _)| *i == rid) {
                return Err(ApiError::new(format!("Rule with id {rid} does not have a unique ID.")));
            }
            next.push((rid, rule));
        }
        let c = dnr_constants();
        let max = c[if kind == RuleKind::Dynamic { "MAX_NUMBER_OF_DYNAMIC_RULES" } else { "MAX_NUMBER_OF_SESSION_RULES" }].as_u64().unwrap_or(0) as usize;
        if next.len() > max {
            return Err(ApiError::new("Rule count exceeds the limit."));
        }
        match kind {
            RuleKind::Dynamic => {
                self.dynamic.insert(id.to_string(), next);
                self.store.request();
            }
            RuleKind::Session => {
                self.session.insert(id.to_string(), next);
            }
        }
        self.changed = true;
        Ok(())
    }

    /// Static rulesets the extension enabled (kept for the version they were chosen on), or None for the manifest's defaults.
    pub fn enabled_rulesets(&self, id: &str, version: &str) -> Option<Vec<String>> {
        self.enabled.get(id).filter(|(v, _)| v == version).map(|(_, ids)| ids.clone())
    }

    pub fn set_enabled_rulesets(&mut self, id: &str, version: &str, ids: Vec<String>) {
        self.enabled.insert(id.to_string(), (version.to_string(), ids));
        self.store.request();
        self.changed = true;
    }

    /// `displayActionCountAsBadgeText` of the extension (persisted, as in Chrome).
    pub fn display_action_count(&self, id: &str) -> bool {
        self.action_count.contains(id)
    }

    pub fn set_display_action_count(&mut self, id: &str, on: bool) {
        if self.action_count.contains(id) == on {
            return;
        }
        if on {
            self.action_count.insert(id.to_string());
        } else {
            self.action_count.remove(id);
        }
        self.store.request();
    }

    pub fn get(&self, kind: RuleKind, id: &str, filter: &Value) -> Vec<Rule> {
        let ids: Option<Vec<i64>> = filter.get("ruleIds").and_then(|r| r.as_array()).map(|a| a.iter().filter_map(|n| n.as_i64()).collect());
        self.map(kind, id).into_iter().filter(|(i, _)| ids.as_ref().is_none_or(|l| l.contains(i))).map(|(_, r)| r).collect()
    }

    /// Extension unloaded (reload, update): session rules are in memory only and do not survive it; the rest is kept.
    pub fn clear_session(&mut self, id: &str) {
        if self.session.remove(id).is_some() {
            self.changed = true;
        }
    }

    /// Ids with anything persisted.
    pub fn known_ids(&self) -> Vec<String> {
        let mut set: BTreeSet<String> = self.dynamic.keys().cloned().collect();
        set.extend(self.enabled.keys().cloned());
        set.extend(self.action_count.iter().cloned());
        set.into_iter().collect()
    }

    /// Extension uninstalled: its rules go with it.
    pub fn remove(&mut self, id: &str) {
        self.session.remove(id);
        let had = self.dynamic.remove(id).is_some();
        let enabled = self.enabled.remove(id).is_some();
        let counted = self.action_count.remove(id);
        if enabled || counted || had {
            self.store.request();
        }
        self.changed = true;
    }
}

/// A store that keeps data per extension across runs.
pub trait PersistedStore {
    /// Ids that have anything stored.
    fn known_ids(&self) -> Vec<String>;
    /// The extension is uninstalled: drop what is stored for it.
    fn forget(&mut self, id: &str);
}

impl PersistedStore for DnrStore {
    fn known_ids(&self) -> Vec<String> {
        DnrStore::known_ids(self)
    }
    fn forget(&mut self, id: &str) {
        self.remove(id)
    }
}

/// Forgets every id with stored data that is not installed; returns them. `forget` (uninstall) and `collect` (startup,
/// extensions that vanished) are the only ways data goes: an unload must not touch it.
pub fn collect_garbage(stores: &mut [&mut dyn PersistedStore], installed: &std::collections::HashSet<String>) -> Vec<String> {
    let known: BTreeSet<String> = stores.iter().flat_map(|s| s.known_ids()).collect();
    let gone: Vec<String> = known.into_iter().filter(|id| !installed.contains(id)).collect();
    for id in &gone {
        for s in stores.iter_mut() {
            s.forget(id);
        }
    }
    gone
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn rule(id: i64) -> Value {
        json!({
            "id": id, "priority": 1,
            "action": { "type": "modifyHeaders", "requestHeaders": [{ "header": "User-Agent", "operation": "set", "value": "x" }] },
            "condition": { "urlFilter": "|https://a.example/*", "resourceTypes": ["xmlhttprequest"] }
        })
    }
    fn with(mut r: Value, key: &str, v: Value) -> Value {
        r[key] = v;
        r
    }
    fn ids(rules: Vec<Rule>) -> Vec<i64> {
        rules.iter().map(rule_id).collect()
    }

    #[test]
    fn validate_accepts_a_header_rule_and_rejects_malformed_ones() {
        assert_eq!(rule_id(&validate_rule(&rule(1)).unwrap()), 1);
        let mut c = rule(1);
        c["condition"] = json!({ "resourceTypes": ["XMLHTTPREQUEST"] });
        let mut m = rule(1);
        m["condition"] = json!({ "requestMethods": ["GET"] });
        let mut d = rule(1);
        d["condition"] = json!({ "domainType": "THIRD_PARTY" });
        for bad in [
            with(rule(1), "action", json!({ "type": "MODIFY_HEADERS" })),
            with(rule(1), "action", json!({ "type": "modifyHeaders", "requestHeaders": [{ "header": "x", "operation": "SET" }] })),
            c,
            m,
            d,
            Value::Null,
            rule(0),
            with(rule(1), "action", json!({ "type": "NOPE" })),
            with(rule(1), "condition", Value::Null),
            with(rule(1), "priority", json!(0)),
        ] {
            assert!(validate_rule(&bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn store_adds_removes_first_filters_and_keeps_extensions_apart() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = DnrStore::new(dir.path().join("d.json"));
        s.update(RuleKind::Dynamic, A, &json!({ "addRules": [rule(1), rule(2)] })).unwrap();
        s.update(RuleKind::Dynamic, A, &json!({ "removeRuleIds": [1], "addRules": [rule(1), rule(3)] })).unwrap();
        assert_eq!(ids(s.get(RuleKind::Dynamic, A, &Value::Null)), [2, 1, 3]);
        assert_eq!(ids(s.get(RuleKind::Dynamic, A, &json!({ "ruleIds": [3] }))), [3]);
        assert!(s.get(RuleKind::Dynamic, &"b".repeat(32), &Value::Null).is_empty());
    }

    #[test]
    fn store_is_all_or_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = DnrStore::new(dir.path().join("d.json"));
        s.update(RuleKind::Dynamic, A, &json!({ "addRules": [rule(1)] })).unwrap();
        assert!(s.update(RuleKind::Dynamic, A, &json!({ "addRules": [rule(2), rule(1)] })).unwrap_err().0.contains("unique"));
        assert_eq!(ids(s.get(RuleKind::Dynamic, A, &Value::Null)), [1]);
        assert!(s.update(RuleKind::Dynamic, A, &json!({ "addRules": [rule(5), rule(5)] })).unwrap_err().0.contains("unique"));
    }

    #[test]
    fn store_persists_dynamic_rules_only_and_drops_them_on_removal() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("d.json");
        let mut s = DnrStore::new(&file);
        s.update(RuleKind::Dynamic, A, &json!({ "addRules": [rule(1)] })).unwrap();
        s.update(RuleKind::Session, A, &json!({ "addRules": [rule(9)] })).unwrap();
        assert!(s.flush());
        let mut again = DnrStore::new(&file);
        assert_eq!(again.get(RuleKind::Dynamic, A, &Value::Null).len(), 1);
        assert_eq!(again.get(RuleKind::Session, A, &Value::Null).len(), 0);
        again.remove(A);
        assert_eq!(again.get(RuleKind::Dynamic, A, &Value::Null).len(), 0);
    }

    #[test]
    fn store_drops_rules_written_with_enum_names_and_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("old.json");
        let bad = with(rule(1), "action", json!({ "type": "MODIFY_HEADERS" }));
        std::fs::write(&file, json!({ "dynamic": { A: [bad, rule(2)] } }).to_string()).unwrap();
        let mut s = DnrStore::new(&file);
        assert_eq!(ids(s.get(RuleKind::Dynamic, A, &Value::Null)), [2]);
        assert!(s.take_log()[0].contains("dropped 1"));
    }

    #[test]
    fn chrome_enum_values() {
        let e = dnr_enums();
        assert_eq!(e["RuleActionType"]["MODIFY_HEADERS"], "modifyHeaders");
        assert_eq!(e["ResourceType"]["XMLHTTPREQUEST"], "xmlhttprequest");
        assert_eq!(e["HeaderOperation"]["SET"], "set");
        assert_eq!(e["DomainType"]["THIRD_PARTY"], "thirdParty");
    }

    #[test]
    fn store_notifies_changes_and_remembers_enabled_rulesets_per_version() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = DnrStore::new(dir.path().join("d.json"));
        s.update(RuleKind::Session, A, &json!({ "addRules": [rule(1)] })).unwrap();
        assert!(s.take_changed());
        s.set_enabled_rulesets(A, "1.0", vec!["x".into()]);
        assert!(s.take_changed());
        assert_eq!(s.enabled_rulesets(A, "1.0"), Some(vec!["x".to_string()]));
        assert_eq!(s.enabled_rulesets(A, "1.1"), None);
        s.remove(A);
        assert_eq!(s.enabled_rulesets(A, "1.0"), None);
        assert!(s.take_changed());
    }

    #[test]
    fn checks_regular_expressions() {
        assert_eq!(regex_support(&json!({ "regex": "^https://a\\.com/" })), json!({ "isSupported": true }));
        assert_eq!(regex_support(&json!({ "regex": "(" }))["reason"], "syntaxError");
        assert_eq!(regex_support(&json!({ "regex": "(?=a)b" }))["isSupported"], false);
    }

    #[test]
    fn garbage_collection_drops_data_of_vanished_extensions() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = DnrStore::new(dir.path().join("d.json"));
        s.update(RuleKind::Dynamic, A, &json!({ "addRules": [rule(1)] })).unwrap();
        let b = "b".repeat(32);
        s.update(RuleKind::Dynamic, &b, &json!({ "addRules": [rule(1)] })).unwrap();
        let installed: std::collections::HashSet<String> = [b.clone()].into();
        let gone = collect_garbage(&mut [&mut s], &installed);
        assert_eq!(gone, vec![A.to_string()]);
        assert!(s.get(RuleKind::Dynamic, A, &Value::Null).is_empty());
        assert_eq!(s.get(RuleKind::Dynamic, &b, &Value::Null).len(), 1);
    }
}

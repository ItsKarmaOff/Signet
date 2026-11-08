//! `ChromeSetting` objects (`chrome.proxy.settings`, `chrome.privacy.*`). Port of `apis/settings.ts`, `apis/proxy.ts` and
//! `apis/privacy.ts`.
//!
//! State of every setting: at most one extension controls each (the last one to set it wins). Persisted so the control
//! survives a restart: the value is applied again when the controlling extension loads. Incognito scopes are accepted and
//! ignored: private windows run without extensions and without these settings.
//!
//! Applying a value to the engine is a decision handed to the app: [`ChromeSettings::take_applies`] lists the
//! [`Apply`] entries (proxy -> `EngineProxy` preference, WebRTC policy -> browser preference or switch).

use crate::common::{ApiError, ApiResult, Emitter, JsonStore};
use crate::proxyconfig::{parse_proxy_config, system_proxy};
use crate::shared::privacy_settings;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

pub const WEBRTC_KEY: &str = "privacy.network.webRTCIPHandlingPolicy";
const WEBRTC_POLICIES: [&str; 4] = ["default", "default_public_and_private_interfaces", "default_public_interface_only", "disable_non_proxied_udp"];

pub type Parser = Box<dyn Fn(&Value) -> ApiResult<Value> + Send + Sync>;

/// One `ChromeSetting`.
pub struct SettingDef {
    /// Dotted API path, also the method prefix (`proxy.settings` -> `proxy.settings.get`).
    pub key: String,
    /// Manifest permission required to read or write it.
    pub permission: String,
    pub default_value: Value,
    /// Returns the clean value or an `ApiError`.
    pub parse: Parser,
    /// The value has an effect in the engine (otherwise it is only stored and returned).
    pub applies: bool,
}

/// `chrome.proxy.settings`: applied to the profile's proxy; the default is the system proxy.
pub fn proxy_setting() -> SettingDef {
    SettingDef { key: "proxy.settings".into(), permission: "proxy".into(), default_value: system_proxy(), parse: Box::new(parse_proxy_config), applies: true }
}

/// `chrome.privacy.*` settings. Only `network.webRTCIPHandlingPolicy` does something; the others store and return their
/// value so an extension that reads or writes them keeps working.
pub fn privacy_defs() -> Vec<SettingDef> {
    let mut defs = Vec::new();
    for (ns, list) in privacy_settings() {
        for (name, default) in list {
            let key = format!("privacy.{ns}.{name}");
            if key == WEBRTC_KEY {
                defs.push(SettingDef {
                    key,
                    permission: "privacy".into(),
                    default_value: default,
                    parse: Box::new(|v| match v.as_str().filter(|s| WEBRTC_POLICIES.contains(s)) {
                        Some(_) => Ok(v.clone()),
                        None => Err(ApiError::new(format!("Invalid value for argument 1. Property 'value': Value must be one of: [{}].", WEBRTC_POLICIES.join(", ")))),
                    }),
                    applies: true,
                });
            } else {
                let is_bool = default.is_boolean();
                let ty = if is_bool { "boolean" } else { "string" };
                defs.push(SettingDef {
                    key,
                    permission: "privacy".into(),
                    default_value: default,
                    parse: Box::new(move |v| {
                        let ok = if is_bool { v.is_boolean() } else { v.is_string() };
                        if ok { Ok(v.clone()) } else { Err(ApiError::new(format!("Invalid value for argument 1. Property 'value': Expected '{ty}'."))) }
                    }),
                    applies: false,
                });
            }
        }
    }
    defs
}

/// A value to make effective in the engine.
#[derive(Debug, Clone, PartialEq)]
pub struct Apply {
    pub key: String,
    pub value: Value,
}

#[derive(Debug, Clone, PartialEq)]
struct Controller {
    extension_id: String,
    value: Value,
}

pub struct ChromeSettings {
    defs: Vec<SettingDef>,
    state: Vec<(String, Controller)>,
    store: JsonStore,
    emitter: Arc<dyn Emitter>,
    applies: Vec<Apply>,
}

impl ChromeSettings {
    pub fn new(file: impl AsRef<Path>, defs: Vec<SettingDef>, emitter: Arc<dyn Emitter>) -> Self {
        let store = JsonStore::new(file.as_ref());
        let mut s = ChromeSettings { defs, state: vec![], store, emitter, applies: vec![] };
        if let Some(Value::Object(map)) = s.store.read().and_then(|d| d.get("settings").cloned()) {
            for (key, c) in map {
                let Some(def) = s.defs.iter().find(|d| d.key == key) else { continue };
                let Some(ext) = c.get("extensionId").and_then(|e| e.as_str()) else { continue };
                // A stale or corrupt value is dropped.
                if let Ok(v) = (def.parse)(c.get("value").unwrap_or(&Value::Null)) {
                    s.state.push((key, Controller { extension_id: ext.to_string(), value: v }));
                }
            }
        }
        s
    }

    fn snapshot(&self) -> Value {
        let settings: serde_json::Map<String, Value> =
            self.state.iter().map(|(k, c)| (k.clone(), json!({ "extensionId": c.extension_id, "value": c.value }))).collect();
        json!({ "version": 1, "settings": settings })
    }

    /// Writes pending changes now (shutdown, tests).
    pub fn flush(&mut self) -> bool {
        let v = self.snapshot();
        self.store.flush(&v)
    }

    pub fn has(&self, key: &str) -> bool {
        self.defs.iter().any(|d| d.key == key)
    }

    pub fn permission_of(&self, key: &str) -> Option<&str> {
        self.defs.iter().find(|d| d.key == key).map(|d| d.permission.as_str())
    }

    /// All setting keys.
    pub fn keys(&self) -> Vec<String> {
        self.defs.iter().map(|d| d.key.clone()).collect()
    }

    fn def(&self, key: &str) -> ApiResult<&SettingDef> {
        self.defs.iter().find(|d| d.key == key).ok_or_else(|| ApiError::new(format!("Unknown setting {key}")))
    }

    fn controller(&self, key: &str) -> Option<&Controller> {
        self.state.iter().find(|(k, _)| k == key).map(|(_, c)| c)
    }

    /// Value in force: the controller's, else the default.
    pub fn effective(&self, key: &str) -> Value {
        self.controller(key).map(|c| c.value.clone()).or_else(|| self.def(key).ok().map(|d| d.default_value.clone())).unwrap_or(Value::Null)
    }

    pub fn controller_of(&self, key: &str) -> Option<&str> {
        self.controller(key).map(|c| c.extension_id.as_str())
    }

    pub fn level_of(&self, key: &str, extension_id: &str) -> &'static str {
        match self.controller_of(key) {
            None => "controllable_by_this_extension",
            Some(o) if o == extension_id => "controlled_by_this_extension",
            Some(_) => "controlled_by_other_extensions",
        }
    }

    pub fn details(&self, key: &str, extension_id: &str) -> Value {
        json!({ "value": self.effective(key), "levelOfControl": self.level_of(key, extension_id), "incognitoSpecific": false })
    }

    pub fn set(&mut self, key: &str, extension_id: &str, details: &Value) -> ApiResult<()> {
        let def = self.def(key)?;
        let value = (def.parse)(details.get("value").unwrap_or(&Value::Null))?;
        self.change(key, extension_id, Some(Controller { extension_id: extension_id.to_string(), value }));
        Ok(())
    }

    pub fn clear(&mut self, key: &str, extension_id: &str) {
        // Only the controlling extension can give the setting back.
        if self.controller_of(key) == Some(extension_id) {
            self.change(key, extension_id, None);
        }
    }

    /// Ids that control a setting.
    pub fn known_ids(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for (_, c) in &self.state {
            if !out.contains(&c.extension_id) {
                out.push(c.extension_id.clone());
            }
        }
        out
    }

    /// Extension uninstalled: it no longer controls anything. An unload (reload, update) keeps the control.
    pub fn release(&mut self, extension_id: &str) {
        let keys: Vec<String> = self.state.iter().filter(|(_, c)| c.extension_id == extension_id).map(|(k, _)| k.clone()).collect();
        for key in keys {
            self.change(&key, extension_id, None);
        }
    }

    fn change(&mut self, key: &str, caller: &str, next: Option<Controller>) {
        let before = self.controller(key).map(|c| c.extension_id.clone());
        let Ok(def) = self.def(key) else { return };
        let permission = def.permission.clone();
        let mut audience: Vec<String> = self.emitter.extensions_with(&permission);
        for id in [Some(caller.to_string()), before.clone().or(Some(caller.to_string()))].into_iter().flatten() {
            if !audience.contains(&id) {
                audience.push(id);
            }
        }
        let snapshot = |s: &Self| audience.iter().map(|id| (id.clone(), s.details(key, id).to_string())).collect::<Vec<_>>();
        let was = snapshot(self);
        self.state.retain(|(k, _)| k != key);
        let next_owner = next.as_ref().map(|c| c.extension_id.clone());
        if let Some(c) = next {
            self.state.push((key.to_string(), c));
        }
        self.store.request();
        self.queue_apply(key);
        // Listeners hear about their own level of control as well as the value. The extensions whose control changed hand
        // are woken; the others only get the event if they are running (a stopped worker will read the state when it asks).
        let now = snapshot(self);
        for ((id, a), (_, b)) in was.iter().zip(now.iter()) {
            if a == b {
                continue;
            }
            let wake = Some(id) == before.as_ref() || Some(id) == next_owner.as_ref();
            self.emitter.emit(id, &format!("{key}.onChange"), vec![self.details(key, id)], wake, true);
        }
    }

    fn queue_apply(&mut self, key: &str) {
        if self.def(key).is_ok_and(|d| d.applies) {
            let value = self.effective(key);
            self.applies.push(Apply { key: key.to_string(), value });
        }
    }

    /// Values to make effective in the engine since the last call.
    pub fn take_applies(&mut self) -> Vec<Apply> {
        std::mem::take(&mut self.applies)
    }

    /// A controlling extension loaded (startup, new profile): apply what it set.
    pub fn reapply(&mut self, extension_id: &str) {
        let keys: Vec<String> = self.state.iter().filter(|(_, c)| c.extension_id == extension_id).map(|(k, _)| k.clone()).collect();
        for k in keys {
            self.queue_apply(&k);
        }
    }

    /// Settings that currently have a controller, with their value (for contexts created later, e.g. new tabs).
    pub fn active(&self) -> Vec<(String, Value)> {
        self.state.iter().map(|(k, c)| (k.clone(), c.value.clone())).collect()
    }

    /// Router dispatch of `<key>.get`, `<key>.set`, `<key>.clear`.
    pub fn call(&mut self, extension_id: &str, method: &str, args: &[Value]) -> ApiResult<Value> {
        let (key, op) = method.rsplit_once('.').ok_or_else(|| ApiError::new(format!("Unknown Extension API - {method}")))?;
        self.def(key)?;
        match op {
            "get" => Ok(self.details(key, extension_id)),
            "set" => self.set(key, extension_id, args.first().unwrap_or(&Value::Null)).map(|_| Value::Null),
            "clear" => {
                self.clear(key, extension_id);
                Ok(Value::Null)
            }
            _ => Err(ApiError::new(format!("Unknown Extension API - {method}"))),
        }
    }

    /// Distinct permissions needed by the settings (router registration).
    pub fn permissions(&self) -> HashSet<String> {
        self.defs.iter().map(|d| d.permission.clone()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::Outbox;

    fn def() -> SettingDef {
        proxy_setting()
    }

    fn make(dir: &Path) -> (ChromeSettings, Arc<Outbox>) {
        let out = Arc::new(Outbox::new());
        out.set_holders("proxy", vec!["A".into(), "B".into()]);
        (ChromeSettings::new(dir.join("s.json"), vec![def()], out.clone()), out)
    }

    fn direct() -> Value {
        json!({ "mode": "direct" })
    }

    #[test]
    fn reports_the_default_and_who_may_control_it() {
        let dir = tempfile::tempdir().unwrap();
        let (s, _) = make(dir.path());
        assert_eq!(s.details("proxy.settings", "A"), json!({ "value": system_proxy(), "levelOfControl": "controllable_by_this_extension", "incognitoSpecific": false }));
    }

    #[test]
    fn the_setter_controls_the_others_see_another_extension() {
        let dir = tempfile::tempdir().unwrap();
        let (mut s, _) = make(dir.path());
        s.set("proxy.settings", "A", &json!({ "value": direct() })).unwrap();
        let a = s.details("proxy.settings", "A");
        assert_eq!((a["value"].clone(), a["levelOfControl"].clone()), (direct(), json!("controlled_by_this_extension")));
        assert_eq!(s.details("proxy.settings", "B")["levelOfControl"], "controlled_by_other_extensions");
        assert_eq!(s.take_applies(), vec![Apply { key: "proxy.settings".into(), value: direct() }]);
    }

    #[test]
    fn the_last_setter_wins_and_only_the_controller_can_clear() {
        let dir = tempfile::tempdir().unwrap();
        let (mut s, _) = make(dir.path());
        s.set("proxy.settings", "A", &json!({ "value": direct() })).unwrap();
        s.set("proxy.settings", "B", &json!({ "value": { "mode": "auto_detect" } })).unwrap();
        assert_eq!(s.controller_of("proxy.settings"), Some("B"));
        s.clear("proxy.settings", "A");
        assert_eq!(s.controller_of("proxy.settings"), Some("B"));
        s.take_applies();
        s.clear("proxy.settings", "B");
        assert_eq!(s.controller_of("proxy.settings"), None);
        assert_eq!(s.take_applies(), vec![Apply { key: "proxy.settings".into(), value: system_proxy() }]);
    }

    #[test]
    fn an_invalid_value_leaves_the_state_alone() {
        let dir = tempfile::tempdir().unwrap();
        let (mut s, _) = make(dir.path());
        assert!(s.set("proxy.settings", "A", &json!({ "value": { "mode": "x" } })).is_err());
        assert_eq!(s.controller_of("proxy.settings"), None);
    }

    #[test]
    fn on_change_goes_to_extensions_whose_view_changed() {
        let dir = tempfile::tempdir().unwrap();
        let (mut s, out) = make(dir.path());
        s.set("proxy.settings", "A", &json!({ "value": direct() })).unwrap();
        let ev = out.drain();
        let a = ev.iter().find(|e| e.extension_id == "A").unwrap();
        let b = ev.iter().find(|e| e.extension_id == "B").unwrap();
        assert_eq!(a.name, "proxy.settings.onChange");
        assert_eq!(a.args, vec![json!({ "value": direct(), "levelOfControl": "controlled_by_this_extension", "incognitoSpecific": false })]);
        assert!(a.wake);
        assert_eq!(b.args, vec![json!({ "value": direct(), "levelOfControl": "controlled_by_other_extensions", "incognitoSpecific": false })]);
        assert!(!b.wake);
        s.set("proxy.settings", "A", &json!({ "value": direct() })).unwrap();
        assert!(out.drain().is_empty(), "same effective value and level: nothing");
    }

    #[test]
    fn release_gives_everything_back() {
        let dir = tempfile::tempdir().unwrap();
        let (mut s, _) = make(dir.path());
        s.set("proxy.settings", "A", &json!({ "value": direct() })).unwrap();
        s.release("A");
        assert_eq!(s.controller_of("proxy.settings"), None);
        assert_eq!(s.effective("proxy.settings"), system_proxy());
    }

    #[test]
    fn persists_the_controller_and_reapplies_when_it_loads() {
        let dir = tempfile::tempdir().unwrap();
        let (mut first, _) = make(dir.path());
        first.set("proxy.settings", "A", &json!({ "value": direct() })).unwrap();
        assert!(first.flush());
        let (mut second, _) = make(dir.path());
        assert_eq!(second.controller_of("proxy.settings"), Some("A"));
        second.reapply("B");
        assert!(second.take_applies().is_empty());
        second.reapply("A");
        assert_eq!(second.take_applies(), vec![Apply { key: "proxy.settings".into(), value: direct() }]);
    }

    #[test]
    fn privacy_settings_validate_and_only_webrtc_applies() {
        let dir = tempfile::tempdir().unwrap();
        let out = Arc::new(Outbox::new());
        let mut s = ChromeSettings::new(dir.path().join("p.json"), privacy_defs(), out);
        assert!(s.has(WEBRTC_KEY));
        assert!(s.set(WEBRTC_KEY, "A", &json!({ "value": "nope" })).is_err());
        s.set(WEBRTC_KEY, "A", &json!({ "value": "disable_non_proxied_udp" })).unwrap();
        assert_eq!(s.take_applies().len(), 1);
        s.set("privacy.services.autofillEnabled", "A", &json!({ "value": false })).unwrap();
        assert!(s.take_applies().is_empty());
        assert!(s.set("privacy.services.autofillEnabled", "A", &json!({ "value": "x" })).is_err());
        assert_eq!(s.effective("privacy.websites.referrersEnabled"), json!(true));
    }

    #[test]
    fn call_dispatches_get_set_clear() {
        let dir = tempfile::tempdir().unwrap();
        let (mut s, _) = make(dir.path());
        s.call("A", "proxy.settings.set", &[json!({ "value": direct() })]).unwrap();
        assert_eq!(s.call("A", "proxy.settings.get", &[]).unwrap()["value"], direct());
        s.call("A", "proxy.settings.clear", &[]).unwrap();
        assert_eq!(s.controller_of("proxy.settings"), None);
        assert!(s.call("A", "nope.get", &[]).is_err());
    }
}

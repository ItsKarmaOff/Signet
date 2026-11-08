//! `chrome.notifications`. Port of `apis/notifications.ts`.
//!
//! The Electron code owns the native `Notification` objects. Here every shown notification has a numeric handle: the model
//! returns [`NotifEffect`]s (`Show`, `Close`) for the app to carry out, and the app reports `clicked`, `action` and
//! `closed` by handle. A handle that was replaced no longer speaks for its id.

use crate::common::{ApiError, ApiResult, Emitter};
use crate::commands::CommandPlatform;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;

pub const CLICKED_EVENT: &str = "notifications.onClicked";
pub const BUTTON_EVENT: &str = "notifications.onButtonClicked";
pub const CLOSED_EVENT: &str = "notifications.onClosed";

/// Chrome shows at most two buttons.
pub const MAX_BUTTONS: usize = 2;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChromeNotification {
    pub kind: String,
    pub icon_url: Option<String>,
    pub title: String,
    pub message: String,
    pub context_message: Option<String>,
    pub priority: Option<f64>,
    pub silent: Option<bool>,
    pub require_interaction: bool,
    pub items: Option<Vec<(String, String)>>,
    pub buttons: Vec<String>,
}

impl ChromeNotification {
    pub fn from_json(v: &Value) -> Option<Self> {
        let s = |k: &str| v.get(k).and_then(|x| x.as_str()).filter(|x| !x.is_empty()).map(String::from);
        Some(ChromeNotification {
            kind: s("type")?,
            icon_url: s("iconUrl"),
            title: s("title")?,
            message: s("message")?,
            context_message: s("contextMessage"),
            priority: v.get("priority").and_then(|p| p.as_f64()),
            silent: v.get("silent").and_then(|p| p.as_bool()),
            require_interaction: v.get("requireInteraction").and_then(|p| p.as_bool()) == Some(true),
            items: v.get("items").and_then(|i| i.as_array()).map(|a| {
                a.iter().map(|i| (i.get("title").and_then(|t| t.as_str()).unwrap_or("").to_string(), i.get("message").and_then(|t| t.as_str()).unwrap_or("").to_string())).collect()
            }),
            buttons: v.get("buttons").and_then(|b| b.as_array()).map(|a| a.iter().filter_map(|b| b.get("title").and_then(|t| t.as_str()).map(String::from)).collect()).unwrap_or_default(),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Urgency {
    Low,
    Normal,
    Critical,
}

/// What the app shows through the platform's notification service.
#[derive(Debug, Clone, PartialEq)]
pub struct NotificationSpec {
    pub title: String,
    pub subtitle: String,
    pub body: String,
    pub silent: Option<bool>,
    pub icon: Option<String>,
    pub urgency: Urgency,
    pub never_timeout: bool,
    /// Action button labels (macOS only).
    pub actions: Vec<String>,
}

pub fn urgency_of(priority: Option<f64>) -> Urgency {
    match priority {
        None => Urgency::Normal,
        Some(p) if p >= 2.0 => Urgency::Critical,
        Some(p) if p < 0.0 => Urgency::Low,
        _ => Urgency::Normal,
    }
}

/// Buttons the platform can show: macOS only (Windows would need toast XML, Linux ignores them). At most two.
pub fn buttons_of(opts: &ChromeNotification, platform: CommandPlatform) -> Vec<String> {
    if platform != CommandPlatform::Darwin {
        return vec![];
    }
    opts.buttons.iter().take(MAX_BUTTONS).cloned().collect()
}

/// Chrome notification options to the platform's. `icon` is a path or data URL already checked by the caller.
pub fn to_spec(opts: &ChromeNotification, platform: CommandPlatform, app_name: &str, icon: Option<String>) -> NotificationSpec {
    let body = match (&opts.kind[..], &opts.items) {
        ("list", Some(items)) => items.iter().map(|(t, m)| format!("{t} - {m}")).collect::<Vec<_>>().join("\n"),
        _ => opts.message.clone(),
    };
    NotificationSpec {
        title: opts.title.clone(),
        subtitle: opts.context_message.clone().unwrap_or_else(|| app_name.to_string()),
        body,
        silent: opts.silent,
        icon,
        urgency: urgency_of(opts.priority),
        never_timeout: opts.require_interaction,
        actions: buttons_of(opts, platform),
    }
}

/// An action index among the buttons shown is the extension's own index (buttons are only dropped from the end).
pub fn button_index(index: i64, opts: &ChromeNotification, platform: CommandPlatform) -> Option<usize> {
    (index >= 0 && (index as usize) < buttons_of(opts, platform).len()).then_some(index as usize)
}

/// Options of `update` laid over the ones the notification has; `type` cannot change.
pub fn merge_options(current: &ChromeNotification, changes: &Value) -> ChromeNotification {
    let mut next = current.clone();
    let Some(obj) = changes.as_object() else { return next };
    let patch = |k: &str| obj.get(k).filter(|v| !v.is_null());
    if let Some(v) = patch("iconUrl").and_then(|v| v.as_str()) {
        next.icon_url = Some(v.into());
    }
    if let Some(v) = patch("title").and_then(|v| v.as_str()) {
        next.title = v.into();
    }
    if let Some(v) = patch("message").and_then(|v| v.as_str()) {
        next.message = v.into();
    }
    if let Some(v) = patch("contextMessage").and_then(|v| v.as_str()) {
        next.context_message = Some(v.into());
    }
    if let Some(v) = patch("priority").and_then(|v| v.as_f64()) {
        next.priority = Some(v);
    }
    if let Some(v) = patch("silent").and_then(|v| v.as_bool()) {
        next.silent = Some(v);
    }
    if let Some(v) = patch("requireInteraction").and_then(|v| v.as_bool()) {
        next.require_interaction = v;
    }
    if let Some(parsed) = ChromeNotification::from_json(&json!({ "type": "x", "title": "x", "message": "x", "items": obj.get("items"), "buttons": obj.get("buttons") })) {
        if patch("items").is_some() {
            next.items = parsed.items;
        }
        if patch("buttons").is_some() {
            next.buttons = parsed.buttons;
        }
    }
    next
}

#[derive(Debug, Clone, PartialEq)]
pub enum NotifEffect {
    Show { handle: u64, spec: NotificationSpec },
    Close { handle: u64 },
}

struct Entry {
    handle: u64,
    opts: ChromeNotification,
}

pub struct ExtensionNotifications {
    emitter: Arc<dyn Emitter>,
    platform: CommandPlatform,
    app_name: String,
    /// A path or data URL for an icon, or None when it is not the extension's own file.
    icon: Box<dyn Fn(&str, &str) -> Option<String> + Send + Sync>,
    supported: bool,
    entries: HashMap<String, Vec<(String, Entry)>>,
    counter: u64,
    next_handle: u64,
    effects: Vec<NotifEffect>,
}

impl ExtensionNotifications {
    pub fn new(emitter: Arc<dyn Emitter>, platform: CommandPlatform, app_name: &str, icon: Box<dyn Fn(&str, &str) -> Option<String> + Send + Sync>, supported: bool) -> Self {
        ExtensionNotifications { emitter, platform, app_name: app_name.into(), icon, supported, entries: HashMap::new(), counter: 0, next_handle: 0, effects: vec![] }
    }

    /// Show/close work for the app, since the last call.
    pub fn take_effects(&mut self) -> Vec<NotifEffect> {
        std::mem::take(&mut self.effects)
    }

    fn find(&self, ext: &str, id: &str) -> Option<&Entry> {
        self.entries.get(ext)?.iter().find(|(k, _)| k == id).map(|(_, e)| e)
    }

    fn take(&mut self, ext: &str, id: &str) -> Option<Entry> {
        let list = self.entries.get_mut(ext)?;
        let pos = list.iter().position(|(k, _)| k == id)?;
        Some(list.remove(pos).1)
    }

    fn present(&mut self, ext: &str, id: &str, opts: ChromeNotification) -> ApiResult<()> {
        let icon = match &opts.icon_url {
            Some(url) => Some((self.icon)(ext, url).ok_or_else(|| ApiError::new("Unable to download all specified images."))?),
            None => None,
        };
        self.next_handle += 1;
        let handle = self.next_handle;
        let spec = to_spec(&opts, self.platform, &self.app_name, icon);
        self.entries.entry(ext.to_string()).or_default().push((id.to_string(), Entry { handle, opts }));
        self.effects.push(NotifEffect::Show { handle, spec });
        Ok(())
    }

    pub fn create(&mut self, ext: &str, first: &Value, second: &Value) -> ApiResult<String> {
        let named = first.is_string();
        let raw = if named { second } else { first };
        let opts = ChromeNotification::from_json(raw).ok_or_else(|| ApiError::new("Missing required notification options."))?;
        if !["basic", "image", "list", "progress"].contains(&opts.kind.as_str()) {
            return Err(ApiError::new(format!("Invalid notification type: {}.", opts.kind)));
        }
        let id = match first.as_str().filter(|s| !s.is_empty()) {
            Some(s) => s.to_string(),
            None => {
                self.counter += 1;
                format!("signet-notification-{}", self.counter)
            }
        };
        // Same id: the old one is replaced without telling the extension it was closed.
        if let Some(old) = self.take(ext, &id) {
            self.effects.push(NotifEffect::Close { handle: old.handle });
        }
        self.present(ext, &id, opts)?;
        Ok(id)
    }

    /// The platform cannot change a notification that is showing: it is shown again with the merged options.
    pub fn update(&mut self, ext: &str, id: &Value, changes: &Value) -> ApiResult<bool> {
        let Some(id) = id.as_str() else { return Ok(false) };
        let Some(entry) = self.take(ext, id) else { return Ok(false) };
        let opts = merge_options(&entry.opts, changes);
        self.effects.push(NotifEffect::Close { handle: entry.handle });
        self.present(ext, id, opts)?;
        Ok(true)
    }

    pub fn clear(&mut self, ext: &str, id: &Value) -> bool {
        let Some(id) = id.as_str() else { return false };
        let Some(entry) = self.take(ext, id) else { return false };
        self.effects.push(NotifEffect::Close { handle: entry.handle });
        self.emitter.emit(ext, CLOSED_EVENT, vec![json!(id), json!(false)], true, true);
        true
    }

    pub fn get_all(&self, ext: &str) -> Value {
        Value::Object(self.entries.get(ext).into_iter().flatten().map(|(id, _)| (id.clone(), json!(true))).collect())
    }

    fn owner(&self, handle: u64) -> Option<(String, String)> {
        self.entries.iter().find_map(|(ext, l)| l.iter().find(|(_, e)| e.handle == handle).map(|(id, _)| (ext.clone(), id.clone())))
    }

    /// The user clicked the notification.
    pub fn clicked(&mut self, handle: u64) {
        if let Some((ext, id)) = self.owner(handle) {
            self.emitter.emit(&ext, CLICKED_EVENT, vec![json!(id)], true, true);
        }
    }

    /// The user pressed an action button.
    pub fn action(&mut self, handle: u64, index: i64) {
        let Some((ext, id)) = self.owner(handle) else { return };
        let opts = self.find(&ext, &id).map(|e| e.opts.clone());
        if let Some(i) = opts.and_then(|o| button_index(index, &o, self.platform)) {
            self.emitter.emit(&ext, BUTTON_EVENT, vec![json!(id), json!(i)], true, true);
        }
    }

    /// The notification was dismissed by the user (or timed out).
    pub fn closed(&mut self, handle: u64) {
        if let Some((ext, id)) = self.owner(handle) {
            self.take(&ext, &id);
            self.emitter.emit(&ext, CLOSED_EVENT, vec![json!(id), json!(true)], true, true);
        }
    }

    /// The extension is gone: its notifications go with it, silently.
    pub fn unload(&mut self, ext: &str) {
        for (_, e) in self.entries.remove(ext).into_iter().flatten() {
            self.effects.push(NotifEffect::Close { handle: e.handle });
        }
    }

    pub fn call(&mut self, ext: &str, method: &str, args: &[Value]) -> ApiResult<Value> {
        let a = |i: usize| args.get(i).cloned().unwrap_or(Value::Null);
        match method {
            "notifications.create" => self.create(ext, &a(0), &a(1)).map(|id| json!(id)),
            "notifications.update" => self.update(ext, &a(0), &a(1)).map(|b| json!(b)),
            "notifications.clear" => Ok(json!(self.clear(ext, &a(0)))),
            "notifications.getAll" => Ok(self.get_all(ext)),
            "notifications.getPermissionLevel" => Ok(json!(if self.supported { "granted" } else { "denied" })),
            _ => Err(ApiError::new(format!("Unknown Extension API - {method}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::Outbox;
    use CommandPlatform::*;

    fn opts(extra: Value) -> Value {
        let mut v = json!({ "type": "basic", "iconUrl": "i.png", "title": "T", "message": "M" });
        for (k, val) in extra.as_object().unwrap() {
            v[k] = val.clone();
        }
        v
    }
    fn parsed(extra: Value) -> ChromeNotification {
        ChromeNotification::from_json(&opts(extra)).unwrap()
    }

    #[test]
    fn maps_title_message_priority_silent_and_require_interaction() {
        let s = to_spec(&parsed(json!({ "priority": 2, "silent": true, "requireInteraction": true, "contextMessage": "ctx" })), Linux, "Signet", None);
        assert_eq!((s.title.as_str(), s.subtitle.as_str(), s.body.as_str(), s.silent, s.urgency, s.never_timeout), ("T", "ctx", "M", Some(true), Urgency::Critical, true));
        let s = to_spec(&parsed(json!({})), Linux, "Signet", Some("/x/i.png".into()));
        assert_eq!((s.subtitle.as_str(), s.icon.as_deref(), s.urgency, s.never_timeout), ("Signet", Some("/x/i.png"), Urgency::Normal, false));
        assert_eq!(urgency_of(Some(-1.0)), Urgency::Low);
    }

    #[test]
    fn joins_list_items() {
        let n = parsed(json!({ "type": "list", "items": [{ "title": "a", "message": "1" }, { "title": "b", "message": "2" }] }));
        assert_eq!(to_spec(&n, Linux, "S", None).body, "a - 1\nb - 2");
    }

    #[test]
    fn at_most_two_buttons_as_macos_actions_and_none_elsewhere() {
        let n = parsed(json!({ "buttons": [{ "title": "Yes" }, { "title": "No" }, { "title": "Maybe" }] }));
        assert_eq!(to_spec(&n, Darwin, "S", None).actions, ["Yes", "No"]);
        assert!(to_spec(&n, Win32, "S", None).actions.is_empty());
        assert!(buttons_of(&n, Linux).is_empty());
    }

    #[test]
    fn maps_an_action_index_to_the_button_index() {
        let n = parsed(json!({ "buttons": [{ "title": "a" }, { "title": "b" }, { "title": "c" }] }));
        assert_eq!(button_index(1, &n, Darwin), Some(1));
        assert_eq!(button_index(2, &n, Darwin), None);
        assert_eq!(button_index(0, &n, Win32), None);
        assert_eq!(button_index(-1, &n, Darwin), None);
    }

    #[test]
    fn merges_update_options_and_never_changes_the_type() {
        let m = merge_options(&parsed(json!({})), &json!({ "title": "N", "type": "list", "message": null }));
        assert_eq!((m.kind.as_str(), m.title.as_str(), m.message.as_str()), ("basic", "N", "M"));
    }

    fn setup(platform: CommandPlatform) -> (ExtensionNotifications, Arc<Outbox>) {
        let out = Arc::new(Outbox::new());
        let n = ExtensionNotifications::new(out.clone(), platform, "Signet", Box::new(|_, url| if url == "bad.png" { None } else { Some(url.into()) }), true);
        (n, out)
    }
    fn shows(n: &mut ExtensionNotifications) -> Vec<(u64, NotificationSpec)> {
        n.take_effects().into_iter().filter_map(|e| if let NotifEffect::Show { handle, spec } = e { Some((handle, spec)) } else { None }).collect()
    }

    #[test]
    fn create_returns_the_id_shows_it_and_lists_it() {
        let (mut n, _) = setup(Darwin);
        assert_eq!(n.create("ext", &json!("a"), &opts(json!({}))).unwrap(), "a");
        let generated = n.create("ext", &opts(json!({})), &Value::Null).unwrap();
        assert!(generated.starts_with("signet-notification-"));
        assert_eq!(shows(&mut n).len(), 2);
        assert_eq!(n.get_all("ext").as_object().unwrap().keys().cloned().collect::<Vec<_>>(), ["a".to_string(), generated]);
        assert!(n.create("ext", &json!("b"), &json!({ "type": "basic", "title": "x" })).unwrap_err().0.contains("Missing"));
        assert!(n.create("ext", &json!("b"), &opts(json!({ "iconUrl": "bad.png" }))).unwrap_err().0.contains("images"));
    }

    #[test]
    fn click_button_click_and_close_events() {
        let (mut n, out) = setup(Darwin);
        n.create("ext", &json!("a"), &opts(json!({ "buttons": [{ "title": "Yes" }, { "title": "No" }] }))).unwrap();
        let h = shows(&mut n)[0].0;
        n.clicked(h);
        n.action(h, 1);
        n.action(h, 5);
        let ev = out.drain();
        assert_eq!((ev[0].name.as_str(), ev[0].args.clone(), ev[0].wake), ("notifications.onClicked", vec![json!("a")], true));
        assert_eq!((ev[1].name.as_str(), ev[1].args.clone()), ("notifications.onButtonClicked", vec![json!("a"), json!(1)]));
        assert_eq!(ev.len(), 2);
        n.closed(h);
        let last = out.drain();
        assert_eq!((last[0].name.as_str(), last[0].args.clone()), ("notifications.onClosed", vec![json!("a"), json!(true)]));
        assert_eq!(n.get_all("ext"), json!({}));
    }

    #[test]
    fn update_shows_the_merged_options_again_without_a_closed_event() {
        let (mut n, out) = setup(Darwin);
        assert!(!n.update("ext", &json!("nope"), &json!({ "title": "x" })).unwrap());
        n.create("ext", &json!("a"), &opts(json!({}))).unwrap();
        let h1 = shows(&mut n)[0].0;
        assert!(n.update("ext", &json!("a"), &json!({ "title": "New", "message": "Msg" })).unwrap());
        let effects = n.take_effects();
        assert!(matches!(effects[0], NotifEffect::Close { handle } if handle == h1));
        let NotifEffect::Show { handle: h2, spec } = &effects[1] else { panic!() };
        assert_eq!((spec.title.as_str(), spec.body.as_str()), ("New", "Msg"));
        assert!(out.drain().is_empty());
        n.clicked(h1);
        assert!(out.drain().is_empty(), "the replaced notification no longer speaks for the id");
        n.clicked(*h2);
        assert_eq!(out.drain()[0].name, "notifications.onClicked");
    }

    #[test]
    fn clear_reports_a_closure_not_made_by_the_user_and_unload_is_silent() {
        let (mut n, out) = setup(Darwin);
        n.create("ext", &json!("a"), &opts(json!({}))).unwrap();
        assert!(n.clear("ext", &json!("a")));
        assert!(!n.clear("ext", &json!("a")));
        assert_eq!(out.drain()[0].args, vec![json!("a"), json!(false)]);
        n.take_effects();
        n.create("ext", &json!("b"), &opts(json!({}))).unwrap();
        n.create("ext", &json!("b"), &opts(json!({ "title": "again" }))).unwrap();
        n.unload("ext");
        assert!(out.drain().is_empty());
        let closes = n.take_effects().iter().filter(|e| matches!(e, NotifEffect::Close { .. })).count();
        assert_eq!(closes, 2);
        assert_eq!(n.call("ext", "notifications.getPermissionLevel", &[]).unwrap(), json!("granted"));
    }
}

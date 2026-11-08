//! `chrome.contextMenus`: the item store, context matching and the menu tree. Port of `apis/contextmenus.ts`.
//! The app builds the native/GPUI menu from [`ContextMenus::tree`] and reports clicks with [`ContextMenus::click`].

use crate::common::{ApiError, ApiResult, Emitter, JsonStore};
use crate::urlmatch::url_matches;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

pub const MENU_CLICK_EVENT: &str = "contextMenus.onClicked";

/// Contexts of the extension button's own menu.
pub const ACTION_CONTEXTS: [&str; 3] = ["action", "browser_action", "page_action"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MenuType {
    Normal,
    Checkbox,
    Radio,
    Separator,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MenuItem {
    /// As the extension gave it (string or number); keys use its string form.
    pub id: Value,
    #[serde(rename = "type")]
    pub kind: MenuType,
    pub title: String,
    pub checked: bool,
    pub enabled: bool,
    pub visible: bool,
    pub contexts: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document_url_patterns: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_url_patterns: Option<Vec<String>>,
}

/// String form of an id (`1` and `"1"` are the same item, as in Chrome).
pub fn key_of(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn list(v: &Value) -> Option<Vec<String>> {
    v.as_array().map(|a| a.iter().map(key_of).collect())
}

/// Properties that `create` and `update` accept, applied over an item.
pub fn merge_props(item: &MenuItem, props: &Value) -> ApiResult<MenuItem> {
    let mut next = item.clone();
    if let Some(t) = props.get("type").and_then(|t| t.as_str()) {
        next.kind = match t {
            "normal" => MenuType::Normal,
            "checkbox" => MenuType::Checkbox,
            "radio" => MenuType::Radio,
            "separator" => MenuType::Separator,
            _ => return Err(ApiError::new(format!("Invalid value for argument: type '{t}'."))),
        };
    }
    if let Some(t) = props.get("title").and_then(|t| t.as_str()) {
        next.title = t.to_string();
    }
    for (key, slot) in [("checked", &mut next.checked), ("enabled", &mut next.enabled), ("visible", &mut next.visible)] {
        if let Some(b) = props.get(key).and_then(|b| b.as_bool()) {
            *slot = b;
        }
    }
    if let Some(c) = props.get("contexts") {
        let wrapped = if c.is_array() { c.clone() } else { Value::Array(vec![c.clone()]) };
        if let Some(l) = list(&wrapped) {
            next.contexts = l;
        }
    }
    if let Some(p) = props.get("documentUrlPatterns") {
        next.document_url_patterns = list(p);
    }
    if let Some(p) = props.get("targetUrlPatterns") {
        next.target_url_patterns = list(p);
    }
    if let Some(p) = props.get("parentId").filter(|p| !p.is_null()) {
        next.parent_id = Some(p.clone());
    }
    if next.kind != MenuType::Separator && next.title.is_empty() {
        return Err(ApiError::new("All menu items except for separators must have a title."));
    }
    Ok(next)
}

/// What was right-clicked.
#[derive(Debug, Clone, Default)]
pub struct MenuTarget {
    pub page_url: Option<String>,
    pub frame_url: Option<String>,
    pub link_url: Option<String>,
    pub src_url: Option<String>,
    pub selection_text: Option<String>,
    pub media_type: Option<String>,
    pub is_editable: bool,
}

fn some(s: &Option<String>) -> Option<&str> {
    s.as_deref().filter(|s| !s.is_empty())
}

/// Contexts a click falls in. `page` only applies when nothing more specific was clicked, like Chrome.
pub fn contexts_of(t: &MenuTarget) -> HashSet<String> {
    let mut out: HashSet<String> = HashSet::from(["all".to_string()]);
    if let Some(m) = some(&t.media_type).filter(|m| ["image", "video", "audio"].contains(m)) {
        out.insert(m.to_string());
    }
    if some(&t.link_url).is_some() {
        out.insert("link".into());
    }
    if some(&t.frame_url).is_some() {
        out.insert("frame".into());
    }
    if some(&t.selection_text).is_some() {
        out.insert("selection".into());
    }
    if t.is_editable {
        out.insert("editable".into());
    }
    if out.len() == 1 || (out.len() == 2 && out.contains("frame")) {
        out.insert("page".into());
    }
    out
}

/// Does the item belong in a menu for these contexts (and, for a page menu, this target)?
pub fn item_matches(item: &MenuItem, contexts: &HashSet<String>, target: Option<&MenuTarget>) -> bool {
    if !item.visible {
        return false;
    }
    let wanted: Vec<String> = if item.contexts.is_empty() { vec!["page".into()] } else { item.contexts.clone() };
    if !wanted.iter().any(|c| contexts.contains(c)) {
        return false;
    }
    if let Some(t) = target {
        let target_url = some(&t.src_url).or(some(&t.link_url));
        let document_url = some(&t.frame_url).or(some(&t.page_url));
        if let (Some(patterns), Some(url)) = (item.target_url_patterns.as_ref().filter(|p| !p.is_empty()), target_url) {
            if !patterns.iter().any(|p| url_matches(p, url)) {
                return false;
            }
        }
        if let (Some(patterns), Some(url)) = (item.document_url_patterns.as_ref().filter(|p| !p.is_empty()), document_url) {
            if !patterns.iter().any(|p| url_matches(p, url)) {
                return false;
            }
        }
    }
    true
}

/// An entry to show: the item, its label for this click and its submenu.
#[derive(Debug, Clone, PartialEq)]
pub struct MenuNode {
    pub item: MenuItem,
    pub label: String,
    pub children: Vec<MenuNode>,
}

/// `%s` stands for the selected text.
pub fn label_for(title: &str, selection: Option<&str>) -> String {
    match selection {
        Some(s) if !s.is_empty() && title.contains("%s") => title.replace("%s", s),
        _ => title.to_string(),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClickInfo {
    pub menu_item_id: Value,
    pub parent_menu_item_id: Option<Value>,
    pub checked: Option<bool>,
    pub was_checked: Option<bool>,
}

impl ClickInfo {
    pub fn to_json(&self) -> Value {
        let mut v = json!({ "menuItemId": self.menu_item_id });
        if let Some(p) = &self.parent_menu_item_id {
            v["parentMenuItemId"] = p.clone();
        }
        if let Some(c) = self.checked {
            v["checked"] = json!(c);
        }
        if let Some(c) = self.was_checked {
            v["wasChecked"] = json!(c);
        }
        v
    }
}

type Items = Vec<(String, MenuItem)>;

fn find<'a>(items: &'a Items, key: &str) -> Option<&'a MenuItem> {
    items.iter().find(|(k, _)| k == key).map(|(_, i)| i)
}

/// Items of every extension, with the rules of `chrome.contextMenus`.
pub struct ContextMenus {
    items: HashMap<String, Items>,
    counter: u64,
    store: Option<JsonStore>,
    /// Items read from disk, until their extension loads.
    saved: HashMap<String, Vec<MenuItem>>,
    /// `extensionId/id` of items that exist only because they were restored: a `create` with that id replaces them.
    restored: HashSet<String>,
    emitter: Arc<dyn Emitter>,
}

impl ContextMenus {
    pub fn new(emitter: Arc<dyn Emitter>) -> Self {
        ContextMenus { items: HashMap::new(), counter: 0, store: None, saved: HashMap::new(), restored: HashSet::new(), emitter }
    }

    /// Keeps the items in `file` (Chrome keeps them across restarts).
    pub fn persist(&mut self, file: impl AsRef<Path>) {
        let store = JsonStore::new(file.as_ref());
        if let Some(Value::Object(map)) = store.read() {
            for (id, items) in map {
                let parsed: Vec<MenuItem> = items.as_array().into_iter().flatten().filter_map(|i| serde_json::from_value(i.clone()).ok()).collect();
                if !parsed.is_empty() {
                    self.saved.insert(id, parsed);
                }
            }
        }
        self.store = Some(store);
    }

    /// The extension loaded: its saved items come back (those it already created this run win).
    pub fn restore(&mut self, extension_id: &str) {
        let Some(saved) = self.saved.remove(extension_id).filter(|s| !s.is_empty()) else { return };
        let items = self.items.entry(extension_id.to_string()).or_default();
        for item in saved {
            let k = key_of(&item.id);
            if find(items, &k).is_some() {
                continue;
            }
            self.restored.insert(format!("{extension_id}/{k}"));
            items.push((k, item));
        }
    }

    pub fn flush(&mut self) {
        let snapshot: serde_json::Map<String, Value> = self
            .items
            .iter()
            .filter(|(_, m)| !m.is_empty())
            .map(|(id, m)| (id.clone(), Value::Array(m.iter().filter_map(|(_, i)| serde_json::to_value(i).ok()).collect())))
            .collect();
        if let Some(s) = &mut self.store {
            s.flush(&Value::Object(snapshot));
        }
    }

    fn changed(&mut self) {
        if let Some(s) = &mut self.store {
            s.request();
        }
    }

    pub fn create(&mut self, extension_id: &str, raw: &Value) -> ApiResult<Value> {
        let id = match raw.get("id") {
            Some(v) if v.is_string() || v.is_number() => v.clone(),
            _ => {
                self.counter += 1;
                json!(format!("signet-menu-{}", self.counter))
            }
        };
        let k = key_of(&id);
        let restored_key = format!("{extension_id}/{k}");
        let items = self.items.entry(extension_id.to_string()).or_default();
        if find(items, &k).is_some() {
            // An id that only came back from disk is replaced (the extension recreates its items at startup).
            if !self.restored.contains(&restored_key) {
                return Err(ApiError::new(format!("Cannot create item with duplicate id {k}")));
            }
            self.restored.remove(&restored_key);
            items.retain(|(key, _)| *key != k);
        }
        if let Some(p) = raw.get("parentId").filter(|p| !p.is_null()) {
            if find(items, &key_of(p)).is_none() {
                return Err(ApiError::new(format!("Cannot find menu item with id {}", key_of(p))));
            }
        }
        let base = MenuItem {
            id: id.clone(),
            kind: MenuType::Normal,
            title: String::new(),
            checked: false,
            enabled: true,
            visible: true,
            contexts: vec!["page".into()],
            parent_id: None,
            document_url_patterns: None,
            target_url_patterns: None,
        };
        let item = merge_props(&base, raw)?;
        items.push((k, item));
        self.changed();
        Ok(id)
    }

    pub fn update(&mut self, extension_id: &str, id: &Value, props: &Value) -> ApiResult<()> {
        let k = key_of(id);
        let not_found = |v: &str| ApiError::new(format!("Cannot find menu item with id {v}"));
        let items = self.items.entry(extension_id.to_string()).or_default();
        let item = find(items, &k).cloned().ok_or_else(|| not_found(&k))?;
        if let Some(p) = props.get("parentId").filter(|p| !p.is_null()) {
            let pk = key_of(p);
            if pk == k || find(items, &pk).is_none() || self.descendants(extension_id, &k).contains(&pk) {
                return Err(not_found(&pk));
            }
        }
        let items = self.items.entry(extension_id.to_string()).or_default();
        let merged = merge_props(&item, props)?;
        if let Some(slot) = items.iter_mut().find(|(key, _)| *key == k) {
            slot.1 = merged;
        }
        self.changed();
        Ok(())
    }

    fn descendants(&self, extension_id: &str, id: &str) -> HashSet<String> {
        let mut out = HashSet::new();
        let Some(items) = self.items.get(extension_id) else { return out };
        let mut stack = vec![id.to_string()];
        while let Some(parent) = stack.pop() {
            for (key, i) in items {
                if i.parent_id.as_ref().is_some_and(|p| key_of(p) == parent) && out.insert(key.clone()) {
                    stack.push(key.clone());
                }
            }
        }
        out
    }

    pub fn remove(&mut self, extension_id: &str, id: &Value) -> ApiResult<()> {
        let k = key_of(id);
        if self.items.get(extension_id).and_then(|i| find(i, &k)).is_none() {
            return Err(ApiError::new(format!("Cannot find menu item with id {k}")));
        }
        let mut gone = self.descendants(extension_id, &k);
        gone.insert(k);
        if let Some(items) = self.items.get_mut(extension_id) {
            items.retain(|(key, _)| !gone.contains(key));
        }
        self.changed();
        Ok(())
    }

    pub fn remove_all(&mut self, extension_id: &str) {
        self.items.remove(extension_id);
        self.changed();
    }

    pub fn has(&self, extension_id: &str) -> bool {
        self.items.get(extension_id).is_some_and(|m| !m.is_empty())
    }

    /// The tree to show for an extension: matching items, a child only under a parent that is shown.
    pub fn tree(&self, extension_id: &str, contexts: &HashSet<String>, target: Option<&MenuTarget>) -> Vec<MenuNode> {
        let Some(all) = self.items.get(extension_id) else { return vec![] };
        let selection = target.and_then(|t| t.selection_text.as_deref());
        let shown: Vec<&(String, MenuItem)> = all.iter().filter(|(_, i)| item_matches(i, contexts, target)).collect();
        fn build(shown: &[&(String, MenuItem)], parent: Option<&str>, selection: Option<&str>) -> Vec<MenuNode> {
            shown
                .iter()
                .filter(|(_, i)| match (&i.parent_id, parent) {
                    (None, None) => true,
                    (Some(p), Some(q)) => key_of(p) == q,
                    _ => false,
                })
                .map(|(k, i)| MenuNode { item: i.clone(), label: label_for(&i.title, selection), children: build(shown, Some(k), selection) })
                .collect()
        }
        // Roots: items without a parent. Children whose parent is not shown never appear.
        build(&shown, None, selection)
    }

    /// A click: flips a checkbox, selects a radio (and clears its neighbours), and reports the state before and after.
    pub fn click(&mut self, extension_id: &str, id: &Value) -> ClickInfo {
        let k = key_of(id);
        let items = self.items.entry(extension_id.to_string()).or_default();
        let mut info = ClickInfo { menu_item_id: id.clone(), parent_menu_item_id: None, checked: None, was_checked: None };
        let Some(item) = find(items, &k).cloned() else { return info };
        info.parent_menu_item_id = item.parent_id.clone();
        match item.kind {
            MenuType::Checkbox => {
                info.was_checked = Some(item.checked);
                info.checked = Some(!item.checked);
                if let Some((_, i)) = items.iter_mut().find(|(key, _)| *key == k) {
                    i.checked = !item.checked;
                }
            }
            MenuType::Radio => {
                info.was_checked = Some(item.checked);
                let group = item.parent_id.as_ref().map(key_of);
                for (key, other) in items.iter_mut() {
                    if other.kind == MenuType::Radio && other.parent_id.as_ref().map(key_of) == group {
                        other.checked = *key == k;
                    }
                }
                info.checked = Some(true);
            }
            _ => {}
        }
        self.changed();
        info
    }

    /// Tells the extension an item was clicked (wakes its worker).
    pub fn clicked(&self, extension_id: &str, info: &ClickInfo, tab: Option<Value>) {
        let mut args = vec![info.to_json()];
        args.extend(tab);
        self.emitter.emit(extension_id, MENU_CLICK_EVENT, args, true, true);
    }

    /// Ids with items, live or saved.
    pub fn known_ids(&self) -> Vec<String> {
        let mut v: Vec<String> = self.items.keys().cloned().collect();
        for k in self.saved.keys() {
            if !v.contains(k) {
                v.push(k.clone());
            }
        }
        v.sort();
        v
    }

    /// The extension is uninstalled: its items go, on disk too. An unload (reload, update) keeps them.
    pub fn unload(&mut self, extension_id: &str) {
        self.items.remove(extension_id);
        self.saved.remove(extension_id);
        self.changed();
    }

    /// Router dispatch of the `contextMenus.*` methods.
    pub fn call(&mut self, extension_id: &str, method: &str, args: &[Value]) -> ApiResult<Value> {
        let a = |i: usize| args.get(i).cloned().unwrap_or(Value::Null);
        match method {
            "contextMenus.create" => self.create(extension_id, &a(0)),
            "contextMenus.update" => self.update(extension_id, &a(0), &a(1)).map(|_| Value::Null),
            "contextMenus.remove" => self.remove(extension_id, &a(0)).map(|_| Value::Null),
            "contextMenus.removeAll" => {
                self.remove_all(extension_id);
                Ok(Value::Null)
            }
            _ => Err(ApiError::new(format!("Unknown Extension API - {method}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::Outbox;

    fn item(p: Value) -> MenuItem {
        let base = json!({ "id": "a", "type": "normal", "title": "A", "checked": false, "enabled": true, "visible": true, "contexts": ["page"] });
        let mut v = base;
        for (k, val) in p.as_object().unwrap() {
            v[k] = val.clone();
        }
        serde_json::from_value(v).unwrap()
    }
    fn setup() -> (ContextMenus, Arc<Outbox>) {
        let out = Arc::new(Outbox::new());
        (ContextMenus::new(out.clone()), out)
    }
    fn set(items: &[&str]) -> HashSet<String> {
        items.iter().map(|s| s.to_string()).collect()
    }
    fn page_target() -> MenuTarget {
        MenuTarget { page_url: Some("https://a/".into()), ..Default::default() }
    }

    #[test]
    fn merge_props_changes_fields_and_keeps_the_rest() {
        let next = merge_props(
            &item(json!({ "documentUrlPatterns": ["https://a/*"] })),
            &json!({ "title": "B", "checked": true, "enabled": false, "visible": false, "contexts": "link", "targetUrlPatterns": ["https://t/*"] }),
        )
        .unwrap();
        assert_eq!(next.title, "B");
        assert!(next.checked && !next.enabled && !next.visible);
        assert_eq!(next.contexts, ["link"]);
        assert_eq!(next.document_url_patterns, Some(vec!["https://a/*".to_string()]));
        assert_eq!(next.target_url_patterns, Some(vec!["https://t/*".to_string()]));
    }

    #[test]
    fn merge_props_rejects_empty_title_and_unknown_type() {
        assert!(merge_props(&item(json!({})), &json!({ "title": "" })).unwrap_err().0.contains("title"));
        assert!(merge_props(&item(json!({})), &json!({ "type": "bogus" })).is_err());
        assert_eq!(merge_props(&item(json!({ "type": "separator", "title": "" })), &json!({})).unwrap().kind, MenuType::Separator);
    }

    #[test]
    fn page_applies_only_when_nothing_more_specific_was_clicked() {
        let mut c: Vec<String> = contexts_of(&page_target()).into_iter().collect();
        c.sort();
        assert_eq!(c, ["all", "page"]);
        assert!(!contexts_of(&MenuTarget { page_url: Some("https://a/".into()), link_url: Some("https://b/".into()), ..Default::default() }).contains("page"));
        assert_eq!(
            contexts_of(&MenuTarget { page_url: Some("https://a/".into()), selection_text: Some("x".into()), is_editable: true, ..Default::default() }),
            set(&["all", "selection", "editable"])
        );
        assert!(contexts_of(&MenuTarget { media_type: Some("image".into()), ..Default::default() }).contains("image"));
    }

    #[test]
    fn keeps_items_of_a_matching_context_and_honours_patterns() {
        let link = contexts_of(&MenuTarget { link_url: Some("https://t/x".into()), ..Default::default() });
        assert!(item_matches(&item(json!({ "contexts": ["link"] })), &link, None));
        assert!(!item_matches(&item(json!({ "contexts": ["page"] })), &link, None));
        assert!(item_matches(&item(json!({ "contexts": ["all"] })), &link, None));
        assert!(!item_matches(&item(json!({ "contexts": ["link"], "visible": false })), &link, None));
        let target = MenuTarget { link_url: Some("https://t/x".into()), page_url: Some("https://p/".into()), ..Default::default() };
        assert!(item_matches(&item(json!({ "contexts": ["link"], "targetUrlPatterns": ["https://t/*"] })), &link, Some(&target)));
        assert!(!item_matches(&item(json!({ "contexts": ["link"], "targetUrlPatterns": ["https://other/*"] })), &link, Some(&target)));
        assert!(item_matches(&item(json!({ "contexts": ["link"], "documentUrlPatterns": ["https://p/*"] })), &link, Some(&target)));
        assert!(!item_matches(&item(json!({ "contexts": ["link"], "documentUrlPatterns": ["https://q/*"] })), &link, Some(&target)));
    }

    #[test]
    fn action_contexts_are_only_in_the_button_menu() {
        let button = set(&["all", "action"]);
        assert!(item_matches(&item(json!({ "contexts": ["action"] })), &button, None));
        assert!(!item_matches(&item(json!({ "contexts": ["action"] })), &contexts_of(&page_target()), None));
    }

    #[test]
    fn percent_s_stands_for_the_selection() {
        assert_eq!(label_for("Search %s", Some("cats")), "Search cats");
        assert_eq!(label_for("Search %s", None), "Search %s");
    }

    #[test]
    fn create_generates_ids_refuses_duplicates_and_unknown_parents() {
        let (mut m, _) = setup();
        let id = m.create("e", &json!({ "title": "x" })).unwrap();
        assert!(id.is_string());
        assert!(m.create("e", &json!({ "id": id, "title": "y" })).unwrap_err().0.contains("duplicate"));
        assert!(m.create("e", &json!({ "id": "c", "title": "y", "parentId": "nope" })).unwrap_err().0.contains("Cannot find"));
        assert!(m.create("e", &json!({ "id": "d" })).unwrap_err().0.contains("title"));
        m.create("other", &json!({ "id": id, "title": "same id, other extension" })).unwrap();
    }

    #[test]
    fn update_changes_the_item_shown_and_fails_for_an_unknown_id() {
        let (mut m, _) = setup();
        m.create("e", &json!({ "id": "a", "title": "Old", "contexts": ["link"] })).unwrap();
        let link = contexts_of(&MenuTarget { link_url: Some("https://x/".into()), ..Default::default() });
        assert_eq!(m.tree("e", &link, None)[0].label, "Old");
        m.update("e", &json!("a"), &json!({ "title": "New", "contexts": ["selection"] })).unwrap();
        assert!(m.tree("e", &link, None).is_empty());
        let sel = contexts_of(&MenuTarget { selection_text: Some("s".into()), ..Default::default() });
        assert_eq!(m.tree("e", &sel, None)[0].label, "New");
        assert!(m.update("e", &json!("zzz"), &json!({ "title": "x" })).unwrap_err().0.contains("Cannot find"));
        assert!(m.update("e", &json!("a"), &json!({ "parentId": "a" })).is_err());
    }

    #[test]
    fn tree_nesting_removal_and_hidden_parents() {
        let (mut m, _) = setup();
        m.create("e", &json!({ "id": "p", "title": "P" })).unwrap();
        m.create("e", &json!({ "id": "c", "title": "C", "parentId": "p" })).unwrap();
        let page = contexts_of(&page_target());
        let tree = m.tree("e", &page, None);
        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].children.iter().map(|n| key_of(&n.item.id)).collect::<Vec<_>>(), ["c"]);
        m.update("e", &json!("p"), &json!({ "visible": false })).unwrap();
        assert!(m.tree("e", &page, None).is_empty());
        m.remove("e", &json!("p")).unwrap();
        assert!(!m.has("e"));
    }

    #[test]
    fn click_flips_checkboxes_selects_radios_and_wakes_the_worker() {
        let (mut m, out) = setup();
        m.create("e", &json!({ "id": "cb", "title": "C", "type": "checkbox" })).unwrap();
        let c1 = m.click("e", &json!("cb"));
        assert_eq!((c1.checked, c1.was_checked), (Some(true), Some(false)));
        let c2 = m.click("e", &json!("cb"));
        assert_eq!((c2.checked, c2.was_checked), (Some(false), Some(true)));
        m.create("e", &json!({ "id": "r1", "title": "R1", "type": "radio", "checked": true })).unwrap();
        m.create("e", &json!({ "id": "r2", "title": "R2", "type": "radio" })).unwrap();
        m.click("e", &json!("r2"));
        let radios: Vec<_> = m.tree("e", &set(&["page"]), None).iter().filter(|n| key_of(&n.item.id).starts_with('r')).map(|n| (key_of(&n.item.id), n.item.checked)).collect();
        assert_eq!(radios, [("r1".to_string(), false), ("r2".to_string(), true)]);
        m.clicked("e", &ClickInfo { menu_item_id: json!("cb"), parent_menu_item_id: None, checked: None, was_checked: None }, Some(json!({ "id": 3 })));
        let ev = out.drain();
        assert_eq!((ev[0].name.as_str(), ev[0].wake), ("contextMenus.onClicked", true));
        assert_eq!(ev[0].args, vec![json!({ "menuItemId": "cb" }), json!({ "id": 3 })]);
    }

    fn file() -> (tempfile::TempDir, std::path::PathBuf) {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("extension-context-menus.json");
        (d, f)
    }
    const OLD: &str = r#"{"e":[{"id":"x","type":"normal","title":"Old","checked":false,"enabled":true,"visible":true,"contexts":["page"]}]}"#;

    #[test]
    fn saves_items_restores_them_when_the_extension_loads_and_flushes() {
        let (_d, path) = file();
        let (mut a, _) = setup();
        a.persist(&path);
        a.create("e", &json!({ "id": "x", "title": "X" })).unwrap();
        a.create("f", &json!({ "id": "y", "title": "Y" })).unwrap();
        a.update("e", &json!("x"), &json!({ "title": "X2" })).unwrap();
        a.remove("f", &json!("y")).unwrap();
        a.flush();
        let saved: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(saved.as_object().unwrap().keys().collect::<Vec<_>>(), ["e"]);
        let (mut b, _) = setup();
        b.persist(&path);
        assert!(b.tree("e", &set(&["page"]), None).is_empty());
        b.restore("e");
        assert_eq!(b.tree("e", &set(&["page"]), None).iter().map(|n| n.label.clone()).collect::<Vec<_>>(), ["X2"]);
    }

    #[test]
    fn a_create_with_a_restored_id_replaces_it_but_a_real_duplicate_fails() {
        let (_d, path) = file();
        std::fs::write(&path, OLD).unwrap();
        let (mut m, _) = setup();
        m.persist(&path);
        m.restore("e");
        assert_eq!(m.create("e", &json!({ "id": "x", "title": "New" })).unwrap(), json!("x"));
        assert_eq!(m.tree("e", &set(&["page"]), None)[0].label, "New");
        assert!(m.create("e", &json!({ "id": "x", "title": "Again" })).unwrap_err().0.contains("duplicate"));
    }

    #[test]
    fn an_item_created_this_run_is_not_overwritten_by_the_restore() {
        let (_d, path) = file();
        std::fs::write(&path, OLD).unwrap();
        let (mut m, _) = setup();
        m.persist(&path);
        m.create("e", &json!({ "id": "x", "title": "Fresh" })).unwrap();
        m.restore("e");
        assert_eq!(m.tree("e", &set(&["page"]), None)[0].label, "Fresh");
    }

    #[test]
    fn uninstalling_drops_the_items_from_memory_and_disk() {
        let (_d, path) = file();
        let (mut m, _) = setup();
        m.persist(&path);
        m.create("e", &json!({ "id": "x", "title": "X" })).unwrap();
        m.flush();
        m.unload("e");
        m.flush();
        let saved: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(saved, json!({}));
        assert!(!m.has("e"));
        let (mut again, _) = setup();
        again.persist(&path);
        again.restore("e");
        assert!(!again.has("e"));
    }

    #[test]
    fn call_dispatches_the_api() {
        let (mut m, _) = setup();
        let id = m.call("e", "contextMenus.create", &[json!({ "id": 7, "title": "N" })]).unwrap();
        assert_eq!(id, json!(7));
        m.call("e", "contextMenus.update", &[json!(7), json!({ "title": "M" })]).unwrap();
        m.call("e", "contextMenus.remove", &[json!(7)]).unwrap();
        m.call("e", "contextMenus.removeAll", &[]).unwrap();
        assert!(m.call("e", "contextMenus.nope", &[]).is_err());
    }
}

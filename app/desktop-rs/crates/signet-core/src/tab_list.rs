//! Ports `packages/shared/src/tab-list.ts`: flat tab list with an active tab (pure reducer).

use crate::factory::new_tab;
use crate::ids::now_ms;
use crate::types::TabInfo;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct TabList {
    pub tabs: Vec<TabInfo>,
    pub active_id: Option<String>,
}

pub enum TabListAction {
    Open { url: String, title: Option<String>, tab: Option<Box<TabInfo>> },
    Close { id: String },
    Activate { id: String },
    /// `patch`: JSON object merged into the tab (the `id` field is ignored).
    Update { id: String, patch: Value },
    CloseAll,
    Restore { list: TabList },
}

pub fn empty_tab_list() -> TabList {
    TabList::default()
}

/// `TabInfo` fields kept from one launch to the next; the rest starts from scratch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredTab {
    pub id: String,
    pub url: String,
    pub title: String,
    pub last_active: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredTabList {
    pub tabs: Vec<StoredTab>,
    pub active_id: Option<String>,
}

pub fn serialize_tab_list(list: &TabList) -> StoredTabList {
    StoredTabList {
        tabs: list.tabs.iter().map(|t| StoredTab { id: t.id.clone(), url: t.url.clone(), title: t.title.clone(), last_active: t.last_active }).collect(),
        active_id: list.active_id.clone(),
    }
}

/// Reads back a saved list ignoring anything malformed; an unknown `active_id` falls back to the last tab.
pub fn parse_tab_list(raw: &Value) -> TabList {
    let Some(items) = raw.get("tabs").and_then(Value::as_array) else { return empty_tab_list() };
    let mut seen = HashSet::new();
    let mut tabs = Vec::new();
    for item in items {
        let Some(id) = item.get("id").and_then(Value::as_str).filter(|s| !s.is_empty()) else { continue };
        let Some(url) = item.get("url").and_then(Value::as_str) else { continue };
        if !seen.insert(id.to_string()) {
            continue;
        }
        let mut tab = new_tab(url, item.get("title").and_then(Value::as_str).unwrap_or(""));
        tab.id = id.to_string();
        if let Some(n) = item.get("lastActive").and_then(Value::as_f64) {
            tab.last_active = n as i64;
        }
        tabs.push(tab);
    }
    let active_id = raw
        .get("activeId")
        .and_then(Value::as_str)
        .filter(|a| seen.contains(*a))
        .map(String::from)
        .or_else(|| tabs.last().map(|t| t.id.clone()));
    TabList { tabs, active_id }
}

pub fn tab_list_reducer(state: &TabList, action: TabListAction) -> TabList {
    match action {
        TabListAction::Open { url, title, tab } => {
            let tab = tab.map(|t| *t).unwrap_or_else(|| new_tab(&url, title.as_deref().unwrap_or("")));
            let mut tabs = state.tabs.clone();
            let id = tab.id.clone();
            tabs.push(tab);
            TabList { tabs, active_id: Some(id) }
        }
        TabListAction::Close { id } => {
            let Some(index) = state.tabs.iter().position(|t| t.id == id) else { return state.clone() };
            let tabs: Vec<TabInfo> = state.tabs.iter().filter(|t| t.id != id).cloned().collect();
            if state.active_id.as_deref() != Some(id.as_str()) {
                return TabList { tabs, active_id: state.active_id.clone() };
            }
            // The active tab closes: move to the right neighbor, otherwise the left one.
            let next = tabs.get(index).or_else(|| index.checked_sub(1).and_then(|i| tabs.get(i))).map(|t| t.id.clone());
            TabList { tabs, active_id: next }
        }
        TabListAction::Activate { id } => {
            if state.active_id.as_deref() == Some(id.as_str()) || !state.tabs.iter().any(|t| t.id == id) {
                return state.clone();
            }
            let tabs = state.tabs.iter().cloned().map(|mut t| {
                if t.id == id {
                    t.last_active = now_ms();
                }
                t
            });
            TabList { tabs: tabs.collect(), active_id: Some(id) }
        }
        TabListAction::Update { id, patch } => {
            if !state.tabs.iter().any(|t| t.id == id) {
                return state.clone();
            }
            let tabs = state
                .tabs
                .iter()
                .map(|t| {
                    if t.id != id {
                        return t.clone();
                    }
                    let mut v = serde_json::to_value(t).unwrap_or(Value::Null);
                    if let (Some(o), Some(p)) = (v.as_object_mut(), patch.as_object()) {
                        for (k, x) in p {
                            if k != "id" {
                                o.insert(k.clone(), x.clone());
                            }
                        }
                    }
                    serde_json::from_value(v).unwrap_or_else(|_| t.clone())
                })
                .collect();
            TabList { tabs, active_id: state.active_id.clone() }
        }
        TabListAction::CloseAll => empty_tab_list(),
        TabListAction::Restore { list } => list,
    }
}

pub fn active_tab(list: &TabList) -> Option<&TabInfo> {
    list.tabs.iter().find(|t| Some(&t.id) == list.active_id.as_ref())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn open(s: &TabList, url: &str) -> TabList {
        tab_list_reducer(s, TabListAction::Open { url: url.into(), title: None, tab: None })
    }

    #[test]
    fn open_activates() {
        let s = open(&open(&empty_tab_list(), "https://a.com"), "https://b.com");
        assert_eq!(s.tabs.iter().map(|t| t.url.as_str()).collect::<Vec<_>>(), ["https://a.com", "https://b.com"]);
        assert_eq!(s.active_id.as_deref(), Some(s.tabs[1].id.as_str()));
        assert_eq!(active_tab(&s).unwrap().url, "https://b.com");
    }

    #[test]
    fn close_neighbors() {
        let mut s = open(&open(&open(&empty_tab_list(), "a"), "b"), "c");
        let ids: Vec<String> = s.tabs.iter().map(|t| t.id.clone()).collect();
        s = tab_list_reducer(&s, TabListAction::Activate { id: ids[1].clone() });
        s = tab_list_reducer(&s, TabListAction::Close { id: ids[1].clone() });
        assert_eq!(s.active_id.as_deref(), Some(ids[2].as_str()));
        s = tab_list_reducer(&s, TabListAction::Close { id: ids[2].clone() });
        assert_eq!(s.active_id.as_deref(), Some(ids[0].as_str()));
        s = tab_list_reducer(&s, TabListAction::Close { id: ids[0].clone() });
        assert_eq!(s, empty_tab_list());
        assert!(active_tab(&s).is_none());
    }

    #[test]
    fn inactive_close_and_unknown() {
        let s = open(&open(&empty_tab_list(), "a"), "b");
        let (a, b) = (s.tabs[0].id.clone(), s.tabs[1].id.clone());
        assert_eq!(tab_list_reducer(&s, TabListAction::Close { id: a }).active_id.as_deref(), Some(b.as_str()));
        assert_eq!(tab_list_reducer(&s, TabListAction::Close { id: "nope".into() }), s);
        assert_eq!(tab_list_reducer(&s, TabListAction::Activate { id: "nope".into() }), s);
    }

    #[test]
    fn update_merges_without_id() {
        let s = open(&empty_tab_list(), "a");
        let id = s.tabs[0].id.clone();
        let u = tab_list_reducer(&s, TabListAction::Update { id: id.clone(), patch: json!({"url":"https://x.com","title":"X","id":"other"}) });
        assert_eq!((u.tabs[0].id.as_str(), u.tabs[0].url.as_str(), u.tabs[0].title.as_str()), (id.as_str(), "https://x.com", "X"));
        assert_eq!(tab_list_reducer(&s, TabListAction::Update { id: "nope".into(), patch: json!({"title":"X"}) }), s);
    }

    #[test]
    fn close_all_and_restore() {
        let s = open(&empty_tab_list(), "a");
        assert_eq!(tab_list_reducer(&s, TabListAction::CloseAll), empty_tab_list());
        assert_eq!(tab_list_reducer(&empty_tab_list(), TabListAction::Restore { list: s.clone() }), s);
    }

    #[test]
    fn round_trip_and_malformed() {
        let s = open(&open(&empty_tab_list(), "https://a.com"), "https://b.com");
        let json = serde_json::to_value(serialize_tab_list(&s)).unwrap();
        let back = parse_tab_list(&json);
        assert_eq!(back.active_id, s.active_id);
        assert_eq!(back.tabs.iter().map(|t| (&t.id, &t.url)).collect::<Vec<_>>(), s.tabs.iter().map(|t| (&t.id, &t.url)).collect::<Vec<_>>());
        assert_eq!(parse_tab_list(&Value::Null), empty_tab_list());
        assert_eq!(parse_tab_list(&json!({"tabs":"x"})), empty_tab_list());
        let p = parse_tab_list(&json!({"tabs":[null,{"id":1},{"id":"a","url":"u"},{"id":"a","url":"dup"},{"id":"b","url":"v","title":3}],"activeId":"zz"}));
        assert_eq!(p.tabs.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["a", "b"]);
        assert_eq!(p.tabs[1].title, "");
        assert_eq!(p.active_id.as_deref(), Some("b"));
    }
}

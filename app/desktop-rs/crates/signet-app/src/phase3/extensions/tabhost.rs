//! `TabHost` over a snapshot of the core state. The snapshot is taken on the main thread right before each call, so the
//! trait object (which must be `Send + Sync`) holds plain data only. The few operations that change the browser are
//! queued in `actions` and run by the caller after the call.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{json, Value};
use signet_extensions::tabs::{TabHost, TabInfo};
use signet_web::{FrameTarget, ViewId, WebView};

use super::bridge::tab_number;
use crate::browser::Core;

/// What a call asked the browser to do through the `TabHost`.
#[derive(Debug, Clone, PartialEq)]
pub enum HostAction {
    Activate(String),
}

#[derive(Default)]
pub struct SnapHost {
    pub tabs: Vec<TabInfo>,
    pub active: Option<i64>,
    /// Extension id to folder, for `read_extension_file`.
    pub dirs: HashMap<String, PathBuf>,
    /// Tab number to the view that shows it (loaded tabs only).
    pub views: HashMap<i64, ViewId>,
    /// Tab number to the Signet tab id.
    pub core_ids: HashMap<i64, String>,
    pub actions: Mutex<Vec<HostAction>>,
    css: AtomicU64,
}

impl SnapHost {
    pub fn take_actions(&self) -> Vec<HostAction> {
        self.actions.lock().map(|mut a| std::mem::take(&mut *a)).unwrap_or_default()
    }
}

/// Extension view of every tab of the active space (favorites and pinned ones included), in the bar's order.
pub fn snapshot(core: &Core, dirs: HashMap<String, PathBuf>) -> SnapHost {
    let state = core.state();
    let active = core.active_tab_id();
    let mut host = SnapHost { dirs, ..Default::default() };
    for (index, id) in signet_core::model::tab_ids_in_order(state).iter().enumerate() {
        let Some(tab) = state.tabs.get(id) else { continue };
        let number = tab_number(id);
        let pinned = signet_core::model::locate::locate(state, id).is_some_and(|l| l.zone != signet_core::types::Zone::Today);
        let is_active = active.as_deref() == Some(id.as_str());
        if let Some(v) = core.views.view_of(id) {
            host.views.insert(number, v.id());
        }
        host.core_ids.insert(number, id.clone());
        if is_active {
            host.active = Some(number);
        }
        let bounds = core.views.view_of(id).map(|v| v.bounds());
        host.tabs.push(TabInfo {
            id: number,
            window_id: 1,
            index: index as i64,
            active: is_active,
            pinned,
            highlighted: is_active,
            incognito: false,
            url: tab.url.clone(),
            title: tab.custom_title.clone().unwrap_or_else(|| tab.title.clone()),
            fav_icon_url: tab.favicon.clone(),
            loading: tab.loading,
            audible: tab.audible,
            muted: tab.muted,
            discarded: !tab.loaded,
            width: bounds.map(|b| b.width as i64).unwrap_or(0),
            height: bounds.map(|b| b.height as i64).unwrap_or(0),
            group_id: -1,
            opener_tab_id: None,
        });
    }
    host
}

fn window_json() -> Value {
    json!({ "id": 1, "focused": true, "incognito": false, "type": "normal", "state": "normal", "alwaysOnTop": false })
}

impl TabHost for SnapHost {
    fn tab(&self, tab_id: i64) -> Option<TabInfo> {
        self.tabs.iter().find(|t| t.id == tab_id).cloned()
    }

    fn active_tab(&self, _window_id: Option<i64>) -> Option<TabInfo> {
        self.active.and_then(|id| self.tab(id))
    }

    fn capture(&self, _tab_id: i64, _format: &str, _quality: i64) -> Result<String, String> {
        // A screenshot is an asynchronous DevTools call; `ExtensionHost::call` runs on the main thread here and cannot wait for it.
        Err("tabs.captureVisibleTab is not supported in Signet yet.".into())
    }

    fn insert_css(&self, tab_id: i64, css: &str, _origin: &str) -> Result<String, String> {
        let view = self.views.get(&tab_id).and_then(|v| WebView::from_id(*v)).ok_or_else(|| format!("No tab with id: {tab_id}."))?;
        let key = format!("signet-ext-css-{}", self.css.fetch_add(1, Ordering::Relaxed));
        view.insert_css(&FrameTarget::Main, &key, css);
        Ok(format!("{tab_id}:{key}"))
    }

    fn remove_css(&self, tab_id: i64, key: &str) {
        let key = key.split_once(':').map(|(_, k)| k).unwrap_or(key);
        if let Some(view) = self.views.get(&tab_id).and_then(|v| WebView::from_id(*v)) {
            view.remove_css(&FrameTarget::Main, key);
        }
    }

    fn page_language(&self, _tab_id: i64) -> Result<String, String> {
        Err("The page language is not available synchronously.".into())
    }

    fn activate_at(&self, index: i64, _window_id: Option<i64>) -> Option<Value> {
        let tab = self.tabs.iter().find(|t| t.index == index)?;
        let id = self.core_ids.get(&tab.id)?.clone();
        if let Ok(mut a) = self.actions.lock() {
            a.push(HostAction::Activate(id));
        }
        Some(window_json())
    }

    fn all_tab_ids(&self) -> Vec<i64> {
        self.tabs.iter().map(|t| t.id).collect()
    }

    fn read_extension_file(&self, extension_id: &str, file: &str) -> Result<String, String> {
        let dir = self.dirs.get(extension_id).ok_or("Unknown extension.")?;
        let path = signet_extensions::urlmatch::extension_file_path(dir, extension_id, file).ok_or("File outside the extension.")?;
        std::fs::read_to_string(path).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host() -> SnapHost {
        let mut h = SnapHost::default();
        for (i, id) in ["a", "b"].iter().enumerate() {
            let number = tab_number(id);
            h.tabs.push(TabInfo { id: number, index: i as i64, active: i == 1, ..Default::default() });
            h.core_ids.insert(number, id.to_string());
        }
        h.active = Some(tab_number("b"));
        h
    }

    #[test]
    fn tabs_are_found_by_number_and_the_active_one_is_marked() {
        let h = host();
        assert_eq!(h.tab(tab_number("a")).map(|t| t.index), Some(0));
        assert_eq!(h.active_tab(None).map(|t| t.id), Some(tab_number("b")));
        assert_eq!(h.all_tab_ids().len(), 2);
    }

    #[test]
    fn activating_by_index_queues_an_action() {
        let h = host();
        assert!(h.activate_at(0, None).is_some());
        assert_eq!(h.take_actions(), vec![HostAction::Activate("a".into())]);
        assert!(h.activate_at(9, None).is_none());
    }
}

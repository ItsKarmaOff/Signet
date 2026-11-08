//! Port of `browser/views.ts`: one CEF `WebView` per loaded tab, created on demand in the right profile, destroyed
//! on sleep or on closing.

use std::collections::HashMap;

use raw_window_handle::RawWindowHandle;
use signet_web::{Bounds, ProfileKind, ViewId, ViewOptions, WebView};

#[derive(Default)]
pub struct Views {
    map: HashMap<String, WebView>,
    rev: HashMap<ViewId, String>,
    /// Native window the views are children of (the main window).
    pub parent: Option<RawWindowHandle>,
    /// Views that were closed by the app: their `Closed` event is expected.
    pub dev_views: HashMap<ViewId, String>,
}

impl Views {
    pub fn view_of(&self, tab_id: &str) -> Option<WebView> {
        self.map.get(tab_id).copied()
    }

    pub fn tab_of(&self, view: ViewId) -> Option<&str> {
        self.rev.get(&view).map(String::as_str)
    }

    pub fn ids(&self) -> Vec<String> {
        self.map.keys().cloned().collect()
    }

    pub fn create(&mut self, tab_id: &str, profile: ProfileKind, url: &str, bounds: Bounds, visible: bool) -> Option<WebView> {
        let parent = self.parent?;
        let opts = ViewOptions { profile, url: url.to_string(), bounds, visible, transparent_background: false };
        match WebView::create(parent, opts) {
            Ok(v) => {
                self.map.insert(tab_id.to_string(), v);
                self.rev.insert(v.id(), tab_id.to_string());
                Some(v)
            }
            Err(e) => {
                eprintln!("[views] cannot create the view of {tab_id}: {e}");
                None
            }
        }
    }

    /// Takes the tab's view out of the table without closing it (it moves to another place).
    pub fn take(&mut self, tab_id: &str) -> Option<WebView> {
        let v = self.map.remove(tab_id)?;
        self.rev.remove(&v.id());
        Some(v)
    }

    /// Puts a view taken with [`Views::take`] back.
    pub fn put(&mut self, tab_id: &str, v: WebView) {
        self.map.insert(tab_id.to_string(), v);
        self.rev.insert(v.id(), tab_id.to_string());
    }

    /// Closes the tab's view (the tab stays in the state, unloaded).
    pub fn destroy(&mut self, tab_id: &str) {
        if let Some(v) = self.map.remove(tab_id) {
            self.rev.remove(&v.id());
            v.close();
        }
    }

    pub fn destroy_all(&mut self) {
        for id in self.ids() {
            self.destroy(&id);
        }
    }
}

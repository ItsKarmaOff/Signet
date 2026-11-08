//! Navigation history of tabs (port of `queueHistory` / `saveHistory` / `writeHistory` of `browser/views.ts`).
//!
//! The back/forward list of every live view is copied into `store.data.histories` (written to `tab-history.json`
//! by the store), so that a tab put to sleep, closed (undo) or restarted keeps its entries. Copies are batched:
//! navigation events mark the tab, the next `tick` copies it; sleeping and closing a tab copy it right away
//! through `ModelEnv::save_history` (see `capture` and `commit_histories`, wired by `browser/mod.rs`).
//!
//! The bookmarks HTML import and export had no counterpart in the Electron desktop app (only the mobile app has
//! them), so there is no menu command to port here.

use std::collections::HashSet;

use indexmap::IndexMap;
use signet_core::core::store::{trim_history, NavEntry, TabHistory, HISTORY_TAB_STATE_MAX};
use signet_web::{HistoryEntry, WebEvent, WebView};

use crate::browser::views::Views;
use crate::browser::Core;

/// State owned by this feature (a field of `Phase3`).
#[derive(Default)]
pub struct State {
    /// Tabs whose history changed since the last copy.
    dirty: HashSet<String>,
}

// ------------------------------------------------------------------ pure parts

/// Bounded history from the list of a view (`current` marks the active entry); `None` if there is no usable entry.
fn history_of(entries: &[HistoryEntry]) -> Option<TabHistory> {
    let list: Vec<NavEntry> = entries.iter().map(|e| NavEntry { url: e.url.clone(), title: e.title.clone(), page_state: None }).collect();
    let index = entries.iter().position(|e| e.current)? as i64;
    trim_history(&list, index, HISTORY_TAB_STATE_MAX)
}

/// Stores `new` for `tab_id` unless it is the same as the stored one. True if the store changed.
fn merge(histories: &mut IndexMap<String, TabHistory>, tab_id: &str, new: TabHistory) -> bool {
    if histories.get(tab_id) == Some(&new) {
        return false;
    }
    histories.insert(tab_id.to_string(), new);
    true
}

/// Copy of a view's history (call while the view is alive, on the main thread).
pub(crate) fn capture_view(view: WebView) -> Option<TabHistory> {
    history_of(&view.navigation_entries())
}

/// Copy of a tab's history, for `ModelEnv::save_history`.
pub(crate) fn capture(views: &Views, tab_id: &str) -> Option<TabHistory> {
    capture_view(views.view_of(tab_id)?)
}

impl Core {
    /// Stores captured histories (detached tabs and unknown tabs are skipped). Returns true if something changed.
    pub fn commit_histories(&mut self, list: Vec<(String, TabHistory)>) -> bool {
        let mut changed = false;
        for (id, h) in list {
            if self.store.data.detached.contains(&id) || !self.store.data.state.tabs.contains_key(&id) {
                continue;
            }
            changed |= merge(&mut self.store.data.histories, &id, h);
        }
        if changed {
            self.store.schedule_history();
        }
        changed
    }

    /// Copies the history of a tab's view into the store right away.
    pub fn save_tab_history(&mut self, tab_id: &str) {
        self.phase3.data.dirty.remove(tab_id);
        if let Some(h) = capture(&self.views, tab_id) {
            self.commit_histories(vec![(tab_id.to_string(), h)]);
        }
    }

    /// Copies the history of every live view (`saveAllHistories`).
    pub fn save_all_histories(&mut self) {
        self.phase3.data.dirty.clear();
        let list: Vec<(String, TabHistory)> = self.views.ids().into_iter().filter_map(|id| capture(&self.views, &id).map(|h| (id, h))).collect();
        self.commit_histories(list);
    }
}

/// Called once after the browser core exists and the web layer is initialized.
pub(super) fn startup(_core: &mut Core) {}

/// Called for every CEF event, before the core handles it: a navigation of a tab asks for a copy.
pub(super) fn web_event(core: &mut Core, ev: &WebEvent) {
    let (view, wanted) = match ev {
        WebEvent::AddressChanged { view, is_main_frame, .. } => (*view, *is_main_frame),
        WebEvent::LoadEnd { view, is_main_frame, .. } => (*view, *is_main_frame),
        WebEvent::LoadingState { view, is_loading, .. } => (*view, !*is_loading),
        _ => return,
    };
    if !wanted {
        return;
    }
    if let Some(tab) = core.views.tab_of(view).map(str::to_string) {
        core.phase3.data.dirty.insert(tab);
    }
}

/// Called on every core tick: copies the marked tabs.
pub(super) fn tick(core: &mut Core) {
    if core.phase3.data.dirty.is_empty() {
        return;
    }
    let ids: Vec<String> = core.phase3.data.dirty.drain().collect();
    let list: Vec<(String, TabHistory)> = ids.into_iter().filter_map(|id| capture(&core.views, &id).map(|h| (id, h))).collect();
    core.commit_histories(list);
}

/// Called before quitting: the last copy goes to disk with the store flush.
pub(super) fn quit(core: &mut Core) {
    core.save_all_histories();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(i: usize, url: &str, current: bool) -> HistoryEntry {
        HistoryEntry { index: i, url: url.into(), display_url: url.into(), title: format!("t{i}"), current, http_status: 200 }
    }

    #[test]
    fn active_entry_gives_the_index() {
        let h = history_of(&[entry(0, "https://a.com", false), entry(1, "https://b.com", true), entry(2, "https://c.com", false)]).unwrap();
        assert_eq!(h.index, 1);
        assert_eq!(h.entries.len(), 3);
        assert_eq!(h.entries[2].url, "https://c.com");
        assert!(h.entries.iter().all(|e| e.page_state.is_none()));
    }

    #[test]
    fn no_entry_or_no_active_one_gives_nothing() {
        assert!(history_of(&[]).is_none());
        assert!(history_of(&[entry(0, "https://a.com", false)]).is_none());
    }

    #[test]
    fn long_lists_are_bounded() {
        let list: Vec<HistoryEntry> = (0..200).map(|i| entry(i, &format!("https://a.com/{i}"), i == 100)).collect();
        let h = history_of(&list).unwrap();
        assert!(h.entries.len() <= 100);
        assert_eq!(h.entries[h.index].url, "https://a.com/100");
    }

    #[test]
    fn identical_history_does_not_dirty_the_store() {
        let mut m = IndexMap::new();
        let h = history_of(&[entry(0, "https://a.com", true)]).unwrap();
        assert!(merge(&mut m, "t", h.clone()));
        assert!(!merge(&mut m, "t", h.clone()));
        let h2 = history_of(&[entry(0, "https://a.com", false), entry(1, "https://b.com", true)]).unwrap();
        assert!(merge(&mut m, "t", h2));
    }
}

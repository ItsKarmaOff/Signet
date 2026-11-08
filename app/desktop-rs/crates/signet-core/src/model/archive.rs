//! Ports `app/desktop/src/main/model/archive.ts`: archiving today tabs, on demand (the whole space) or by expiry,
//! with undo for the batch.

use super::locate::active_space_idx;
use super::types::ARCHIVE_MAX;
use super::Model;
use crate::core::store::{ArchivedTab, TabHistory};
use crate::tree::{group_of_item, item_tab_ids};
use crate::types::{Item, TabGroup, TabInfo};
use indexmap::IndexMap;
use std::collections::HashSet;

/// What is needed to undo the last grouped archiving of today tabs.
#[derive(Debug, Clone, PartialEq)]
pub struct ArchiveUndo {
    pub space_id: String,
    /// List of today tabs before archiving (copy).
    pub items: Vec<Item>,
    /// Group headers before archiving (copy).
    pub groups: IndexMap<String, TabGroup>,
    pub tabs: Vec<TabInfo>,
    pub histories: Vec<(String, TabHistory)>,
    /// Entries created in the archive.
    pub entries: Vec<ArchivedTab>,
    pub active: Option<String>,
    /// Active tab just after archiving.
    pub active_after: Option<String>,
}

/// Result of `clear_today`: the number of archived tabs and the means to undo (`restore_archive`).
#[derive(Debug, Clone, PartialEq)]
pub struct ClearedToday {
    pub count: usize,
    pub undo: ArchiveUndo,
}

impl Model<'_> {
    /// Puts the tab in the archive (500 entries at most) then deletes it.
    pub fn archive_tab(&mut self, tab_id: &str) -> Option<ArchivedTab> {
        let tab = self.data.state.tabs.get(tab_id)?.clone();
        let entry = ArchivedTab { id: Some(self.env.new_id()), url: tab.url, title: tab.title, favicon: tab.favicon, archived_at: self.env.now() };
        self.data.state.archive.insert(0, entry.clone());
        self.data.state.archive.truncate(ARCHIVE_MAX);
        self.remove(tab_id);
        Some(entry)
    }

    /// Archives all today tabs of the active space. Returns the number of tabs and the undo data (`None`: nothing to archive).
    pub fn clear_today(&mut self) -> Option<ClearedToday> {
        let space_idx = active_space_idx(&self.data.state);
        let ids: Vec<String> = self.data.state.spaces[space_idx].today.iter().flat_map(item_tab_ids).collect();
        if ids.is_empty() {
            return None;
        }
        for id in &ids {
            self.env.save_history(id);
        }
        let space = &self.data.state.spaces[space_idx];
        let mut undo = ArchiveUndo {
            space_id: space.id.clone(),
            items: space.today.clone(),
            groups: space.groups.clone().unwrap_or_default(),
            tabs: ids.iter().filter_map(|id| self.data.state.tabs.get(id).cloned()).collect(),
            histories: ids.iter().filter_map(|id| self.data.histories.get(id).map(|h| (id.clone(), h.clone()))).collect(),
            entries: Vec::new(),
            active: space.active_tab_id.clone(),
            active_after: None,
        };
        for id in &ids {
            if let Some(e) = self.archive_tab(id) {
                undo.entries.push(e);
            }
        }
        undo.active_after = self.data.state.spaces[space_idx].active_tab_id.clone();
        Some(ClearedToday { count: ids.len(), undo })
    }

    /// Puts the tabs back in their places and removes them from the archive; false if the space is gone.
    pub fn restore_archive(&mut self, undo: &ArchiveUndo) -> bool {
        let Some(space_idx) = self.data.state.spaces.iter().position(|s| s.id == undo.space_id) else { return false };
        for tab in &undo.tabs {
            if self.data.state.tabs.contains_key(&tab.id) {
                continue;
            }
            let mut t = tab.clone();
            t.loaded = false;
            t.loading = false;
            t.audible = false;
            self.data.state.tabs.insert(t.id.clone(), t);
        }
        for (id, h) in &undo.histories {
            self.data.histories.insert(id.clone(), h.clone());
        }
        for entry in &undo.entries {
            if let Some(at) = self.data.state.archive.iter().position(|e| e == entry) {
                self.data.state.archive.remove(at);
            }
        }
        // Tabs opened since stay at the top; the old ones resume their order below them, groups and headers included.
        let space = &mut self.data.state.spaces[space_idx];
        space.today.extend(undo.items.iter().cloned());
        let mut groups = undo.groups.clone();
        if let Some(current) = space.groups.take() {
            for (k, v) in current {
                groups.insert(k, v);
            }
        }
        space.groups = Some(groups);
        // The previously active tab comes back, unless the user chose another in the meantime.
        if let Some(active) = &undo.active
            && self.data.state.tabs.contains_key(active)
            && self.data.state.spaces[space_idx].active_tab_id == undo.active_after
        {
            self.set_active(space_idx, Some(active));
        }
        true
    }

    /// Archives today tabs left too long without activation (except active tabs). `busy`: tabs that sound, the mini
    /// player, media or developer tools keep open. Returns the number of archived tabs.
    pub fn sweep(&mut self, busy: &dyn Fn(&str) -> bool) -> usize {
        let hours = self.data.state.settings.archive_after_hours;
        if hours <= 0.0 {
            return 0; // 0 = never
        }
        let limit = self.env.now() as f64 - hours * 3_600_000.0;
        let actives: HashSet<Option<String>> = self.data.state.spaces.iter().map(|s| s.active_tab_id.clone()).collect();
        let mut count = 0;
        for i in 0..self.data.state.spaces.len() {
            count += self.sweep_space(i, &actives, limit, busy);
        }
        count
    }

    fn sweep_space(&mut self, space_idx: usize, actives: &HashSet<Option<String>>, limit: f64, busy: &dyn Fn(&str) -> bool) -> usize {
        let stale = |st: &crate::core::store::State, id: &str| (st.tabs.get(id).map_or(f64::INFINITY, |t| t.last_active as f64)) < limit;
        let is_active = |id: &str| actives.contains(&Some(id.to_string()));
        let mut seen: HashSet<String> = HashSet::new();
        let mut count = 0;
        let snapshot: Vec<Item> = self.data.state.spaces[space_idx].today.clone();
        for item in &snapshot {
            if let Some(group) = group_of_item(item) {
                // A group is archived as a block, when none of its tabs has been used within the delay.
                if !seen.insert(group.to_string()) {
                    continue;
                }
                let ids: Vec<String> = self.data.state.spaces[space_idx].today.iter().filter(|i| i.group() == Some(group)).flat_map(item_tab_ids).collect();
                if ids.iter().any(|id| is_active(id) || busy(id)) || !ids.iter().all(|id| stale(&self.data.state, id)) {
                    continue;
                }
                for id in &ids {
                    if self.archive_tab(id).is_some() {
                        count += 1;
                    }
                }
                continue;
            }
            let ids = item_tab_ids(item);
            if ids.iter().any(|id| is_active(id)) {
                continue;
            }
            for id in &ids {
                if stale(&self.data.state, id) && !busy(id) && self.archive_tab(id).is_some() {
                    count += 1;
                }
            }
        }
        count
    }
}

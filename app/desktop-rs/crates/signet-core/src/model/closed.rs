//! Ports `app/desktop/src/main/model/closed.ts`: stack of closed tabs (reopen) and tab copies for undo operations.

use super::locate::{active_space, active_space_idx, Location};
use super::Model;
use crate::core::store::{ClosedTab, TabHistory};
use crate::factory::new_tab;
use crate::tree::{find_folder_slot, folder_by_id_mut, insert_at, insert_today, outside_groups, tab_item};
use crate::types::{Item, TabGroup, TabInfo, TabItem, Zone};

/// Number of tabs kept for reopening.
pub const CLOSED_MAX: usize = 20;

/// Copy of tabs (and their history) before their destruction.
#[derive(Debug, Clone, PartialEq)]
pub struct TabSnapshot {
    pub tabs: Vec<TabInfo>,
    pub histories: Vec<(String, TabHistory)>,
}

impl Model<'_> {
    /// Batch number: tabs closed at once (group, folder, space) share the same one.
    pub fn next_batch(&mut self) -> u64 {
        self.data.batch_seq += 1;
        self.data.batch_seq
    }

    /// Pushes a tab about to disappear: original place and navigation history included. `batch` links the tabs of a
    /// closed group.
    pub fn remember(&mut self, tab_id: &str, loc: Option<&Location>, batch: Option<u64>, header: Option<&TabGroup>) {
        let Some(tab) = self.data.state.tabs.get(tab_id).cloned() else { return };
        self.env.save_history(tab_id);
        let folder_id = match loc {
            Some(l) if l.zone == Zone::Pinned => l.list.as_ref().and_then(|r| r.folder.clone()),
            _ => None,
        };
        let space_id = match loc.and_then(|l| l.space) {
            Some(i) => self.data.state.spaces[i].id.clone(),
            None => active_space(&self.data.state).id.clone(),
        };
        let group_id = match loc {
            Some(l) if l.zone == Zone::Today && !l.item.is_folder() => l.item.group().map(String::from),
            _ => None,
        };
        self.data.closed.insert(
            0,
            ClosedTab {
                url: tab.url,
                title: tab.title,
                favicon: tab.favicon,
                pinned_url: tab.pinned_url,
                custom_title: tab.custom_title,
                custom_icon: tab.custom_icon,
                space_id,
                zone: loc.map_or(Zone::Today, |l| l.zone),
                folder_id,
                index: loc.map_or(0, |l| l.index),
                history: self.data.histories.get(tab_id).cloned(),
                batch,
                group: header.cloned(),
                group_id,
            },
        );
        self.data.closed.truncate(CLOSED_MAX);
    }

    fn revive(&mut self, c: &ClosedTab) -> TabInfo {
        let mut tab = new_tab(&c.url, &c.title);
        tab.favicon = c.favicon.clone();
        tab.custom_title = c.custom_title.clone();
        tab.custom_icon = c.custom_icon.clone();
        self.data.state.tabs.insert(tab.id.clone(), tab.clone());
        if let Some(h) = &c.history {
            self.data.histories.insert(tab.id.clone(), h.clone());
        }
        tab
    }

    /// Reopens the last closed tab at its original place (otherwise in the current space), with its history. A group
    /// closed at once comes back whole. Returns the number of batch tabs left to reopen, one by one (`None`: empty stack).
    pub fn reopen(&mut self) -> Option<usize> {
        if self.data.closed.is_empty() {
            return None;
        }
        let closed = self.data.closed.remove(0);
        let home = self.data.state.spaces.iter().position(|s| s.id == closed.space_id);
        let space_idx = home.unwrap_or_else(|| active_space_idx(&self.data.state));
        // Group closed at once: all its tabs come back together, grouped, in the group's place.
        let mut rest: Vec<ClosedTab> = Vec::new();
        while closed.batch.is_some() && self.data.closed.first().is_some_and(|c| c.batch == closed.batch) {
            rest.push(self.data.closed.remove(0));
        }
        // Batch without header (folder deleted with its tabs): each tab takes its place, one after the other.
        let mut more = 0;
        if !rest.is_empty() && closed.group.is_none() && closed.group_id.is_none() {
            for c in &mut rest {
                c.batch = None;
            }
            more = rest.len();
            for c in rest.drain(..).rev() {
                self.data.closed.insert(0, c);
            }
        }
        if !rest.is_empty() {
            let group = self.env.new_id();
            let at_default = self.new_tab_index(space_idx);
            let at = outside_groups(&self.data.state.spaces[space_idx].today, if home.is_some() { closed.index } else { at_default });
            let mut all = vec![closed.clone()];
            all.extend(rest);
            let tabs: Vec<TabInfo> = all.iter().map(|c| self.revive(c)).collect();
            for (k, tab) in tabs.iter().enumerate() {
                insert_at(&mut self.data.state.spaces[space_idx].today, at + k, Item::Tab(TabItem { tab_id: tab.id.clone(), group: Some(group.clone()) }));
            }
            if let Some(h) = &closed.group {
                self.data.state.spaces[space_idx].groups.get_or_insert_with(Default::default).insert(group, h.clone());
            }
            self.data.state.active_space_id = self.data.state.spaces[space_idx].id.clone();
            self.set_active(space_idx, Some(&tabs[0].id));
            return Some(0);
        }
        let mut tab = self.revive(&closed);
        // Space deleted: a pinned tab has no place anymore, it comes back among the today tabs.
        let zone = if closed.zone == Zone::Pinned && home.is_none() { Zone::Today } else { closed.zone };
        if zone == Zone::Today {
            let index = if home.is_some() { closed.index } else { self.new_tab_index(space_idx) };
            insert_today(&mut self.data.state.spaces[space_idx].today, index, tab_item(&tab.id), closed.group_id.as_deref());
        } else {
            let pinned = closed.pinned_url.clone().unwrap_or_else(|| closed.url.clone());
            tab.pinned_url = Some(pinned.clone());
            if let Some(t) = self.data.state.tabs.get_mut(&tab.id) {
                t.pinned_url = Some(pinned);
            }
            if zone == Zone::Favorites {
                let favs = &mut self.data.state.favorites;
                favs.insert(closed.index.min(favs.len()), tab.id.clone());
            } else {
                let space = &mut self.data.state.spaces[space_idx];
                let has_folder = closed.folder_id.as_deref().is_some_and(|f| find_folder_slot(&space.pinned, f).is_some());
                if has_folder {
                    if let Some(f) = closed.folder_id.as_deref().and_then(|f| folder_by_id_mut(&mut space.pinned, f)) {
                        insert_at(&mut f.items, closed.index, tab_item(&tab.id));
                    }
                } else {
                    // Folder gone: at the end of the pinned tabs.
                    let at = if closed.folder_id.is_some() { space.pinned.len() } else { closed.index };
                    insert_at(&mut space.pinned, at, tab_item(&tab.id));
                }
            }
        }
        // A favorite is global: we stay in the current space.
        if zone != Zone::Favorites {
            self.data.state.active_space_id = self.data.state.spaces[space_idx].id.clone();
        }
        let target = if zone == Zone::Favorites { active_space_idx(&self.data.state) } else { space_idx };
        self.set_active(target, Some(&tab.id));
        Some(more)
    }

    /// Copy of tabs (and their history) before their destruction.
    pub fn snapshot_tabs(&self, ids: &[String]) -> TabSnapshot {
        TabSnapshot {
            tabs: ids.iter().filter_map(|id| self.data.state.tabs.get(id).cloned()).collect(),
            histories: ids.iter().filter_map(|id| self.data.histories.get(id).map(|h| (id.clone(), h.clone()))).collect(),
        }
    }

    pub fn restore_tabs(&mut self, snap: &TabSnapshot) {
        for tab in &snap.tabs {
            if self.data.state.tabs.contains_key(&tab.id) {
                continue;
            }
            let mut t = tab.clone();
            t.loaded = false;
            t.loading = false;
            t.audible = false;
            self.data.state.tabs.insert(t.id.clone(), t);
        }
        for (id, h) in &snap.histories {
            self.data.histories.insert(id.clone(), h.clone());
        }
    }

    /// Removes from the reopen stack the tabs closed at once (undo done: they must not come back twice).
    pub fn drop_batch(&mut self, batch: u64) {
        self.data.closed.retain(|c| c.batch != Some(batch));
    }
}

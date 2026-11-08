//! Ports `app/desktop/src/main/model/splits.ts`: split views (several tabs side by side): joining, separating, layout
//! and sizes.

use super::locate::{active_space_idx, find_split, list_of_mut, locate};
use super::Model;
use crate::split::{insert_into_split_item, parts_from_lengths, sanitize_split_item, DropEdge, MAX_PANES};
use crate::tree::{insert_today, item_tab_ids, tab_item};
use crate::types::{Item, SplitItem, SplitLayout, TabItem, Zone};

impl Model<'_> {
    /// True if the active tab can host one more pane.
    pub fn can_split(&self) -> bool {
        let st = &self.data.state;
        let Some(active) = st.spaces[active_space_idx(st)].active_tab_id.clone() else { return false };
        let Some(loc) = locate(st, &active) else { return false };
        if loc.zone == Zone::Favorites {
            return false;
        }
        match &loc.item {
            Item::Tab(_) => true,
            Item::Split(s) => s.tab_ids.len() < MAX_PANES,
            Item::Folder(_) => false,
        }
    }

    /// Makes `other_id` join the active tab's split view (without activating it). False if impossible.
    pub fn split_with(&mut self, other_id: &str, edge: Option<DropEdge>) -> bool {
        let st = &self.data.state;
        let Some(active_id) = st.spaces[active_space_idx(st)].active_tab_id.clone() else { return false };
        if active_id == other_id || !st.tabs.contains_key(other_id) || self.data.detached.contains(other_id) || st.favorites.iter().any(|f| f == other_id) {
            return false;
        }
        if !self.can_split() {
            return false;
        }
        if locate(st, &active_id).is_some_and(|l| item_tab_ids(&l.item).iter().any(|t| t == other_id)) {
            return false;
        }
        let from = self.extract(other_id, false);
        let Some(loc) = locate(&self.data.state, &active_id) else { return false };
        let Some(list_ref) = loc.list.clone() else { return false };
        if let Some(list) = list_of_mut(&mut self.data.state, &list_ref) {
            match list[loc.index].clone() {
                Item::Tab(t) => {
                    let mut created = SplitItem { id: self.env.new_id(), tab_ids: vec![active_id.clone()], ..Default::default() };
                    insert_into_split_item(&mut created, other_id, edge.unwrap_or(DropEdge::Right), Some(&active_id));
                    created.group = t.group;
                    list[loc.index] = Item::Split(created);
                }
                Item::Split(mut s) => {
                    if let Some(e) = edge {
                        insert_into_split_item(&mut s, other_id, e, Some(&active_id));
                    } else {
                        s.tab_ids.push(other_id.to_string());
                        s.sizes = None;
                        sanitize_split_item(&mut s);
                    }
                    list[loc.index] = Item::Split(s);
                }
                Item::Folder(_) => {}
            }
        }
        if let Some(other) = self.data.state.tabs.get_mut(other_id) {
            if loc.zone == Zone::Pinned {
                if other.pinned_url.is_none() {
                    other.pinned_url = Some(other.url.clone());
                }
            } else {
                other.pinned_url = None;
            }
        }
        if let Some(from) = from {
            self.repick(&from, &[other_id.to_string()], true);
        }
        self.env.reconcile_sessions(&[other_id.to_string()]);
        true
    }

    /// Undoes the split view: its tabs stay in its place, side by side in the list.
    pub fn unsplit(&mut self, split_id: &str) -> bool {
        let Some(loc) = find_split(&self.data.state, split_id) else { return false };
        let Item::Split(split) = &loc.item else { return false };
        let Some(r) = &loc.list else { return false };
        let Some(list) = list_of_mut(&mut self.data.state, r) else { return false };
        let tabs: Vec<Item> = split.tab_ids.iter().map(|id| if let Some(g) = &split.group { Item::Tab(TabItem { tab_id: id.clone(), group: Some(g.clone()) }) } else { tab_item(id) }).collect();
        list.splice(loc.index..loc.index + 1, tabs);
        true
    }

    /// Takes the tab out of its split view: it is placed right after it.
    pub fn remove_from_split(&mut self, tab_id: &str) -> bool {
        let Some(loc) = locate(&self.data.state, tab_id) else { return false };
        let Item::Split(split) = &loc.item else { return false };
        self.extract(tab_id, false);
        let Some(r) = &loc.list else { return false };
        let Some(list) = list_of_mut(&mut self.data.state, r) else { return false };
        if loc.zone == Zone::Today {
            insert_today(list, loc.index + 1, tab_item(tab_id), split.group.as_deref());
        } else {
            list.insert((loc.index + 1).min(list.len()), tab_item(tab_id));
        }
        true
    }

    fn with_split(&mut self, split_id: &str, f: impl FnOnce(&mut SplitItem) -> bool) -> bool {
        let Some(loc) = find_split(&self.data.state, split_id) else { return false };
        let Some(r) = &loc.list else { return false };
        let Some(list) = list_of_mut(&mut self.data.state, r) else { return false };
        match list.get_mut(loc.index) {
            Some(Item::Split(s)) => f(s),
            _ => false,
        }
    }

    pub fn set_layout(&mut self, split_id: &str, layout: SplitLayout) -> bool {
        self.with_split(split_id, |s| {
            if layout == SplitLayout::Grid && s.tab_ids.len() < 3 {
                return false;
            }
            s.layout = Some(layout);
            s.sizes = None;
            sanitize_split_item(s);
            true
        })
    }

    /// Sizes proposed by the interface; minimums are applied when computing the rectangles.
    pub fn set_sizes(&mut self, split_id: &str, sizes: &[f64]) -> bool {
        let clean: Vec<f64> = sizes.iter().map(|x| if x.is_finite() && *x > 0.0 { *x } else { 0.0 }).collect();
        self.with_split(split_id, |s| {
            s.sizes = Some(parts_from_lengths(&clean));
            sanitize_split_item(s);
            true
        })
    }
}

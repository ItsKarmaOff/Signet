//! Ports `app/desktop/src/main/model/activation.ts`: active tab of each space, and choosing the next one when the
//! active one goes away.

use super::locate::{active_space_idx, list_of, locate, reachable, Location};
use super::Model;
use crate::tree::item_tab_ids;
use crate::types::{Item, Zone};
use std::collections::HashSet;

impl Model<'_> {
    pub fn set_active(&mut self, space_idx: usize, tab_id: Option<&str>) {
        self.data.state.spaces[space_idx].active_tab_id = tab_id.map(String::from);
        let Some(id) = tab_id else { return };
        if self.data.state.tabs.contains_key(id) {
            let now = self.env.now();
            if let Some(t) = self.data.state.tabs.get_mut(id) {
                t.last_active = now;
            }
        }
        self.env.on_activated(&self.data.state.spaces[space_idx], id);
    }

    /// Activates the tab (and the space that contains it). Returns the tab that was active before (`Some(None)`: none),
    /// or `None` if the tab is not found.
    pub fn activate(&mut self, tab_id: &str) -> Option<Option<String>> {
        let loc = locate(&self.data.state, tab_id)?;
        let before = self.data.state.spaces[active_space_idx(&self.data.state)].active_tab_id.clone();
        if let Some(i) = loc.space {
            self.data.state.active_space_id = self.data.state.spaces[i].id.clone();
        }
        let idx = active_space_idx(&self.data.state);
        self.set_active(idx, Some(tab_id));
        Some(before)
    }

    /// First usable tab after the removal of item `index` (or staying in place if `removed` is false).
    pub fn pick_next(&self, space_idx: usize, list: &[Item], index: usize, removed: bool, skip: &HashSet<String>) -> Option<String> {
        let before: Vec<&Item> = list[..index.min(list.len())].iter().rev().collect();
        let mut order: Vec<&Item> = Vec::new();
        if removed {
            order.extend(list.iter().skip(index));
        } else {
            order.extend(list.get(index));
            order.extend(list.iter().skip(index + 1));
        }
        order.extend(before);
        order.extend(self.data.state.spaces[space_idx].pinned.iter());
        order.into_iter().find_map(|item| item_tab_ids(item).into_iter().find(|t| !skip.contains(t)))
    }

    /// After closing (or removing) a favorite: the most recently used open tab (today tabs, pinned, already loaded
    /// favorites), otherwise the first today or pinned tab. Never a sleeping favorite: it is not woken up.
    pub fn after_favorite(&self, space_idx: usize, skip: &HashSet<String>) -> Option<String> {
        let st = &self.data.state;
        let space = &st.spaces[space_idx];
        let open: Vec<String> = space.today.iter().chain(space.pinned.iter()).flat_map(item_tab_ids).filter(|id| !skip.contains(id)).collect();
        let mut live: Vec<String> = open.iter().cloned().chain(st.favorites.iter().filter(|id| !skip.contains(*id)).cloned()).filter(|id| st.tabs.get(id).is_some_and(|t| t.loaded)).collect();
        live.sort_by_key(|id| std::cmp::Reverse(st.tabs[id].last_active));
        live.into_iter().next().or_else(|| open.first().cloned())
    }

    /// After a removal or an unload: picks a new active tab where the old one is no longer reachable.
    pub fn repick(&mut self, loc: &Location, ids: &[String], removed: bool) {
        let skip: HashSet<String> = ids.iter().cloned().collect();
        for i in 0..self.data.state.spaces.len() {
            let Some(current) = self.data.state.spaces[i].active_tab_id.clone() else { continue };
            if !skip.contains(&current) {
                continue;
            }
            if removed && reachable(&self.data.state, &self.data.state.spaces[i], &current) {
                continue;
            }
            let here = loc.zone == Zone::Favorites || loc.space == Some(i);
            let next = if loc.zone == Zone::Favorites {
                self.after_favorite(i, &skip)
            } else if here {
                let list = loc.list.as_ref().and_then(|r| list_of(&self.data.state, r)).cloned().unwrap_or_default();
                self.pick_next(i, &list, loc.index, removed, &skip)
            } else {
                let pinned = self.data.state.spaces[i].pinned.clone();
                self.pick_next(i, &pinned, 0, true, &skip)
            };
            self.set_active(i, next.as_deref());
        }
    }
}

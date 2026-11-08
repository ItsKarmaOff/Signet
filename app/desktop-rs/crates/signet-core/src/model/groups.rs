//! Ports `app/desktop/src/main/model/groups.ts`: today tab groups: headers, taking a tab out, closing a block.

use super::folders::{clean_name, Removed, RemovedUndo};
use super::locate::{locate, space_of_group};
use super::Model;
use crate::tree::{group_range, item_tab_ids};
use crate::types::{Item, TabGroup, Zone};

impl Model<'_> {
    pub fn group_header(&self, group_id: &str) -> Option<&TabGroup> {
        let i = space_of_group(&self.data.state, group_id)?;
        self.data.state.spaces[i].groups.as_ref()?.get(group_id)
    }

    fn group_header_mut(&mut self, group_id: &str) -> Option<&mut TabGroup> {
        let i = space_of_group(&self.data.state, group_id)?;
        self.data.state.spaces[i].groups.as_mut()?.get_mut(group_id)
    }

    /// Takes the tab (or its split view) out of its group: it is placed right after the group.
    pub fn leave_group(&mut self, tab_id: &str) -> bool {
        let Some(loc) = locate(&self.data.state, tab_id) else { return false };
        let group = if loc.zone == Zone::Today { loc.item.group().map(String::from) } else { None };
        let (Some(group), Some(space_idx)) = (group, loc.space) else { return false };
        let today = &mut self.data.state.spaces[space_idx].today;
        let Some((_, end)) = group_range(today, &group) else { return false };
        let mut item = today.remove(loc.index);
        item.set_group(None);
        today.insert(end - 1, item);
        true
    }

    /// Dissolves the group: its tabs stay in place, with no link between them.
    pub fn ungroup(&mut self, group_id: &str) {
        for space in &mut self.data.state.spaces {
            for item in &mut space.today {
                if item.group() == Some(group_id) {
                    item.set_group(None);
                }
            }
        }
    }

    pub fn rename_group(&mut self, group_id: &str, name: &str) -> bool {
        let clean = clean_name(name);
        match self.group_header_mut(group_id) {
            Some(g) if !clean.is_empty() => {
                g.name = clean;
                true
            }
            _ => false,
        }
    }

    pub fn set_group_hue(&mut self, group_id: &str, hue: f64) -> bool {
        match self.group_header_mut(group_id) {
            Some(g) => {
                g.hue = hue;
                true
            }
            None => false,
        }
    }

    /// Emoji instead of the first tab's icon; absent = first tab's icon.
    pub fn set_group_icon(&mut self, group_id: &str, icon: Option<&str>) -> bool {
        match self.group_header_mut(group_id) {
            Some(g) => {
                g.icon = icon.filter(|i| !i.is_empty()).map(String::from);
                true
            }
            None => false,
        }
    }

    pub fn toggle_group(&mut self, group_id: &str, open: Option<bool>) -> bool {
        match self.group_header_mut(group_id) {
            Some(g) => {
                g.open = open.unwrap_or(!g.open);
                true
            }
            None => false,
        }
    }

    /// Closes all the group's tabs; reopening brings them back together, with their header. Returns the undo data, or
    /// `None` if the group does not exist in this space.
    pub fn close_group(&mut self, space_idx: usize, group_id: &str) -> Option<Removed> {
        let (start, end) = group_range(&self.data.state.spaces[space_idx].today, group_id)?;
        let items: Vec<Item> = self.data.state.spaces[space_idx].today[start..end].to_vec();
        let ids: Vec<String> = items.iter().flat_map(item_tab_ids).collect();
        let batch = self.next_batch();
        let header = self.data.state.spaces[space_idx].groups.as_ref().and_then(|g| g.get(group_id)).cloned();
        let snap = self.snapshot_tabs(&ids);
        let was_active = self.data.state.spaces[space_idx].active_tab_id.clone();
        // Stacked from last to first: the group's first tab (which carries the header) ends up at the top of the stack.
        for id in ids.iter().rev() {
            let loc = locate(&self.data.state, id);
            let h = if Some(id) == ids.first() { header.as_ref() } else { None };
            self.remember(id, loc.as_ref(), Some(batch), h);
        }
        self.data.state.spaces[space_idx].today.drain(start..end);
        self.drop_tabs(&ids);
        let loc = super::locate::Location {
            zone: Zone::Today,
            space: Some(space_idx),
            list: Some(super::locate::ListRef::root(space_idx, Zone::Today)),
            index: start,
            item: items[0].clone(),
            shrunk: false,
        };
        self.repick(&loc, &ids, true);
        let space_id = self.data.state.spaces[space_idx].id.clone();
        Some(Removed {
            count: ids.len(),
            undo: RemovedUndo::Group { space_id, snap, batch, start, saved: items, group_id: group_id.to_string(), header, was_active },
        })
    }
}

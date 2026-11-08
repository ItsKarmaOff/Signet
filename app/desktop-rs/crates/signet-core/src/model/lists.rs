//! Ports `app/desktop/src/main/model/lists.ts`: item lists (favorites, pinned, day tabs, folders): removal, moving, pinning.

use super::locate::{active_space_idx, find_folder, find_split, list_of, list_of_mut, locate, space_of_group, ListRef, Location};
use super::Model;
use crate::split::sanitize_split_item;
use crate::tree::{
    folder_by_id, folder_by_id_mut, folder_contains, group_range, insert_at, insert_today, item_tab_ids, leaves_of, outside_groups, pick_group_hue, tab_item,
};
use crate::types::{DragSource, DropTarget, FolderItem, Item, TabGroup, TabItem, Zone};

impl Model<'_> {
    /// Removes the tab from its list. In a split view, only this tab goes (unless `whole`).
    pub fn extract(&mut self, tab_id: &str, whole: bool) -> Option<Location> {
        let loc = locate(&self.data.state, tab_id)?;
        if loc.zone == Zone::Favorites {
            self.data.state.favorites.remove(loc.index);
            return Some(loc);
        }
        let list_ref = loc.list.clone()?;
        let list = list_of_mut(&mut self.data.state, &list_ref)?;
        if let Item::Split(split) = &loc.item
            && !whole
        {
            let tab_ids: Vec<String> = split.tab_ids.iter().filter(|t| *t != tab_id).cloned().collect();
            // The tab left alone takes the split view's place in its group.
            if tab_ids.len() == 1 {
                list[loc.index] = Item::Tab(TabItem { tab_id: tab_ids[0].clone(), group: split.group.clone() });
            } else {
                let mut left = split.clone();
                left.sizes = None;
                left.tab_ids = tab_ids;
                sanitize_split_item(&mut left);
                list[loc.index] = Item::Split(left);
            }
            return Some(Location { shrunk: true, ..loc });
        }
        list.remove(loc.index);
        Some(loc)
    }

    /// Moves a tab, a split view, a folder or a group. False if nothing moved.
    pub fn move_item(&mut self, source: &DragSource, target: &DropTarget) -> bool {
        if let DragSource::Group { id } = source {
            return if target.zone == Zone::Pinned { self.pin_group(id, Some(target)) } else { self.move_group(id, target) };
        }
        if let (DragSource::Folder { id }, Zone::Today) = (source, target.zone) {
            return self.unpin_folder(id, Some(target));
        }
        let origin = match source {
            DragSource::Tab { tab_id } => locate(&self.data.state, tab_id),
            DragSource::Split { id } => find_split(&self.data.state, id),
            DragSource::Folder { id } => find_folder(&self.data.state, id),
            DragSource::Group { .. } => None,
        };
        let Some(origin) = origin else { return false };
        let item: Item = match source {
            DragSource::Tab { tab_id } => tab_item(tab_id),
            _ => origin.item.clone(),
        };
        let ids = item_tab_ids(&item);
        let dest = self.data.state.spaces.iter().position(|s| s.id == target.space_id).unwrap_or_else(|| active_space_idx(&self.data.state));
        if target.zone == Zone::Favorites && !matches!(item, Item::Tab(_)) {
            return false;
        }
        if item.is_folder() && target.zone != Zone::Pinned {
            return false;
        }
        // Target list: zone root, or folder (pinned only).
        let dest_list: Option<ListRef> = if target.zone == Zone::Favorites {
            None
        } else if target.zone == Zone::Pinned && target.folder_id.is_some() {
            let fid = target.folder_id.as_deref().unwrap_or("");
            let Some(folder) = folder_by_id(&self.data.state.spaces[dest].pinned, fid) else { return false };
            // No dropping a folder into itself or one of its descendants; a live folder fills itself.
            if folder.live.is_some() || item.as_folder().is_some_and(|f| folder_contains(f, &folder.id)) {
                return false;
            }
            Some(ListRef { space: dest, zone: Zone::Pinned, folder: Some(fid.to_string()) })
        } else {
            Some(ListRef::root(dest, target.zone))
        };
        if ids.is_empty() && !item.is_folder() {
            return false;
        }
        let loc = if item.is_folder() {
            if let Some(r) = &origin.list
                && let Some(l) = list_of_mut(&mut self.data.state, r)
            {
                l.remove(origin.index);
            }
            origin.clone()
        } else {
            match self.extract(&ids[0], matches!(item, Item::Split(_))) {
                Some(l) => l,
                None => return false,
            }
        };
        let mut index = target.index;
        let same_list = match &dest_list {
            Some(d) => loc.list.as_ref() == Some(d),
            None => loc.zone == Zone::Favorites,
        };
        if same_list && !loc.shrunk && loc.index < index {
            index -= 1;
        }
        let mut item = item;
        match &dest_list {
            None => {
                let favs = &mut self.data.state.favorites;
                favs.insert(index.min(favs.len()), ids[0].clone());
            }
            Some(d) => {
                if target.zone == Zone::Today {
                    if let Some(list) = list_of_mut(&mut self.data.state, d) {
                        insert_today(list, index, item, target.group_id.as_deref());
                    }
                } else {
                    if !item.is_folder() {
                        item.set_group(None);
                    }
                    if let Some(list) = list_of_mut(&mut self.data.state, d) {
                        insert_at(list, index, item);
                    }
                }
            }
        }
        for id in &ids {
            if let Some(tab) = self.data.state.tabs.get_mut(id) {
                if target.zone == Zone::Today {
                    tab.pinned_url = None;
                } else if tab.pinned_url.is_none() {
                    tab.pinned_url = Some(tab.url.clone());
                }
            }
        }
        self.repick(&loc, &ids, true);
        self.env.reconcile_sessions(&ids);
        true
    }

    /// Moves a whole group into a space's day tabs (never into the middle of another group).
    fn move_group(&mut self, group_id: &str, target: &DropTarget) -> bool {
        if target.zone != Zone::Today {
            return false;
        }
        let Some(from) = self.data.state.spaces.iter().position(|sp| group_range(&sp.today, group_id).is_some()) else { return false };
        let Some((start, end)) = group_range(&self.data.state.spaces[from].today, group_id) else { return false };
        let dest = self.data.state.spaces.iter().position(|sp| sp.id == target.space_id).unwrap_or_else(|| active_space_idx(&self.data.state));
        let items: Vec<Item> = self.data.state.spaces[from].today.drain(start..end).collect();
        let mut index = target.index;
        if dest == from && start < index {
            index = start.max(index.saturating_sub(items.len()));
        }
        // In the middle of another group: land just after it.
        let at = outside_groups(&self.data.state.spaces[dest].today, index);
        let first = items[0].clone();
        let ids: Vec<String> = items.iter().flat_map(item_tab_ids).collect();
        for (k, it) in items.into_iter().enumerate() {
            self.data.state.spaces[dest].today.insert(at + k, it);
        }
        if dest != from
            && let Some(header) = self.data.state.spaces[from].groups.as_mut().and_then(|g| g.shift_remove(group_id))
        {
            self.data.state.spaces[dest].groups.get_or_insert_with(Default::default).insert(group_id.to_string(), header);
        }
        let loc = Location { zone: Zone::Today, space: Some(from), list: Some(ListRef::root(from, Zone::Today)), index: start, item: first, shrunk: false };
        self.repick(&loc, &ids, true);
        self.env.reconcile_sessions(&ids);
        true
    }

    /// Pins a group: it becomes a folder of the pinned items (same name, emoji, tint, collapsed state), at the end of
    /// the pinned items of its space, or at the spot targeted by a drag and drop (root or inside a folder).
    pub fn pin_group(&mut self, group_id: &str, target: Option<&DropTarget>) -> bool {
        let Some(from) = space_of_group(&self.data.state, group_id) else { return false };
        let Some((start, end)) = group_range(&self.data.state.spaces[from].today, group_id) else { return false };
        let dest = target.and_then(|t| self.data.state.spaces.iter().position(|sp| sp.id == t.space_id)).unwrap_or(from);
        let parent_id = target.and_then(|t| t.folder_id.clone());
        if let Some(pid) = &parent_id
            && folder_by_id(&self.data.state.spaces[dest].pinned, pid).is_none()
        {
            return false;
        }
        let header = self.data.state.spaces[from].groups.as_ref().and_then(|g| g.get(group_id)).cloned();
        let mut items: Vec<Item> = self.data.state.spaces[from].today.drain(start..end).collect();
        for it in &mut items {
            it.set_group(None);
        }
        let ids: Vec<String> = items.iter().flat_map(item_tab_ids).collect();
        let first_tab = ids.first().and_then(|id| self.data.state.tabs.get(id));
        let name = header
            .as_ref()
            .map(|h| h.name.clone())
            .filter(|n| !n.is_empty())
            .or_else(|| first_tab.and_then(|t| t.custom_title.clone()).filter(|n| !n.is_empty()))
            .or_else(|| first_tab.map(|t| t.title.clone()).filter(|n| !n.is_empty()))
            .unwrap_or_else(|| self.env.default_folder_name());
        let first_item = items[0].clone();
        let folder = FolderItem {
            id: self.env.new_id(),
            name,
            open: header.as_ref().is_none_or(|h| h.open),
            items,
            hue: header.as_ref().map(|h| h.hue),
            icon: header.as_ref().and_then(|h| h.icon.clone()).filter(|i| !i.is_empty()),
            live: None,
        };
        let pinned = &mut self.data.state.spaces[dest].pinned;
        let list: &mut Vec<Item> = match &parent_id {
            Some(pid) => match folder_by_id_mut(pinned, pid) {
                Some(f) => &mut f.items,
                None => return false,
            },
            None => pinned,
        };
        let at = target.map_or(list.len(), |t| t.index);
        insert_at(list, at, Item::Folder(folder));
        for id in &ids {
            if let Some(tab) = self.data.state.tabs.get_mut(id)
                && tab.pinned_url.is_none()
            {
                tab.pinned_url = Some(tab.url.clone());
            }
        }
        let loc = Location { zone: Zone::Today, space: Some(from), list: Some(ListRef::root(from, Zone::Today)), index: start, item: first_item, shrunk: false };
        self.repick(&loc, &ids, true);
        self.env.reconcile_sessions(&ids);
        true
    }

    /// Unpins a folder: its tabs (subfolders included, flattened) become a group of the day tabs, with its name, emoji,
    /// tint and collapsed state; at the top of the day tabs, or at the targeted spot (never in the middle of another group).
    pub fn unpin_folder(&mut self, folder_id: &str, target: Option<&DropTarget>) -> bool {
        let Some(loc) = find_folder(&self.data.state, folder_id) else { return false };
        let Some(space_idx) = loc.space else { return false };
        let Some(folder) = loc.item.as_folder().cloned() else { return false };
        let mut leaves = leaves_of(&folder.items);
        if leaves.is_empty() {
            return false;
        }
        let dest = target.and_then(|t| self.data.state.spaces.iter().position(|sp| sp.id == t.space_id)).unwrap_or(space_idx);
        if let Some(r) = &loc.list
            && let Some(l) = list_of_mut(&mut self.data.state, r)
        {
            l.remove(loc.index);
        }
        let id = self.env.new_id();
        for leaf in &mut leaves {
            leaf.set_group(Some(id.clone()));
        }
        let index = outside_groups(&self.data.state.spaces[dest].today, target.map_or(0, |t| t.index));
        let ids: Vec<String> = leaves.iter().flat_map(item_tab_ids).collect();
        for (k, leaf) in leaves.into_iter().enumerate() {
            self.data.state.spaces[dest].today.insert(index + k, leaf);
        }
        let groups = self.data.state.spaces[dest].groups.get_or_insert_with(Default::default);
        let hue = folder.hue.unwrap_or_else(|| pick_group_hue(groups));
        groups.insert(id, TabGroup { name: folder.name.clone(), hue, open: folder.open, icon: folder.icon.clone() });
        for tab_id in &ids {
            if let Some(tab) = self.data.state.tabs.get_mut(tab_id) {
                tab.pinned_url = None;
            }
        }
        self.repick(&loc, &ids, true);
        self.env.reconcile_sessions(&ids);
        true
    }

    /// Pins the tab (or its split view) at the end of its space's pinned items, or puts it back among the day tabs.
    pub fn toggle_pin(&mut self, tab_id: &str) -> bool {
        let Some(loc) = locate(&self.data.state, tab_id) else { return false };
        let source = match &loc.item {
            Item::Split(s) if loc.zone != Zone::Favorites => DragSource::Split { id: s.id.clone() },
            _ => DragSource::Tab { tab_id: tab_id.to_string() },
        };
        let space_idx = loc.space.unwrap_or_else(|| active_space_idx(&self.data.state));
        let to_pinned = loc.zone == Zone::Today;
        let index = if to_pinned { self.data.state.spaces[space_idx].pinned.len() } else { 0 };
        let target = DropTarget {
            zone: if to_pinned { Zone::Pinned } else { Zone::Today },
            space_id: self.data.state.spaces[space_idx].id.clone(),
            folder_id: None,
            group_id: None,
            index,
        };
        self.move_item(&source, &target)
    }

    pub fn toggle_favorite(&mut self, tab_id: &str) -> bool {
        let space_id = self.data.state.spaces[active_space_idx(&self.data.state)].id.clone();
        let source = DragSource::Tab { tab_id: tab_id.to_string() };
        if self.data.state.favorites.iter().any(|t| t == tab_id) {
            return self.move_item(&source, &DropTarget { zone: Zone::Today, space_id, folder_id: None, group_id: None, index: 0 });
        }
        let index = self.data.state.favorites.len();
        self.move_item(&source, &DropTarget { zone: Zone::Favorites, space_id, folder_id: None, group_id: None, index })
    }

    /// Moves a tab up (-1) or down (+1) one rank in its list (alternative to drag and drop).
    pub fn move_tab_step(&mut self, tab_id: &str, dir: i32) -> bool {
        let Some(loc) = locate(&self.data.state, tab_id) else { return false };
        let len = match &loc.list {
            Some(r) => list_of(&self.data.state, r).map_or(0, Vec::len),
            None => self.data.state.favorites.len(),
        } as i64;
        let index: i64 = if dir < 0 { loc.index as i64 - 1 } else { loc.index as i64 + 2 };
        if index < 0 || index > len + 1 || (dir > 0 && loc.index as i64 + 1 >= len) {
            return false;
        }
        let source = match &loc.item {
            Item::Split(s) if loc.zone != Zone::Favorites => DragSource::Split { id: s.id.clone() },
            _ => DragSource::Tab { tab_id: tab_id.to_string() },
        };
        let space_idx = loc.space.unwrap_or_else(|| active_space_idx(&self.data.state));
        let folder_id = if loc.zone == Zone::Pinned { loc.list.as_ref().and_then(|r| r.folder.clone()) } else { None };
        let group_id = if loc.zone == Zone::Today && !loc.item.is_folder() { loc.item.group().map(String::from) } else { None };
        let target = DropTarget { zone: loc.zone, space_id: self.data.state.spaces[space_idx].id.clone(), folder_id, group_id, index: index as usize };
        self.move_item(&source, &target)
    }

    pub fn move_tab_to_space(&mut self, tab_id: &str, space_id: &str) -> bool {
        self.move_item(
            &DragSource::Tab { tab_id: tab_id.to_string() },
            &DropTarget { zone: Zone::Today, space_id: space_id.to_string(), folder_id: None, group_id: None, index: 0 },
        )
    }
}

//! Ports `app/desktop/src/main/model/folders.ts`: pinned folders: creation, settings, deletion (with or without their
//! tabs), filing a tab.

use super::locate::{active_space_idx, find_folder, list_of_mut, locate};
use super::Model;
use crate::tree::{find_folder_slot, folder_by_id, folder_by_id_mut, insert_at, item_tab_ids, new_folder};
use crate::types::{DragSource, DropTarget, FolderItem, Item, Zone};

/// Name of a folder or group typed by the user: no surrounding spaces, 80 characters at most.
pub fn clean_name(name: &str) -> String {
    name.trim().chars().take(80).collect()
}

/// What is needed to undo a deletion that removed tabs.
#[derive(Debug, Clone, PartialEq)]
pub enum RemovedUndo {
    Folder { space_id: Option<String>, snap: super::closed::TabSnapshot, batch: u64, parent_id: Option<String>, at: usize, saved: FolderItem, was_active: Option<String> },
    Group { space_id: String, snap: super::closed::TabSnapshot, batch: u64, start: usize, saved: Vec<Item>, group_id: String, header: Option<crate::types::TabGroup>, was_active: Option<String> },
}

/// Result of a deletion that removed tabs: the means to announce and undo (`Model::restore_removed`).
#[derive(Debug, Clone, PartialEq)]
pub struct Removed {
    /// Number of tabs removed.
    pub count: usize,
    pub undo: RemovedUndo,
}

/// Result of `Model::add_to_folder`.
#[derive(Debug, Clone, PartialEq)]
pub struct AddedToFolder {
    pub folder_id: String,
    pub created: bool,
    pub moved: bool,
}

impl Model<'_> {
    /// Creates a folder at the top of the active space's pinned tabs (or of `parent_id`). Returns a copy.
    pub fn create_folder(&mut self, parent_id: Option<&str>) -> FolderItem {
        let space_idx = active_space_idx(&self.data.state);
        let name = self.env.default_folder_name();
        let folder = new_folder(&name);
        let pinned = &mut self.data.state.spaces[space_idx].pinned;
        match parent_id.and_then(|p| folder_by_id_mut(pinned, p)) {
            Some(parent) => {
                parent.open = true;
                insert_at(&mut parent.items, 0, Item::Folder(folder.clone()));
            }
            None => insert_at(pinned, 0, Item::Folder(folder.clone())),
        }
        folder
    }

    /// The folder (any space) as a mutable reference.
    pub fn folder_mut(&mut self, folder_id: &str) -> Option<&mut FolderItem> {
        self.data.state.spaces.iter_mut().find_map(|sp| folder_by_id_mut(&mut sp.pinned, folder_id))
    }

    pub fn rename_folder(&mut self, folder_id: &str, name: &str) -> bool {
        let clean = clean_name(name);
        match self.folder_mut(folder_id) {
            Some(f) if !clean.is_empty() => {
                f.name = clean;
                true
            }
            _ => false,
        }
    }

    /// Tint of a folder's card; `None` removes it.
    pub fn set_folder_hue(&mut self, folder_id: &str, hue: Option<f64>) -> bool {
        match self.folder_mut(folder_id) {
            Some(f) => {
                f.hue = hue;
                true
            }
            None => false,
        }
    }

    /// Emoji instead of the folder icon; absent = folder icon.
    pub fn set_folder_icon(&mut self, folder_id: &str, icon: Option<&str>) -> bool {
        match self.folder_mut(folder_id) {
            Some(f) => {
                f.icon = icon.filter(|i| !i.is_empty()).map(String::from);
                true
            }
            None => false,
        }
    }

    pub fn toggle_folder(&mut self, folder_id: &str, open: Option<bool>) -> bool {
        match self.folder_mut(folder_id) {
            Some(f) => {
                f.open = open.unwrap_or(!f.open);
                true
            }
            None => false,
        }
    }

    /// Deletes the folder; its items move up into its place, or its tabs are destroyed (`with_tabs`). `None` if the
    /// folder does not exist. The inner `Option` is the undo data, present only if tabs were destroyed.
    pub fn delete_folder(&mut self, folder_id: &str, with_tabs: bool) -> Option<Option<Removed>> {
        let loc = find_folder(&self.data.state, folder_id)?;
        let folder = loc.item.as_folder()?.clone();
        let list_ref = loc.list.clone()?;
        let ids = item_tab_ids(&loc.item);
        if !with_tabs {
            let list = list_of_mut(&mut self.data.state, &list_ref)?;
            list.splice(loc.index..loc.index + 1, folder.items);
            return Some(None);
        }
        // The tabs join the reopen stack in a single batch (the folder's first on top) before the folder disappears.
        let batch = self.next_batch();
        let snap = self.snapshot_tabs(&ids);
        let saved = folder;
        let space_id = loc.space.map(|i| self.data.state.spaces[i].id.clone());
        let was_active = loc.space.and_then(|i| self.data.state.spaces[i].active_tab_id.clone());
        let parent_id = list_ref.folder.clone();
        let at = loc.index;
        for id in ids.iter().rev() {
            let l = locate(&self.data.state, id);
            self.remember(id, l.as_ref(), Some(batch), None);
        }
        if let Some(list) = list_of_mut(&mut self.data.state, &list_ref) {
            list.remove(loc.index);
        }
        self.drop_tabs(&ids);
        self.repick(&loc, &ids, true);
        if ids.is_empty() {
            return Some(None);
        }
        Some(Some(Removed { count: ids.len(), undo: RemovedUndo::Folder { space_id, snap, batch, parent_id, at, saved, was_active } }))
    }

    /// Undoes a deletion that removed tabs (folder or group). False if the space disappeared in the meantime.
    pub fn restore_removed(&mut self, undo: &RemovedUndo) -> bool {
        match undo {
            RemovedUndo::Folder { space_id, snap, batch, parent_id, at, saved, was_active } => {
                let Some(sp_idx) = space_id.as_ref().and_then(|id| self.data.state.spaces.iter().position(|s| s.id == *id)) else { return false };
                self.restore_tabs(snap);
                self.drop_batch(*batch);
                // Parent folder gone: at the root of the pinned tabs.
                let sp = &mut self.data.state.spaces[sp_idx];
                let has_parent = parent_id.as_deref().is_some_and(|p| find_folder_slot(&sp.pinned, p).is_some());
                if has_parent {
                    if let Some(f) = parent_id.as_deref().and_then(|p| folder_by_id_mut(&mut sp.pinned, p)) {
                        insert_at(&mut f.items, *at, Item::Folder(saved.clone()));
                    }
                } else {
                    insert_at(&mut sp.pinned, 0, Item::Folder(saved.clone()));
                }
                if let Some(w) = was_active
                    && self.data.state.tabs.contains_key(w)
                {
                    self.set_active(sp_idx, Some(w));
                }
                true
            }
            RemovedUndo::Group { space_id, snap, batch, start, saved, group_id, header, was_active } => {
                let Some(sp_idx) = self.data.state.spaces.iter().position(|s| s.id == *space_id) else { return false };
                self.restore_tabs(snap);
                self.drop_batch(*batch);
                let sp = &mut self.data.state.spaces[sp_idx];
                let at = crate::tree::outside_groups(&sp.today, *start);
                for (k, it) in saved.iter().enumerate() {
                    sp.today.insert(at + k, it.clone());
                }
                if let Some(h) = header {
                    sp.groups.get_or_insert_with(Default::default).insert(group_id.clone(), h.clone());
                }
                if let Some(w) = was_active
                    && self.data.state.tabs.contains_key(w)
                {
                    self.set_active(sp_idx, Some(w));
                }
                true
            }
        }
    }

    /// Files the tab (or its split view) at the end of a folder of its space; `None` folder id = new folder.
    pub fn add_to_folder(&mut self, tab_id: &str, folder_id: Option<&str>) -> Option<AddedToFolder> {
        let loc = locate(&self.data.state, tab_id)?;
        let space_idx = loc.space?;
        if loc.zone == Zone::Favorites {
            return None;
        }
        let source = match &loc.item {
            Item::Split(s) => DragSource::Split { id: s.id.clone() },
            _ => DragSource::Tab { tab_id: tab_id.to_string() },
        };
        let target_id = match folder_id {
            Some(id) => {
                folder_by_id(&self.data.state.spaces[space_idx].pinned, id)?;
                id.to_string()
            }
            None => {
                let name = self.env.default_folder_name();
                let folder = new_folder(&name);
                let id = folder.id.clone();
                insert_at(&mut self.data.state.spaces[space_idx].pinned, 0, Item::Folder(folder));
                id
            }
        };
        let len = folder_by_id(&self.data.state.spaces[space_idx].pinned, &target_id).map_or(0, |f| f.items.len());
        let space_id = self.data.state.spaces[space_idx].id.clone();
        let moved = self.move_item(&source, &DropTarget { zone: Zone::Pinned, space_id, folder_id: Some(target_id.clone()), group_id: None, index: len });
        if let Some(f) = self.folder_mut(&target_id) {
            f.open = true;
        }
        Some(AddedToFolder { folder_id: target_id, created: folder_id.is_none(), moved })
    }
}

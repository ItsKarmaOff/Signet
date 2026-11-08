//! Ports `app/desktop/src/main/model/locate.ts`: pure reads of the persisted state (where a tab, a folder or a split
//! view is, which space and which tab are active).
//!
//! The TS `Location` holds references into lists; here a list is named by a `ListRef` (space index, zone and the id
//! of the folder that owns it, `None` for the zone root) and fetched on demand.

use crate::core::store::State;
use crate::tree::{find_folder_slot, find_split_slot, find_tab_slot, folder_by_id, folder_by_id_mut, item_at, item_tab_ids, Slot};
use crate::types::{FolderItem, Item, Space, SplitItem, TabInfo, Zone};

/// A list of items: the root of a zone of a space, or the `items` of a folder (pinned zone only).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListRef {
    /// Index in `state.spaces`.
    pub space: usize,
    /// `Pinned` or `Today`.
    pub zone: Zone,
    /// Id of the folder that owns the list, `None` for the zone root.
    pub folder: Option<String>,
}

impl ListRef {
    pub fn root(space: usize, zone: Zone) -> ListRef {
        ListRef { space, zone, folder: None }
    }
}

/// Where a tab is in the sidebar.
#[derive(Debug, Clone, PartialEq)]
pub struct Location {
    pub zone: Zone,
    /// Index of the space in `state.spaces`; None for favorites, which are global.
    pub space: Option<usize>,
    /// List that contains `item`; None for favorites.
    pub list: Option<ListRef>,
    /// Rank in the list (in `favorites` for favorites).
    pub index: usize,
    /// Copy of the item (a plain tab item for favorites).
    pub item: Item,
    /// True if the tab left a split view that still exists.
    pub shrunk: bool,
}

/// The list named by `r`.
pub fn list_of<'a>(state: &'a State, r: &ListRef) -> Option<&'a Vec<Item>> {
    let space = state.spaces.get(r.space)?;
    let root = if r.zone == Zone::Pinned { &space.pinned } else { &space.today };
    match &r.folder {
        None => Some(root),
        Some(id) => folder_by_id(root, id).map(|f| &f.items),
    }
}

pub fn list_of_mut<'a>(state: &'a mut State, r: &ListRef) -> Option<&'a mut Vec<Item>> {
    let space = state.spaces.get_mut(r.space)?;
    let root = if r.zone == Zone::Pinned { &mut space.pinned } else { &mut space.today };
    match &r.folder {
        None => Some(root),
        Some(id) => folder_by_id_mut(root, id).map(|f| &mut f.items),
    }
}

/// The zone root of a space.
pub fn zone_root(space: &Space, zone: Zone) -> &Vec<Item> {
    if zone == Zone::Pinned { &space.pinned } else { &space.today }
}

pub fn zone_root_mut(space: &mut Space, zone: Zone) -> &mut Vec<Item> {
    if zone == Zone::Pinned { &mut space.pinned } else { &mut space.today }
}

pub fn from_slot(zone: Zone, space_idx: usize, space: &Space, slot: &Slot) -> Location {
    let root = zone_root(space, zone);
    let folder = slot.parent().and_then(|p| item_at(root, p)).and_then(Item::as_folder).map(|f| f.id.clone());
    Location {
        zone,
        space: Some(space_idx),
        list: Some(ListRef { space: space_idx, zone, folder }),
        index: slot.index(),
        item: item_at(root, &slot.path).cloned().unwrap_or_else(|| Item::tab("")),
        shrunk: false,
    }
}

/// Index of the active space (the first one if the remembered id has disappeared).
pub fn active_space_idx(state: &State) -> usize {
    state.spaces.iter().position(|s| s.id == state.active_space_id).unwrap_or(0)
}

/// Active space (the first one if the remembered id has disappeared).
pub fn active_space(state: &State) -> &Space {
    &state.spaces[active_space_idx(state)]
}

pub fn active_tab(state: &State) -> Option<&TabInfo> {
    let id = active_space(state).active_tab_id.as_ref()?;
    state.tabs.get(id)
}

pub fn locate(state: &State, tab_id: &str) -> Option<Location> {
    if let Some(fav) = state.favorites.iter().position(|t| t == tab_id) {
        return Some(Location { zone: Zone::Favorites, space: None, list: None, index: fav, item: Item::tab(tab_id), shrunk: false });
    }
    for (i, space) in state.spaces.iter().enumerate() {
        for zone in [Zone::Pinned, Zone::Today] {
            if let Some(slot) = find_tab_slot(zone_root(space, zone), tab_id) {
                return Some(from_slot(zone, i, space, &slot));
            }
        }
    }
    None
}

pub fn find_split(state: &State, id: &str) -> Option<Location> {
    for (i, space) in state.spaces.iter().enumerate() {
        for zone in [Zone::Pinned, Zone::Today] {
            if let Some(slot) = find_split_slot(zone_root(space, zone), id) {
                return Some(from_slot(zone, i, space, &slot));
            }
        }
    }
    None
}

/// Folder (among any space's pinned items) and its position.
pub fn find_folder(state: &State, id: &str) -> Option<Location> {
    for (i, space) in state.spaces.iter().enumerate() {
        if let Some(slot) = find_folder_slot(&space.pinned, id) {
            return Some(from_slot(Zone::Pinned, i, space, &slot));
        }
    }
    None
}

/// The folder item of a folder location.
pub fn folder_of(loc: &Location) -> Option<&FolderItem> {
    loc.item.as_folder()
}

/// Is the tab still reachable from the space (favorite, pinned or day tab)?
pub fn reachable(state: &State, space: &Space, tab_id: &str) -> bool {
    state.favorites.iter().any(|t| t == tab_id)
        || space.pinned.iter().chain(space.today.iter()).any(|i| item_tab_ids(i).iter().any(|t| t == tab_id))
}

/// Item (tab or split view) that carries the space's active tab, among the pinned items or the day tabs.
fn active_item(state: &State, space: &Space) -> Option<(String, Option<Item>)> {
    let active = space.active_tab_id.clone()?;
    state.tabs.get(&active)?;
    let item = find_tab_slot(&space.pinned, &active)
        .and_then(|s| item_at(&space.pinned, &s.path).cloned())
        .or_else(|| find_tab_slot(&space.today, &active).and_then(|s| item_at(&space.today, &s.path).cloned()));
    Some((active, item))
}

/// Tabs displayed in the space's web card: those of the active split view, otherwise the active tab alone.
pub fn displayed_tab_ids(state: &State, space: &Space) -> Vec<String> {
    match active_item(state, space) {
        None => Vec::new(),
        Some((active, None)) => vec![active],
        Some((_, Some(item))) => item_tab_ids(&item),
    }
}

/// Displayed split view (the one of the active tab).
pub fn displayed_split(state: &State, space: &Space) -> Option<SplitItem> {
    match active_item(state, space)?.1? {
        Item::Split(s) => Some(s),
        _ => None,
    }
}

/// Index of the space that contains the tab group.
pub fn space_of_group(state: &State, group_id: &str) -> Option<usize> {
    state.spaces.iter().position(|sp| sp.today.iter().any(|i| i.group() == Some(group_id)))
}

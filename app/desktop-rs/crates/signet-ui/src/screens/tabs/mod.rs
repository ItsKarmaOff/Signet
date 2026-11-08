//! Tabs: rows, folders and tab groups, tab row at the top, split dividers, drop zones, hover preview.
//! Port of `renderer/src/tabs/*` (and the small `lib/` helpers they use: optimistic selection, hover preview timing).
//!
//! Public entry points:
//! - [`row`]: [`row::tab_row`], [`row::split_row`], [`row::header_row`], [`row::favorite_tile`], [`row::RowEnv`], [`row::TabDrag`].
//! - [`strip::TabStrip`]: the "tabs on top" row (SPEC v12).
//! - [`dividers::Dividers`] and [`drop_zones::DropZones`]: drawn by the main window over the pane rectangles.
//! - [`preview::TabPreviewView`]: content of the `#preview` window.
//! - [`optimistic`]: [`optimistic::activate_tab`], [`optimistic::switch_space`], [`optimistic::optimistic`].
//! - [`groups`]: tab group runs and drop marks (pure).

pub mod custom;
pub mod dividers;
pub mod drop_zones;
pub mod groups;
pub mod motion;
pub mod optimistic;
pub mod preview;
pub mod preview_hover;
pub mod row;
pub mod strip;

pub use custom::{IconForm, IconFormClose, RenameEvent, RenameField, display_title};
pub use dividers::Dividers;
pub use drop_zones::DropZones;
pub use preview::TabPreviewView;
pub use row::{BoundsMap, DragGhost, NavRing, RowCallbacks, RowEnv, TabDrag};
pub use strip::TabStrip;

use signet_core::types::{Item, MediaInfo, Snapshot};

use crate::app_state::tr;
use crate::widgets::{MediaInfo as MediaView, MediaLabels};

/// Tab ids carried by an item (a folder: all its tabs, at any depth).
pub fn item_tab_ids(i: &Item) -> Vec<String> {
    match i {
        Item::Tab(t) => vec![t.tab_id.clone()],
        Item::Split(s) => s.tab_ids.clone(),
        Item::Folder(f) => f.items.iter().flat_map(item_tab_ids).collect(),
    }
}

/// Key of an item in a list: the tab id for a tab, the item id otherwise.
pub fn item_key(i: &Item) -> String {
    match i {
        Item::Tab(t) => t.tab_id.clone(),
        Item::Split(s) => s.id.clone(),
        Item::Folder(f) => f.id.clone(),
    }
}

/// Item (tab or shared view) that contains the tab, at any depth.
pub fn leaf_of<'a>(items: &'a [Item], tab_id: &str) -> Option<&'a Item> {
    for i in items {
        match i {
            Item::Folder(f) => {
                if let Some(x) = leaf_of(&f.items, tab_id) {
                    return Some(x);
                }
            }
            _ => {
                if item_tab_ids(i).iter().any(|t| t == tab_id) {
                    return Some(i);
                }
            }
        }
    }
    None
}

/// Ids of the folder and its subfolders: forbidden targets while it is being dragged.
pub fn folder_ids(i: &Item) -> Vec<String> {
    match i {
        Item::Folder(f) => std::iter::once(f.id.clone()).chain(f.items.iter().flat_map(folder_ids)).collect(),
        _ => vec![],
    }
}

pub fn find_folder<'a>(items: &'a [Item], id: &str) -> Option<&'a signet_core::types::FolderItem> {
    for i in items {
        if let Item::Folder(f) = i {
            if f.id == id {
                return Some(f);
            }
            if let Some(x) = find_folder(&f.items, id) {
                return Some(x);
            }
        }
    }
    None
}

/// Media card data from the snapshot (the tab playing elsewhere), with its favicon.
pub fn media_view(s: &Snapshot) -> Option<(MediaView, String)> {
    let m: &MediaInfo = s.media.as_ref()?;
    let tab = s.tabs.get(&m.tab_id);
    Some((
        MediaView {
            title: m.title.clone().into(),
            artist: m.artist.clone().filter(|a| !a.is_empty()).map(Into::into),
            site: m.site.clone().into(),
            artwork: m.artwork.clone().filter(|a| !a.is_empty()).map(|a| gpui::ImageSource::from(gpui::SharedString::from(a))),
            favicon: custom::favicon_source(tab.and_then(|t| t.favicon.as_deref())),
            playing: m.playing,
            muted: m.muted,
            can_skip: m.can_skip,
        },
        m.tab_id.clone(),
    ))
}

/// Translated labels of the media card.
pub fn media_labels() -> MediaLabels {
    MediaLabels {
        previous: tr("media.previous").into(),
        next: tr("media.next").into(),
        play: tr("media.play").into(),
        pause: tr("media.pause").into(),
        mute: tr("media.mute").into(),
        unmute: tr("media.unmute").into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use signet_core::types::{FolderItem, SplitItem, TabItem};

    fn tab(id: &str) -> Item {
        Item::Tab(TabItem { tab_id: id.into(), group: None })
    }
    fn folder(id: &str, items: Vec<Item>) -> Item {
        Item::Folder(FolderItem { id: id.into(), name: id.into(), items, ..Default::default() })
    }

    #[test]
    fn tab_ids_flatten_folders_and_splits() {
        let split = Item::Split(SplitItem { id: "s".into(), tab_ids: vec!["x".into(), "y".into()], ..Default::default() });
        let f = folder("f", vec![tab("a"), folder("g", vec![split])]);
        assert_eq!(item_tab_ids(&f), vec!["a", "x", "y"]);
    }

    #[test]
    fn leaf_of_finds_the_item_at_any_depth() {
        let items = vec![tab("a"), folder("f", vec![folder("g", vec![tab("b")])])];
        assert_eq!(leaf_of(&items, "b"), Some(&tab("b")));
        assert_eq!(leaf_of(&items, "zz"), None);
    }

    #[test]
    fn folder_ids_lists_the_folder_and_its_subfolders() {
        let f = folder("f", vec![folder("g", vec![folder("h", vec![])]), tab("a")]);
        assert_eq!(folder_ids(&f), vec!["f", "g", "h"]);
        assert!(folder_ids(&tab("a")).is_empty());
    }

    #[test]
    fn find_folder_searches_nested_folders() {
        let items = vec![folder("f", vec![folder("g", vec![])])];
        assert_eq!(find_folder(&items, "g").map(|f| f.id.as_str()), Some("g"));
        assert!(find_folder(&items, "x").is_none());
    }
}

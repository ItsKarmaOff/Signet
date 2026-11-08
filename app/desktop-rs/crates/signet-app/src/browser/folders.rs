//! Port of `browser/folder-actions.ts`, `group-actions.ts`, `arrange.ts` and the bookmark gestures.

use signet_core::api::{ArchiveKind, UiEvent};
use signet_core::types::{BookmarkTarget, DragSource, DropTarget, IconTarget};
use signet_features::bookmarks as bm;

use super::{Core, UndoAction};

impl Core {
    pub fn request_rename(&mut self, folder_id: &str) {
        self.emit(UiEvent::RenameFolder { folder_id: folder_id.to_string() });
    }

    pub fn create_folder(&mut self) {
        let f = self.model(|m| m.create_folder(None));
        self.changed();
        self.request_rename(&f.id);
    }

    pub fn rename_folder(&mut self, id: &str, name: &str) {
        if self.model(|m| m.rename_folder(id, name)) {
            self.changed();
        }
    }

    pub fn toggle_folder(&mut self, id: &str, open: Option<bool>) {
        if self.model(|m| m.toggle_folder(id, open)) {
            self.changed();
        }
    }

    pub fn delete_folder(&mut self, id: &str, with_tabs: bool) {
        let Some(removed) = self.model(|m| m.delete_folder(id, with_tabs)) else { return };
        self.changed();
        if let Some(r) = removed {
            self.announce(ArchiveKind::TabsClosed, r.count, String::new(), UndoAction::Removed(r.undo));
        }
    }

    pub fn add_to_folder(&mut self, tab_id: &str, folder_id: Option<&str>) {
        let Some(r) = self.model(|m| m.add_to_folder(tab_id, folder_id)) else { return };
        if r.moved {
            self.changed();
        }
        if r.created {
            self.request_rename(&r.folder_id);
        }
    }

    pub fn rename_group(&mut self, id: &str, name: &str) {
        if self.model(|m| m.rename_group(id, name)) {
            self.changed();
        }
    }

    pub fn toggle_group(&mut self, id: &str, open: Option<bool>) {
        if self.model(|m| m.toggle_group(id, open)) {
            self.changed();
        }
    }

    pub fn close_group(&mut self, id: &str) {
        let Some(idx) = signet_core::model::space_of_group(self.state(), id) else { return };
        let Some(removed) = self.model(|m| m.close_group(idx, id)) else { return };
        self.changed();
        self.announce(ArchiveKind::TabsClosed, removed.count, String::new(), UndoAction::Removed(removed.undo));
    }

    pub fn leave_group(&mut self, tab_id: &str) {
        if self.model(|m| m.leave_group(tab_id)) {
            self.changed();
        }
    }

    pub fn ungroup(&mut self, id: &str) {
        self.model(|m| m.ungroup(id));
        self.changed();
    }

    pub fn move_item(&mut self, source: &DragSource, target: &DropTarget) {
        if self.model(|m| m.move_item(source, target)) {
            self.changed();
        }
    }

    pub fn set_icon(&mut self, target: &IconTarget, emoji: Option<&str>) {
        let ok = match target {
            IconTarget::Tab { tab_id } => match self.store.data.state.tabs.get_mut(tab_id) {
                Some(t) => {
                    t.custom_icon = emoji.map(str::to_string);
                    true
                }
                None => false,
            },
            IconTarget::Folder { folder_id } => self.model(|m| m.set_folder_icon(folder_id, emoji)),
            IconTarget::Group { group_id } => self.model(|m| m.set_group_icon(group_id, emoji)),
        };
        if ok {
            self.changed();
        }
    }

    pub fn rename_tab(&mut self, tab_id: &str, title: Option<&str>) {
        if let Some(t) = self.store.data.state.tabs.get_mut(tab_id) {
            t.custom_title = title.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
            self.changed();
        }
    }

    // ------------------------------------------------------------------ bookmarks

    fn with_bookmarks<R>(&mut self, f: impl FnOnce(&mut Vec<signet_core::types::Bookmark>) -> R) -> R {
        let idx = signet_core::model::active_space_idx(self.state());
        let list = self.store.data.state.spaces[idx].bookmarks.get_or_insert_with(Vec::new);
        let r = f(list);
        if list.is_empty() {
            self.store.data.state.spaces[idx].bookmarks = None;
        }
        r
    }

    fn bookmarks_changed(&mut self) {
        self.store.schedule_bookmarks();
        self.changed();
    }

    pub fn toggle_bookmark(&mut self, tab: Option<&str>) {
        let Some(id) = tab.map(str::to_string).or_else(|| self.active_tab_id()) else { return };
        let Some(t) = self.state().tabs.get(&id).cloned() else { return };
        if !bm::bookmarkable(&t.url) {
            return;
        }
        self.with_bookmarks(|l| bm::toggle(l, &bm::TabInfo { url: &t.url, title: &t.title, custom_title: t.custom_title.as_deref(), favicon: t.favicon.as_deref() }));
        self.bookmarks_changed();
    }

    pub fn add_bookmark(&mut self, tab_id: &str, target: &BookmarkTarget) {
        let Some(t) = self.state().tabs.get(tab_id).cloned() else { return };
        if self.with_bookmarks(|l| bm::add(l, &bm::TabInfo { url: &t.url, title: &t.title, custom_title: t.custom_title.as_deref(), favicon: t.favicon.as_deref() }, target)) {
            self.bookmarks_changed();
        }
    }

    pub fn move_bookmark(&mut self, id: &str, target: &BookmarkTarget) {
        if self.with_bookmarks(|l| bm::move_to(l, id, target)) {
            self.bookmarks_changed();
        }
    }

    pub fn rename_bookmark(&mut self, id: &str, name: &str) {
        if self.with_bookmarks(|l| bm::rename(l, id, name)) {
            self.bookmarks_changed();
        }
    }

    pub fn remove_bookmark(&mut self, id: &str) {
        if self.with_bookmarks(|l| bm::remove(l, id)) {
            self.bookmarks_changed();
        }
    }

    pub fn new_bookmark_folder(&mut self) {
        let name = signet_core::i18n::t("defaults.folder");
        let id = self.with_bookmarks(|l| bm::new_folder(l, &name));
        self.bookmarks_changed();
        self.emit(UiEvent::RenameBookmark { id });
    }

    pub fn toggle_bookmarks_bar(&mut self) {
        let shown = self.bookmarks_visible();
        self.store.data.state.settings.show_bookmarks_bar = Some(!shown);
        self.changed();
    }

    /// Opens a bookmark in the pane's tab or in a new background tab.
    pub fn open_bookmark(&mut self, id: &str, tab_id: Option<&str>, new_tab: bool) {
        let url = {
            let list = self.active_space().bookmarks.clone().unwrap_or_default();
            match bm::find(&list, id) {
                Some(signet_core::types::Bookmark::Link { url, .. }) => url.clone(),
                _ => return,
            }
        };
        match (new_tab, tab_id.map(str::to_string).or_else(|| self.active_tab_id())) {
            (false, Some(t)) => self.load_url(&t, &url, false),
            _ => {
                self.open_tab(&url, Default::default());
            }
        }
    }
}

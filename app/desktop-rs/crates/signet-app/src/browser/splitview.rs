//! Port of `browser/splitview.ts` and the split part of `drag.ts`.

use signet_core::split::DropEdge;
use signet_core::types::{DropZone, SplitLayout};

use super::Core;

impl Core {
    pub fn split_active_with(&mut self, tab_id: &str, edge: Option<DropEdge>) {
        if self.model(|m| m.split_with(tab_id, edge)) {
            let space_idx = signet_core::model::active_space_idx(self.state());
            self.model(|m| m.set_active(space_idx, Some(tab_id)));
            self.changed();
        } else {
            self.activate_tab(tab_id);
        }
    }

    pub fn open_in_split(&mut self, url: &str) -> String {
        if !self.model(|m| m.can_split()) {
            return self.open_tab(url, signet_core::model::OpenOptions { activate: true, ..Default::default() });
        }
        let id = self.open_tab(url, Default::default());
        self.split_active_with(&id, None);
        id
    }

    pub fn unsplit(&mut self, split_id: &str) {
        if self.model(|m| m.unsplit(split_id)) {
            self.changed();
        }
    }

    pub fn remove_from_split(&mut self, tab_id: &str) {
        if self.model(|m| m.remove_from_split(tab_id)) {
            self.changed();
        }
    }

    pub fn set_split_layout(&mut self, split_id: &str, layout: SplitLayout) {
        if self.model(|m| m.set_layout(split_id, layout)) {
            self.changed();
        }
    }

    pub fn set_split_sizes(&mut self, split_id: &str, sizes: &[f64], live: bool) {
        if self.model(|m| m.set_sizes(split_id, sizes)) {
            if live {
                self.changed_view();
            } else {
                self.changed();
            }
        }
    }

    /// Drop on a page zone while a tab is dragged from the sidebar.
    pub fn drop_on_content(&mut self, tab_id: &str, zone: DropZone) {
        self.end_drag();
        let edge = match zone {
            DropZone::Left => Some(DropEdge::Left),
            DropZone::Right => Some(DropEdge::Right),
            DropZone::Top => Some(DropEdge::Top),
            DropZone::Bottom => Some(DropEdge::Bottom),
            DropZone::Center => {
                // Replace the displayed tab: simply show the dropped one.
                self.activate_tab(tab_id);
                return;
            }
        };
        self.split_active_with(tab_id, edge);
    }

    pub fn begin_drag(&mut self, _tab_id: &str) {
        self.dragging = true;
        let area = self.content_area;
        self.ov.payload = Some(signet_core::types::OverlayPayload::Dropzones { area, split_card: None });
        self.changed_view();
        self.emit_overlay();
    }

    pub fn end_drag(&mut self) {
        if !self.dragging {
            return;
        }
        self.dragging = false;
        if matches!(self.ov.payload, Some(signet_core::types::OverlayPayload::Dropzones { .. })) {
            self.ov.payload = None;
            self.emit(signet_core::api::UiEvent::OverlayHide { instant: true });
            self.changed_view();
        }
    }
}

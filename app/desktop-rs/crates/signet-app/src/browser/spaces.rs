//! Port of `browser/space-actions.ts`.

use signet_core::api::ArchiveKind;
use signet_core::model::{space_cycle_target, space_number_target, space_step_target};
use signet_core::types::{SpaceDraft, SpaceProfile};

use super::{Core, UndoAction};

impl Core {
    pub fn switch_space(&mut self, space_id: &str) {
        if !self.model(|m| m.switch_to_space(space_id)) {
            return;
        }
        self.request_focus();
        self.changed();
    }

    pub fn create_space(&mut self, draft: &SpaceDraft) {
        let space = self.model(|m| m.create_space(draft));
        self.store.schedule_bookmarks();
        self.switch_space(&space.id);
    }

    pub fn update_space(&mut self, space_id: &str, draft: &SpaceDraft) {
        if self.model(|m| m.update_space(space_id, draft)) {
            self.changed();
        }
    }

    pub fn set_space_profile(&mut self, space_id: &str, profile: SpaceProfile) {
        let Some(space) = self.state().spaces.iter().find(|s| s.id == space_id) else { return };
        if space.profile.unwrap_or_default() == profile {
            return;
        }
        // The views of the space's tabs live in the other profile: they are rebuilt on demand.
        let ids = signet_core::model::space_tab_ids(space);
        if self.model(|m| m.set_space_profile(space_id, profile)) {
            for id in ids {
                self.views.destroy(&id);
                if let Some(t) = self.store.data.state.tabs.get_mut(&id) {
                    t.loaded = false;
                }
            }
            self.changed();
        }
    }

    pub fn add_route(&mut self, pattern: &str, space_id: &str) {
        if self.model(|m| m.add_route(pattern, space_id)) {
            self.changed();
        }
    }

    pub fn remove_route(&mut self, route_id: &str) {
        self.model(|m| m.remove_route(route_id));
        self.changed();
    }

    pub fn move_space(&mut self, space_id: &str, step: i32) {
        if self.model(|m| m.move_space(space_id, step)) {
            self.changed();
        }
    }

    pub fn delete_space(&mut self, space_id: &str) {
        let Some(removed) = self.model(|m| m.delete_space(space_id)) else { return };
        let name = removed.space.name.clone();
        let count = removed.tab_count;
        self.request_focus();
        self.store.schedule_bookmarks();
        self.changed();
        self.announce(ArchiveKind::SpaceDeleted, count, name, UndoAction::Space(removed.undo));
    }

    pub fn cycle_space(&mut self, step: i64) {
        let id = space_cycle_target(self.state(), step);
        self.switch_space(&id);
    }

    pub fn step_space(&mut self, step: i64) {
        if let Some(id) = space_step_target(self.state(), step) {
            self.switch_space(&id);
        }
    }

    pub fn goto_space(&mut self, n: usize) {
        if let Some(id) = space_number_target(self.state(), n) {
            self.switch_space(&id);
        }
    }

    /// Asks the interface to open the space form (`None`: new space).
    pub fn edit_space(&mut self, space_id: Option<String>) {
        if self.horizontal() {
            self.open_popover_kind(signet_core::types::PopoverRequest::SpaceForm { space_id });
        } else {
            self.emit(signet_core::api::UiEvent::EditSpace { space_id });
        }
    }
}

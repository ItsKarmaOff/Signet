//! Ports `app/desktop/src/main/model/spaces.ts`: spaces: creation, settings, order, undoable deletion, link routing rules.

use super::locate::{active_space_idx, locate};
use super::Model;
use crate::core::store::State;
use crate::factory::{clean_space_draft, new_space};
use crate::routes::normalize_pattern;
use crate::tree::list_tab_ids;
use crate::types::{Space, SpaceDraft, SpaceProfile, SpaceRoute};

/// Deleted space: what is needed to announce it and put it back.
#[derive(Debug, Clone, PartialEq)]
pub struct SpaceRemoved {
    pub space: Space,
    /// Number of tabs destroyed along with the space.
    pub tab_count: usize,
    pub undo: SpaceUndo,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SpaceUndo {
    snap: super::closed::TabSnapshot,
    batch: u64,
    index: usize,
    space: Space,
    routes: Vec<SpaceRoute>,
    was_active: bool,
}

/// Tabs of a space: pinned then day tabs.
pub fn space_tab_ids(space: &Space) -> Vec<String> {
    let mut v = list_tab_ids(&space.pinned);
    v.extend(list_tab_ids(&space.today));
    v
}

impl Model<'_> {
    pub fn has_space(&self, space_id: &str) -> bool {
        self.data.state.spaces.iter().any(|s| s.id == space_id)
    }

    /// Makes this space the active space. False if it does not exist.
    pub fn switch_to_space(&mut self, space_id: &str) -> bool {
        if !self.has_space(space_id) {
            return false;
        }
        self.data.state.active_space_id = space_id.to_string();
        true
    }

    /// Creates a space and returns a copy of it.
    pub fn create_space(&mut self, draft: &SpaceDraft) -> Space {
        let name = self.env.default_space_name();
        let space = new_space(clean_space_draft(draft, &name));
        self.data.state.spaces.push(space.clone());
        space
    }

    pub fn update_space(&mut self, space_id: &str, draft: &SpaceDraft) -> bool {
        let name = self.env.default_space_name();
        let clean = clean_space_draft(draft, &name);
        let Some(space) = self.data.state.spaces.iter_mut().find(|s| s.id == space_id) else { return false };
        space.name = clean.name;
        space.icon = clean.icon;
        space.hue = clean.hue;
        space.theme = clean.theme;
        true
    }

    /// Changes a space's profile (the space may have disappeared during the confirmation); its tabs reload in the new session.
    pub fn set_space_profile(&mut self, space_id: &str, profile: SpaceProfile) -> bool {
        let Some(i) = self.data.state.spaces.iter().position(|s| s.id == space_id) else { return false };
        self.data.state.spaces[i].profile = Some(profile);
        let ids = space_tab_ids(&self.data.state.spaces[i]);
        self.env.reconcile_sessions(&ids);
        true
    }

    pub fn add_route(&mut self, pattern: &str, space_id: &str) -> bool {
        let clean = normalize_pattern(pattern);
        if clean.is_empty() || !self.has_space(space_id) {
            return false;
        }
        if let Some(same) = self.data.state.settings.routes.iter_mut().find(|r| r.pattern == clean) {
            same.space_id = space_id.to_string();
        } else {
            let id = self.env.new_id();
            self.data.state.settings.routes.push(SpaceRoute { id, pattern: clean, space_id: space_id.to_string() });
        }
        true
    }

    pub fn remove_route(&mut self, route_id: &str) {
        self.data.state.settings.routes.retain(|r| r.id != route_id);
    }

    /// Shifts a space by one rank (left/right); the active space stays the same.
    pub fn move_space(&mut self, space_id: &str, step: i32) -> bool {
        let spaces = &mut self.data.state.spaces;
        let Some(from) = spaces.iter().position(|s| s.id == space_id) else { return false };
        let to = from as i64 + i64::from(step);
        if to < 0 || to >= spaces.len() as i64 {
            return false;
        }
        let space = spaces.remove(from);
        spaces.insert(to as usize, space);
        true
    }

    /// Deletes a space with its tabs (which join the reopen stack as one batch). Nothing if it is the last space or it
    /// does not exist. If the deleted space was the active one, its neighbor becomes it.
    pub fn delete_space(&mut self, space_id: &str) -> Option<SpaceRemoved> {
        if self.data.state.spaces.len() < 2 {
            return None;
        }
        let index = self.data.state.spaces.iter().position(|s| s.id == space_id)?;
        // The tabs join the reopen stack while the space still exists (to know their position).
        let tab_ids = space_tab_ids(&self.data.state.spaces[index]);
        let batch = self.next_batch();
        for id in tab_ids.iter().rev() {
            let loc = locate(&self.data.state, id);
            self.remember(id, loc.as_ref(), Some(batch), None);
        }
        let snap = self.snapshot_tabs(&tab_ids);
        let routes: Vec<SpaceRoute> = self.data.state.settings.routes.iter().filter(|r| r.space_id == space_id).cloned().collect();
        let was_active = self.data.state.active_space_id == space_id;
        let space = self.data.state.spaces.remove(index);
        self.data.state.settings.routes.retain(|r| r.space_id != space_id);
        self.drop_tabs(&tab_ids);
        if self.data.state.active_space_id == space_id {
            let n = self.data.state.spaces.len();
            self.data.state.active_space_id = self.data.state.spaces[index.min(n - 1)].id.clone();
        }
        Some(SpaceRemoved { tab_count: tab_ids.len(), undo: SpaceUndo { snap, batch, index, space: space.clone(), routes, was_active }, space })
    }

    /// Puts back a deleted space, its tabs and its routing rules; false if it already came back.
    pub fn restore_space(&mut self, undo: &SpaceUndo) -> bool {
        if self.data.state.spaces.iter().any(|sp| sp.id == undo.space.id) {
            return false;
        }
        self.restore_tabs(&undo.snap);
        self.drop_batch(undo.batch);
        let at = undo.index.min(self.data.state.spaces.len());
        self.data.state.spaces.insert(at, undo.space.clone());
        for r in &undo.routes {
            if !self.data.state.settings.routes.iter().any(|x| x.id == r.id) {
                self.data.state.settings.routes.push(r.clone());
            }
        }
        if undo.was_active {
            self.data.state.active_space_id = undo.space.id.clone();
        }
        true
    }
}

/// Next or previous space, with wrapping.
pub fn space_cycle_target(state: &State, step: i64) -> String {
    let n = state.spaces.len() as i64;
    let i = active_space_idx(state) as i64;
    state.spaces[(i + step).rem_euclid(n) as usize].id.clone()
}

/// Neighboring space without wrapping (three-finger gesture).
pub fn space_step_target(state: &State, step: i64) -> Option<String> {
    let i = active_space_idx(state) as i64 + step;
    if i < 0 { None } else { state.spaces.get(i as usize).map(|s| s.id.clone()) }
}

/// Nth space (starting from 1).
pub fn space_number_target(state: &State, n: usize) -> Option<String> {
    n.checked_sub(1).and_then(|i| state.spaces.get(i)).map(|s| s.id.clone())
}

//! Ports `app/desktop/src/main/model/*`: the persisted-state model, the rules for organizing tabs (open, close, move,
//! archive, group...), without any platform code. Effects (destroying a view, ids, clock) are injected through
//! `ModelEnv`.
//!
//! Usage: `Model::new(&mut store.data, &mut env)` per operation (it only borrows), then call the rule methods.
//! Rules that can be undone return data (`Removed`, `ClearedToday`, `SpaceRemoved`) that you hand back to
//! `restore_removed`, `restore_archive` or `restore_space`.

pub mod activation;
pub mod archive;
pub mod chrome;
pub mod closed;
pub mod folders;
pub mod groups;
pub mod lists;
pub mod locate;
pub mod navigation;
pub mod opening;
pub mod removal;
pub mod spaces;
pub mod splits;
pub mod types;

#[cfg(test)]
pub mod testkit;
#[cfg(test)]
mod tests;

pub use archive::{ArchiveUndo, ClearedToday};
pub use chrome::{is_horizontal, is_sidebar_on, is_sidebar_right};
pub use closed::{TabSnapshot, CLOSED_MAX};
pub use folders::{clean_name, AddedToFolder, Removed, RemovedUndo};
pub use locate::{
    active_space, active_space_idx, active_tab, displayed_split, displayed_tab_ids, find_folder, find_split, list_of, list_of_mut, locate, reachable, space_of_group, ListRef, Location,
};
pub use navigation::{cycle_target, number_target, tab_ids_in_order};
pub use opening::OpenOptions;
pub use spaces::{space_cycle_target, space_number_target, space_step_target, space_tab_ids, SpaceRemoved, SpaceUndo};
pub use types::{ModelData, ModelEnv, ARCHIVE_MAX};

/// The rules, borrowing the data and the injected effects for the duration of an operation.
pub struct Model<'a> {
    pub data: &'a mut ModelData,
    pub env: &'a mut dyn ModelEnv,
}

impl<'a> Model<'a> {
    pub fn new(data: &'a mut ModelData, env: &'a mut dyn ModelEnv) -> Self {
        Model { data, env }
    }

    /// Active space.
    pub fn space(&self) -> &crate::types::Space {
        locate::active_space(&self.data.state)
    }

    pub fn active_tab(&self) -> Option<&crate::types::TabInfo> {
        locate::active_tab(&self.data.state)
    }

    /// Where the tab is in the sidebar.
    pub fn locate(&self, tab_id: &str) -> Option<Location> {
        locate::locate(&self.data.state, tab_id)
    }

    /// Tabs shown in the web card of the active space.
    pub fn displayed_tab_ids(&self) -> Vec<String> {
        locate::displayed_tab_ids(&self.data.state, self.space())
    }

    /// Next (+1) or previous (-1) tab in the bar's order.
    pub fn cycle_target(&self, step: i64) -> Option<String> {
        navigation::cycle_target(&self.data.state, step)
    }

    /// Tab number `n` (Cmd+1 to Cmd+9).
    pub fn number_target(&self, n: usize, horizontal: bool) -> Option<String> {
        navigation::number_target(&self.data.state, n, horizontal)
    }
}

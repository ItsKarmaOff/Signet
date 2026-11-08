//! Ports `app/desktop/src/main/model/types.ts`: data and effects the model needs.

use crate::core::store::{ClosedTab, State, TabHistory};
use crate::types::Space;
use indexmap::IndexMap;
use std::collections::HashSet;

/// What the model reads and changes: the persisted state and the structures that go with it (the `Store` embeds one).
#[derive(Debug, Default)]
pub struct ModelData {
    pub state: State,
    /// Navigation history per tab.
    pub histories: IndexMap<String, TabHistory>,
    /// Stack of recently closed tabs, newest first.
    pub closed: Vec<ClosedTab>,
    /// Tabs with no place in the sidebar (Peek, Little Signet).
    pub detached: HashSet<String>,
    /// Last batch number handed out to a set of tabs closed at once.
    pub batch_seq: u64,
}

/// Injected effects: everything the model cannot do by itself.
pub trait ModelEnv {
    fn new_id(&mut self) -> String;
    fn now(&mut self) -> i64;
    /// (Translated) name of a folder or group created without a name.
    fn default_folder_name(&mut self) -> String;
    /// (Translated) name of a space created without a name.
    fn default_space_name(&mut self) -> String;
    /// A tab becomes the active one of a space (live folders, focus...).
    fn on_activated(&mut self, space: &Space, tab_id: &str);
    /// Destroys a tab's web view (the tab stays in the state, unloaded).
    fn destroy_view(&mut self, state: &mut State, tab_id: &str);
    /// Copies the view's navigation history into `histories`.
    fn save_history(&mut self, tab_id: &str);
    /// Destroys the views that are no longer in the right session after a move.
    fn reconcile_sessions(&mut self, tab_ids: &[String]);
}

pub fn clamp(n: i64, min: i64, max: i64) -> i64 {
    n.min(max).max(min)
}

/// Number of tabs kept in the archive.
pub const ARCHIVE_MAX: usize = 500;

//! Ports `app/desktop/src/main/model/chrome.ts`: display modes derived from settings.

use crate::core::store::State;
use crate::types::{SidebarSide, TabLayout};

/// "Tabs on top" layout: no sidebar, a tab row at the top of the window.
pub fn is_horizontal(state: &State) -> bool {
    state.settings.tab_layout == Some(TabLayout::Horizontal)
}

/// Sidebar on the right (vertical layout only).
pub fn is_sidebar_right(state: &State) -> bool {
    state.settings.sidebar_side == Some(SidebarSide::Right) && !is_horizontal(state)
}

/// Sidebar actually shown: never in compact mode nor with tabs on top.
pub fn is_sidebar_on(state: &State) -> bool {
    state.sidebar_visible && !state.settings.compact_mode && !is_horizontal(state)
}

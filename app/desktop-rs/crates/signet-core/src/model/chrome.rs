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

/// Whether the macOS traffic lights should show (Electron `LayoutManager.update` `buttons`). In system fullscreen
/// they always show: the revealed title bar strip is the system's, the layout never covers it.
pub fn traffic_lights_wanted(horizontal: bool, sidebar_right: bool, compact: bool, floating: bool, sidebar_on: bool, fullscreen: bool) -> bool {
    if fullscreen {
        return true;
    }
    if horizontal || sidebar_right {
        !compact || floating
    } else {
        sidebar_on || (compact && floating)
    }
}

/// Visibility to apply now, from the one last applied (Electron `buttonsVisible`): None keeps it. A closing slide
/// hides them at once; an opening one shows them once the bar has settled (`sliding` false).
pub fn traffic_lights_step(applied: bool, wanted: bool, sliding: bool) -> Option<bool> {
    (applied != wanted && (!sliding || !wanted)).then_some(wanted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_cases() {
        assert_eq!(traffic_lights_step(false, true, true), None);
        assert_eq!(traffic_lights_step(false, true, false), Some(true));
        assert_eq!(traffic_lights_step(true, false, true), Some(false));
        assert_eq!(traffic_lights_step(true, true, false), None);
        assert_eq!(traffic_lights_step(false, false, true), None);
    }

    #[test]
    fn shown_after_hide_then_animated_show() {
        let mut applied = true;
        for (wanted, sliding) in [(false, false), (true, true), (true, false)] {
            if let Some(v) = traffic_lights_step(applied, wanted, sliding) {
                applied = v;
            }
        }
        assert!(applied);
    }

    #[test]
    fn wanted_rules() {
        assert!(traffic_lights_wanted(false, false, false, false, true, false));
        assert!(!traffic_lights_wanted(false, false, false, false, false, false));
        assert!(!traffic_lights_wanted(false, false, true, false, false, false));
        assert!(traffic_lights_wanted(false, false, true, true, false, false));
        assert!(traffic_lights_wanted(false, true, false, false, true, false));
        assert!(!traffic_lights_wanted(false, true, true, false, false, false));
        assert!(traffic_lights_wanted(true, false, false, false, false, false));
        for m in 0..32u32 {
            let b = |i: u32| m & (1 << i) != 0;
            assert!(traffic_lights_wanted(b(0), b(1), b(2), b(3), b(4), true));
        }
    }
}

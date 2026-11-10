//! Port of `browser/shell.ts` (sidebar, compact mode, tab layout) and the hover handling of `floating.ts`/`compact.ts`.

use signet_core::core::store::{SIDEBAR_MAX, SIDEBAR_MIN, SIDEBAR_OVERSHOOT};
use signet_core::types::TabLayout;

use super::Core;
use crate::windows::{Cmd, Later};

const HIDE_DELAY: u64 = 250;

impl Core {
    fn hide_floating(&mut self) {
        self.hover.sidebar = false;
        self.hover.toolbar = false;
    }

    pub fn toggle_sidebar(&mut self) {
        if self.horizontal() {
            return;
        }
        self.hide_floating();
        let st = &mut self.store.data.state;
        if st.settings.compact_mode {
            st.settings.compact_mode = false;
            st.sidebar_visible = true;
            self.stop_slide();
        } else {
            st.sidebar_visible = !st.sidebar_visible;
            let show = st.sidebar_visible;
            self.start_slide(show);
        }
        self.changed();
    }

    pub fn set_tab_layout(&mut self, layout: TabLayout) {
        let st = &mut self.store.data.state.settings;
        if st.tab_layout.unwrap_or_default() == layout {
            return;
        }
        st.tab_layout = Some(layout);
        self.stop_slide();
        self.hide_floating();
        self.changed();
    }

    pub fn toggle_tab_layout(&mut self) {
        let next = if self.horizontal() { TabLayout::Vertical } else { TabLayout::Horizontal };
        self.set_tab_layout(next);
    }

    pub fn set_compact(&mut self, on: bool) {
        if self.store.data.state.settings.compact_mode == on {
            return;
        }
        self.store.data.state.settings.compact_mode = on;
        self.stop_slide();
        self.hide_floating();
        self.changed();
    }

    pub fn toggle_compact(&mut self) {
        let on = !self.state().settings.compact_mode;
        self.set_compact(on);
    }

    pub fn set_sidebar_width(&mut self, width: f64, live: bool) {
        let rounded = width.round();
        if live {
            let next = rounded.clamp(SIDEBAR_MIN - SIDEBAR_OVERSHOOT, SIDEBAR_MAX + SIDEBAR_OVERSHOOT);
            if self.live_sidebar_width == Some(next) {
                return;
            }
            self.live_sidebar_width = Some(next);
            self.changed_view();
            return;
        }
        let next = rounded.clamp(SIDEBAR_MIN, SIDEBAR_MAX);
        let was_live = self.live_sidebar_width.take().is_some();
        if next == self.state().sidebar_width && !was_live {
            return;
        }
        self.store.data.state.sidebar_width = next;
        self.changed();
    }

    pub fn toggle_peek_links(&mut self) {
        let s = &mut self.store.data.state.settings;
        s.peek_external_links = !s.peek_external_links;
        self.changed();
    }

    pub fn toggle_adblock(&mut self) {
        let s = &mut self.store.data.state.settings;
        s.adblock = !s.adblock;
        // PHASE3: the ad blocker engine (request hook of signet-web) follows the setting.
        self.changed();
    }

    /// The floating sidebar can exist: a main window, the sidebar hidden and in the vertical layout.
    pub(crate) fn can_float_sidebar(&self) -> bool {
        self.win.is_some() && !self.sidebar_on() && !self.horizontal()
    }

    /// The mouse touches the window's left edge (true) or left the floating sidebar (false).
    pub fn set_sidebar_hover(&mut self, hover: bool) {
        if hover {
            self.hover.sidebar_leave.cancel();
            if self.sidebar_on() || self.horizontal() || self.ov.payload.is_some() || self.hover.sidebar {
                return;
            }
            signet_ui::trace::mark(|| "hover enter".into());
            self.hover.sidebar = true;
            self.changed_view();
        } else if self.hover.sidebar
            && let Some(g) = self.hover.sidebar_leave.arm()
        {
            self.send(Cmd::Later(HIDE_DELAY, Later::SidebarLeave(g)));
        }
    }

    /// The mouse touches the web card's top edge (true) or left the floating toolbar (false).
    pub fn set_toolbar_hover(&mut self, hover: bool) {
        if hover {
            if !self.state().settings.compact_mode || self.ov.payload.is_some() || self.hover.toolbar {
                return;
            }
            self.hover.toolbar = true;
            self.changed_view();
        } else if self.hover.toolbar {
            self.send(Cmd::Later(HIDE_DELAY, Later::ToolbarLeave));
        }
    }

    pub fn on_later(&mut self, what: Later) {
        match what {
            Later::UndoExpire(id) => self.undo_expired(id),
            Later::SidebarLeave(g) => {
                if self.hover.sidebar_leave.fire(g) && self.hover.sidebar {
                    self.hover.sidebar = false;
                    self.changed_view();
                }
            }
            Later::SidebarWarm => {
                self.hover.sidebar_warming = false;
                if self.can_float_sidebar() && !self.hover.sidebar_warm {
                    self.hover.sidebar_warm = true;
                    self.dirty.layout = true;
                    self.apply();
                }
            }
            Later::ToolbarLeave => {
                if self.hover.toolbar && !matches!(self.ov.payload, Some(signet_core::types::OverlayPayload::Popover { .. })) {
                    self.hover.toolbar = false;
                    self.changed_view();
                }
            }
            Later::SleepCheck => {
                self.discard_idle();
                self.send(Cmd::Later(60_000, Later::SleepCheck));
            }
            Later::Sweep => {
                self.sweep_archive();
                self.send(Cmd::Later(5 * 60_000, Later::Sweep));
            }
            Later::QuitHudHide => self.quit_hide_due(),
            Later::QuitTick => self.quit_tick(),
            Later::SlideStep => self.slide_step(),
            Later::Focus => {
                self.request_focus();
                self.changed_view();
            }
        }
    }
}

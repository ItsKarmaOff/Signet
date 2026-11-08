//! Port of `windows/Recovery.tsx` + `styles/recovery.css`. React's error boundary has no GPUI equivalent (a panic in
//! `render` unwinds the app), so this module keeps what is portable:
//! - [`report_ui_error`]: the log of an interface error (`report_ui_error` of the API), never failing;
//! - [`FailureTracker`]: the retry rule (at most 3 automatic remounts within 30 s, then offer to reload);
//! - [`Recovery`]: the fallback view (message and "Reload interface" button) the app shows in place of a view that failed
//!   repeatedly, e.g. after catching a panic with `std::panic::catch_unwind` around the creation of a root view.

use super::common::{accent_btn, drag_area};
use crate::app_state::{api, is_installed, tr};
use crate::theme::{ThemeExt, space};
use gpui::{App, AppContext, Context, Entity, IntoElement, ParentElement, Render, StatefulInteractiveElement, Styled, Window, div, px};
use std::rc::Rc;
use std::time::{Duration, Instant};

/// Automatic remounts tolerated within [`RETRY_WINDOW`]; beyond that, offer to reload the interface.
pub const MAX_RETRIES: usize = 3;
pub const RETRY_WINDOW: Duration = Duration::from_secs(30);
/// Wait before an automatic remount.
pub const RETRY_DELAY: Duration = Duration::from_millis(300);

/// Reports an interface error to the main process (log), without ever failing.
pub fn report_ui_error(cx: &App, route: &str, message: &str, stack: &str) {
    if is_installed(cx) {
        api(cx).report_ui_error(route, message, stack);
    }
}

/// Failures of a view within the retry window.
#[derive(Default)]
pub struct FailureTracker {
    failures: Vec<Instant>,
}

impl FailureTracker {
    /// Records a failure at `now`; true if the view may be remounted automatically, false if the reload button is due.
    pub fn record(&mut self, now: Instant) -> bool {
        self.failures.retain(|at| now.duration_since(*at) < RETRY_WINDOW);
        self.failures.push(now);
        self.failures.len() <= MAX_RETRIES
    }
}

/// Fallback view in place of a view left empty after repeated errors.
pub struct Recovery {
    on_reload: Rc<dyn Fn(&mut Window, &mut App)>,
}

impl Recovery {
    pub fn new(_window: &mut Window, cx: &mut App, on_reload: impl Fn(&mut Window, &mut App) + 'static) -> Entity<Self> {
        cx.new(|_: &mut Context<Self>| Self { on_reload: Rc::new(on_reload) })
    }
}

impl Render for Recovery {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.signet().clone();
        let reload = self.on_reload.clone();
        drag_area(
            div()
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(space::S3)
                .p(space::S6)
                .bg(t.bg)
                .text_center()
                .text_color(t.text_2)
                .text_size(t.type_scale.base)
                .child(div().max_w(px(360.0)).child(tr("recovery.message")))
                .child(accent_btn("recovery-reload", &t).h(px(30.0)).on_click(move |_, window, cx| reload(window, cx)).child(tr("recovery.reload"))),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_quick_failures_retry_the_fourth_asks_for_a_reload() {
        let mut tracker = FailureTracker::default();
        let t0 = Instant::now();
        assert!(tracker.record(t0));
        assert!(tracker.record(t0 + Duration::from_secs(1)));
        assert!(tracker.record(t0 + Duration::from_secs(2)));
        assert!(!tracker.record(t0 + Duration::from_secs(3)));
    }

    #[test]
    fn old_failures_are_forgotten() {
        let mut tracker = FailureTracker::default();
        let t0 = Instant::now();
        for i in 0..3 {
            tracker.record(t0 + Duration::from_secs(i));
        }
        assert!(tracker.record(t0 + Duration::from_secs(60)));
    }
}

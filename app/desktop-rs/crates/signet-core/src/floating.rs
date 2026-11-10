//! Floating sidebar placement, window plan, leave timer and card pose (ports of `layout.ts` placement of the
//! `#sidebar` view, `floating.ts` delays and the `.float-sidebar` transitions).

use crate::types::Rect;

/// Inset of the floating sidebar from the window edges (`layout.ts`: x 6, y 6, height H - 12).
pub const FLOAT_INSET: f64 = 6.0;
/// Warm-up of the floating sidebar window once the sidebar is hidden (`floating.ts` SIDEBAR_WARM_MS).
pub const SIDEBAR_WARM_MS: u64 = 1500;

/// Window rectangle of the floating sidebar (window coordinates).
pub fn sidebar_rect(w: f64, h: f64, side_w: f64, right: bool) -> Rect {
    Rect::new(if right { w - FLOAT_INSET - side_w } else { FLOAT_INSET }, FLOAT_INSET, side_w, h - 2.0 * FLOAT_INSET)
}

/// What the window manager does with the floating sidebar window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SidebarWindow {
    Closed,
    /// Open, transparent, ignoring the mouse: ready for the next reveal.
    Parked(Rect),
    Shown(Rect),
}

/// `shown`: `Desired.sidebar`; `warm`: `Desired.sidebar_warm`; `keep_alive`: the platform can park a window
/// (macOS: ignores-mouse child window).
pub fn sidebar_window(shown: Option<Rect>, warm: Option<Rect>, keep_alive: bool) -> SidebarWindow {
    match (shown, warm) {
        (Some(r), _) => SidebarWindow::Shown(r),
        (None, Some(r)) if keep_alive => SidebarWindow::Parked(r),
        _ => SidebarWindow::Closed,
    }
}

/// "Pointer left" timer: one pending at a time, cancelled by any return (`floating.ts` clears hideTimer).
#[derive(Debug, Default, Clone, Copy)]
pub struct LeaveGate {
    generation: u64,
    armed: bool,
}

impl LeaveGate {
    /// Arms the timer; None when one is already pending (it keeps its deadline).
    pub fn arm(&mut self) -> Option<u64> {
        if self.armed {
            return None;
        }
        self.armed = true;
        self.generation += 1;
        Some(self.generation)
    }

    pub fn cancel(&mut self) {
        self.armed = false;
    }

    /// The timer `generation` is due: true if it is still the pending one (it is consumed).
    pub fn fire(&mut self, generation: u64) -> bool {
        let ok = self.armed && generation == self.generation;
        if ok {
            self.armed = false;
        }
        ok
    }
}

/// Card pose while it moves from `start` to `target` (offsets in card widths: 0 in place, -1 / 1 parked past the
/// left / right edge), `eased`: eased progress 0..1. Returns (offset, opacity). Entering (target 0): opacity
/// reaches 1 early (`--t-fast` against `--t-slow`); leaving: opacity follows the position (both `--t` ease-out).
pub fn float_pose(start: f32, target: f32, eased: f32) -> (f32, f32) {
    let offset = start + (target - start) * eased;
    let rest = 1.0 - offset.abs().min(1.0);
    let opacity = if target == 0.0 { (rest * 2.7).min(1.0) } else { rest };
    (offset, opacity)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: (f32, f32), b: (f32, f32)) -> bool {
        (a.0 - b.0).abs() < 1e-5 && (a.1 - b.1).abs() < 1e-5
    }

    #[test]
    fn rect_left_and_right() {
        assert_eq!(sidebar_rect(1200., 800., 240., false), Rect::new(6., 6., 240., 788.));
        assert_eq!(sidebar_rect(1200., 800., 240., true).x, 1200. - 6. - 240.);
    }

    #[test]
    fn window_plan() {
        let a = Rect::new(1., 2., 3., 4.);
        let b = Rect::new(5., 6., 7., 8.);
        assert_eq!(sidebar_window(Some(a), Some(b), true), SidebarWindow::Shown(a));
        assert_eq!(sidebar_window(None, Some(b), true), SidebarWindow::Parked(b));
        assert_eq!(sidebar_window(None, Some(b), false), SidebarWindow::Closed);
        assert_eq!(sidebar_window(None, None, true), SidebarWindow::Closed);
    }

    #[test]
    fn leave_gate() {
        let mut g = LeaveGate::default();
        assert_eq!(g.arm(), Some(1));
        assert_eq!(g.arm(), None);
        assert!(g.fire(1));
        assert!(!g.fire(1));
        assert_eq!(g.arm(), Some(2));
        g.cancel();
        assert!(!g.fire(2));
        assert_eq!(g.arm(), Some(3));
        assert!(!g.fire(2));
        assert!(g.fire(3));
    }

    #[test]
    fn pose() {
        assert!(close(float_pose(-1., 0., 0.), (-1., 0.)));
        assert!(close(float_pose(-1., 0., 1.), (0., 1.)));
        assert!(close(float_pose(1., 0., 0.), (1., 0.)));
        assert!(close(float_pose(0., -1., 1.), (-1., 0.)));
        assert!(close(float_pose(0., 1., 0.5), (0.5, 0.5)));
        let (_, o) = float_pose(-1., 0., 0.63);
        assert!((o - 1.0).abs() < 1e-5);
        assert!(float_pose(-1., 0., 0.3).1 < 1.0);
    }
}

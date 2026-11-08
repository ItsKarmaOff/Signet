//! Ports `app/desktop/src/shared/slide.ts`: sidebar slide spring.

use crate::types::Rect;
use serde::{Deserialize, Serialize};

/// Response (s) of the slide spring (critically damped: no bounce).
pub const SLIDE_RESPONSE: f64 = 0.32;
/// Lead (ms) of the page chrome over the native pages while they widen (closing).
pub const SLIDE_UI_LEAD_MS: f64 = 24.0;

/// Horizontal span of the content area.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub struct Span {
    pub x: f64,
    pub width: f64,
}

/// The spring of one slide segment: from `from` (with `velocity`, per second) at `start` (ms) toward `target`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub struct SlideMotion {
    pub start: f64,
    pub from: f64,
    pub velocity: f64,
    /// 1: shown, 0: hidden.
    pub target: f64,
}

/// What the main UI needs to play the slide.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub struct SlidePlan {
    /// Content area as laid out (sidebar hidden).
    pub from: Span,
    /// Sidebar width.
    pub width: f64,
    /// Gap kept when it is hidden.
    pub gap: f64,
    pub right: bool,
    pub motion: SlideMotion,
}

/// Frame of the slide: the bar's shift (px) and the area left to the pages.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SidebarSlide {
    pub progress: f64,
    pub sidebar: f64,
    pub from: Span,
    pub to: Span,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SlideState {
    pub progress: f64,
    pub velocity: f64,
    pub done: bool,
}

/// Position (1 shown, 0 hidden) and speed of the slide at `now` (ms); `done` once settled.
pub fn slide_state(m: &SlideMotion, now: f64) -> SlideState {
    let omega = (2.0 * std::f64::consts::PI) / SLIDE_RESPONSE;
    let t = ((now - m.start) / 1000.0).max(0.0);
    let x0 = m.from - m.target;
    let c = m.velocity + omega * x0;
    let decay = (-omega * t).exp();
    let x = (x0 + c * t) * decay;
    let v = (c - omega * (x0 + c * t)) * decay;
    if x.abs() < 0.001 && v.abs() < 0.01 {
        return SlideState { progress: m.target, velocity: 0.0, done: true };
    }
    SlideState { progress: m.target + x, velocity: v, done: false }
}

/// Frame of the slide at progress `p`.
pub fn slide_at(plan: &SlidePlan, p: f64) -> SidebarSlide {
    let taken = ((plan.width - plan.gap) * p).round();
    let shift = ((1.0 - p) * plan.width).round();
    SidebarSlide {
        progress: p,
        sidebar: if plan.right { shift } else { 0.0 - shift },
        from: plan.from,
        to: Span {
            x: if plan.right { plan.from.x } else { plan.from.x + taken },
            width: plan.from.width - taken,
        },
    }
}

/// `r` (laid out in `from`) stretched into `to`; `keep_width` only moves it (right edge followed).
pub fn slide_rect(r: Rect, s: &SidebarSlide, keep_width: bool) -> Rect {
    let k = if s.from.width > 0.0 { s.to.width / s.from.width } else { 1.0 };
    let at = |x: f64| (s.to.x + (x - s.from.x) * k).round();
    if keep_width {
        return Rect { x: at(r.x + r.width) - r.width, ..r };
    }
    let x = at(r.x);
    Rect { x, width: (at(r.x + r.width) - x).max(0.0), ..r }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settles_on_target() {
        let m = SlideMotion { start: 0.0, from: 0.0, velocity: 0.0, target: 1.0 };
        assert!(!slide_state(&m, 10.0).done);
        let end = slide_state(&m, 5000.0);
        assert!(end.done && end.progress == 1.0);
    }

    #[test]
    fn frame_at_rest_has_no_negative_zero() {
        let plan = SlidePlan { from: Span { x: 0.0, width: 1000.0 }, width: 240.0, gap: 8.0, right: false, motion: SlideMotion::default() };
        let f = slide_at(&plan, 1.0);
        assert!(f.sidebar.is_sign_positive() && f.sidebar == 0.0);
        assert_eq!(f.to.width, 1000.0 - 232.0);
    }
}

//! Port of `lib/useSpaceSwipe.ts`: horizontal swipe between pages (spaces, or pages of space icons) on the trackpad.
//! The track follows the gesture 1:1, rubber-banded at the first and last page, then a critical spring (starting from
//! the gesture velocity) brings it to the neighbor or back.
//!
//! Pure state machine: the view feeds it wheel deltas (`wheel`) and frame ticks (`frame`) with a millisecond clock,
//! and reads `offset()` to place the track. Wheel deltas use the DOM sign (fingers moving left give `dx > 0`).

use crate::motion::{PROJECT_RATE, RUBBER_C, Spring, project, rubberband, unrubberband};

/// Silence that marks the end of a gesture.
const END_DELAY: f64 = 90.0;
/// Silence that marks the end of the momentum of an already settled gesture.
const MOMENTUM_END: f64 = 120.0;
/// Window over which velocity is measured.
const VELOCITY_WINDOW: f64 = 100.0;
/// Horizontal movement (px) that triggers the switch when animations are reduced.
const REDUCED_THRESHOLD: f32 = 80.0;
/// Cumulative movement (px) from which the gesture axis is decided.
const LOCK_AT: f32 = 8.0;
/// Velocity (px/s) above which a fling bounces slightly.
const FLICK: f32 = 1500.0;

/// What the swipe needs to know about its pages.
#[derive(Clone, Copy, Debug)]
pub struct SwipeCfg {
    /// Width of one page.
    pub width: f32,
    /// Index of the active page.
    pub index: usize,
    /// Number of pages.
    pub count: usize,
    pub reduced: bool,
    /// A page change coming from elsewhere slides with a spring from the displayed position.
    pub animate_jumps: bool,
}

impl SwipeCfg {
    fn has_prev(&self) -> bool {
        self.index > 0
    }
    fn has_next(&self) -> bool {
        self.index + 1 < self.count
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
enum Gs {
    #[default]
    Idle,
    Pending,
    Track,
    Ignore,
}

#[derive(Clone, Copy)]
enum Kind {
    /// Release of a gesture toward `target` (0 = back to the active page, +-width = neighbor).
    Release(f32),
    /// Settling after a page change.
    Reset,
}

struct Anim {
    spring: Spring,
    start: f64,
    kind: Kind,
}

/// Result of a wheel event.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WheelOut {
    /// The event was taken by the gesture (do not let the parent scroll).
    pub consumed: bool,
    /// Switch to the neighbor page: -1 previous, +1 next.
    pub switch: Option<i32>,
}

#[derive(Default)]
pub struct Swipe {
    /// Translation requested by the fingers, before resistance.
    raw: f32,
    /// Displayed translation.
    shown: f32,
    samples: Vec<(f64, f32)>,
    end_at: Option<f64>,
    fallback_at: Option<f64>,
    anim: Option<Anim>,
    /// Switch requested, waiting for the new state.
    locked: bool,
    /// Reduced animations: already switched during this gesture.
    triggered: bool,
    /// Switch settled mid-gesture: swallow the rest of the trackpad momentum.
    spent: bool,
    spent_until: Option<f64>,
    spent_hist: Vec<f32>,
    state: Gs,
    acc_x: f32,
    acc_y: f32,
    swiping: bool,
}

impl Swipe {
    /// Displayed translation of the track (px), 0 at rest.
    pub fn offset(&self) -> f32 {
        self.shown
    }

    /// Dev capture harness (`SIGNET_SWIPE`): freezes the track at `x` px, previews drawn, no gesture running.
    pub fn freeze(&mut self, x: f32) {
        self.raw = x;
        self.shown = x;
        self.swiping = true;
        self.state = Gs::Ignore;
    }

    /// True from the start of a gesture until the track is back at rest (previews are drawn, frames requested).
    pub fn swiping(&self) -> bool {
        self.swiping
    }

    /// True while timers or springs are running: the view must keep requesting frames.
    pub fn needs_frames(&self) -> bool {
        self.anim.is_some() || self.end_at.is_some() || self.fallback_at.is_some() || self.spent_until.is_some()
    }

    fn set_state(&mut self, g: Gs) {
        self.state = g;
    }
    fn gs(&self) -> Gs {
        self.state
    }

    /// Displayed translation for a raw translation: rubber-band resistance beyond the first and last page.
    fn to_shown(&self, cfg: &SwipeCfg, r: f32) -> f32 {
        let w = cfg.width;
        let hi = if cfg.has_prev() { w } else { 0.0 };
        let lo = if cfg.has_next() { -w } else { 0.0 };
        if r > hi {
            hi + rubberband(r - hi, w, RUBBER_C)
        } else if r < lo {
            lo + rubberband(r - lo, w, RUBBER_C)
        } else {
            r
        }
    }

    /// Inverse of `to_shown`.
    fn to_raw(&self, cfg: &SwipeCfg, x: f32) -> f32 {
        let w = cfg.width;
        let hi = if cfg.has_prev() { w } else { 0.0 };
        let lo = if cfg.has_next() { -w } else { 0.0 };
        if x > hi {
            hi + unrubberband(x - hi, w, RUBBER_C)
        } else if x < lo {
            lo + unrubberband(x - lo, w, RUBBER_C)
        } else {
            x
        }
    }

    fn clear(&mut self) {
        self.swiping = false;
        self.raw = 0.0;
        self.shown = 0.0;
        self.samples.clear();
        self.locked = false;
        self.triggered = false;
        self.fallback_at = None;
    }

    /// The active page changed (`from`: the previous index, None if unknown): the track recenters without a jolt,
    /// then settles from the displayed position when jumps are animated.
    pub fn page_changed(&mut self, cfg: &SwipeCfg, from: Option<usize>, now: f64) {
        self.anim = None;
        // Same on-screen position, expressed from the new page.
        let x = match from {
            Some(f) => self.shown + (cfg.index as f32 - f as f32) * cfg.width,
            None => 0.0,
        };
        self.clear();
        if !cfg.animate_jumps || cfg.reduced || x.abs() < 0.5 {
            return;
        }
        self.swiping = true;
        self.raw = x;
        self.shown = x;
        self.anim = Some(Anim { spring: Spring::new(x, 0.0), start: now, kind: Kind::Reset });
    }

    fn velocity(&self) -> f32 {
        let Some(&(lt, lx)) = self.samples.last() else { return 0.0 };
        let Some(&(ft, fx)) = self.samples.iter().find(|(t, _)| lt - t <= VELOCITY_WINDOW) else { return 0.0 };
        if lt == ft { 0.0 } else { (lx - fx) / (lt - ft) as f32 * 1000.0 }
    }

    fn finish(&mut self, cfg: &SwipeCfg, forced: Option<f32>, now: f64) {
        if self.locked && forced.is_none() {
            return;
        }
        if cfg.reduced {
            self.raw = 0.0;
            self.triggered = false;
            return;
        }
        let w = cfg.width;
        let v = self.velocity();
        // A gesture stopped dead (near-zero velocity) is judged on position alone.
        let projected = self.raw + project(if v.abs() < 40.0 { 0.0 } else { v }, PROJECT_RATE);
        let mut target = 0.0;
        if projected > w / 2.0 && cfg.has_prev() {
            target = w;
        } else if projected < -w / 2.0 && cfg.has_next() {
            target = -w;
        }
        if let Some(f) = forced {
            target = f;
        }
        if self.shown == 0.0 && target == 0.0 {
            self.clear();
            return;
        }
        // A fast fling toward the neighbor keeps its momentum: slight bounce. Otherwise it stays critical.
        let damping = if forced.is_none() && target != 0.0 && v.abs() > FLICK { 0.85 } else { 1.0 };
        let spring = Spring::new(self.shown, target).velocity(if forced.is_some() { 0.0 } else { v }).damping(damping);
        self.anim = Some(Anim { spring, start: now, kind: Kind::Release(target) });
    }

    fn arm(&mut self, now: f64) {
        self.end_at = Some(now + END_DELAY);
    }

    /// One wheel event. `dx`, `dy` in px with the DOM sign; `now` in ms.
    pub fn wheel(&mut self, cfg: &SwipeCfg, dx: f32, dy: f32, now: f64) -> WheelOut {
        let none = WheelOut { consumed: false, switch: None };
        let taken = WheelOut { consumed: true, switch: None };
        // A single page: nothing to slide here, the gesture goes back to the parent.
        if cfg.count < 2 {
            return none;
        }
        if self.spent {
            // Leftover momentum after a switch: ignore it until silence, or until new fingers (|dx| rising).
            if dx.abs() <= dy.abs() {
                return none;
            }
            self.spent_until = Some(now + MOMENTUM_END);
            self.spent_hist.push(dx.abs());
            if self.spent_hist.len() > 3 {
                self.spent_hist.remove(0);
            }
            let h = &self.spent_hist;
            let rising = h.len() == 3 && h[0] < h[1] && h[1] < h[2];
            if !rising {
                return taken;
            }
            self.spent = false;
            self.set_state(Gs::Idle);
            self.spent_until = None;
        }

        if self.gs() == Gs::Idle {
            self.set_state(Gs::Pending);
            self.acc_x = 0.0;
            self.acc_y = 0.0;
        }
        self.arm(now);
        if self.gs() == Gs::Ignore {
            return none; // vertical gesture: leave it alone
        }
        let mut move_x = dx;
        if self.gs() == Gs::Pending {
            // Before the decision, accumulate without cancelling anything: vertical scrolling stays intact.
            self.acc_x += dx;
            self.acc_y += dy;
            if self.acc_x.abs() + self.acc_y.abs() < LOCK_AT {
                return none;
            }
            if self.acc_x.abs() <= 1.5 * self.acc_y.abs() {
                self.set_state(Gs::Ignore);
                return none;
            }
            self.set_state(Gs::Track);
            move_x = self.acc_x;
            if self.anim.is_some() {
                // New gesture during an animation: restart from the displayed position.
                self.anim = None;
                self.raw = self.to_raw(cfg, self.shown);
                self.samples.clear();
            }
        }
        if self.locked {
            return taken;
        }

        if cfg.reduced {
            self.raw -= move_x;
            if !self.triggered && self.raw.abs() >= REDUCED_THRESHOLD {
                let dir: i32 = if self.raw < 0.0 { 1 } else { -1 };
                let next = cfg.index as i64 + dir as i64;
                if next >= 0 && (next as usize) < cfg.count {
                    self.triggered = true;
                    // The space change resets the gesture: swallow the rest of the momentum to avoid chaining.
                    self.spent = true;
                    self.spent_hist.clear();
                    self.set_state(Gs::Ignore);
                    self.spent_until = Some(now + MOMENTUM_END);
                    return WheelOut { consumed: true, switch: Some(dir) };
                }
            }
            return taken;
        }

        self.swiping = true;
        self.raw -= move_x;
        let w = cfg.width;
        // The neighbor is reached: switch right away without waiting for the end of the momentum, which we swallow.
        if (self.raw >= w && cfg.has_prev()) || (self.raw <= -w && cfg.has_next()) {
            let dir = if self.raw > 0.0 { w } else { -w };
            self.raw = dir;
            self.shown = dir;
            self.end_at = None;
            self.set_state(Gs::Ignore);
            self.spent = true;
            self.spent_hist.clear();
            self.spent_until = Some(now + MOMENTUM_END);
            self.finish(cfg, Some(dir), now);
            return taken;
        }
        let x = self.to_shown(cfg, self.raw);
        self.samples.push((now, x));
        if self.samples.len() > 40 {
            self.samples.remove(0);
        }
        self.shown = x;
        taken
    }

    /// Timers and springs. Returns a switch (-1 / +1) when a release settled on a neighbor page.
    pub fn frame(&mut self, cfg: &SwipeCfg, now: f64) -> Option<i32> {
        if let Some(at) = self.spent_until
            && at <= now
        {
            self.spent_until = None;
            self.spent = false;
            self.set_state(Gs::Idle);
        }
        if let Some(at) = self.end_at
            && at <= now
        {
            self.end_at = None;
            let was = self.gs();
            self.set_state(Gs::Idle);
            if was == Gs::Track {
                self.finish(cfg, None, now);
            }
        }
        if let Some(at) = self.fallback_at
            && at <= now
        {
            self.fallback_at = None;
            self.anim = None;
            self.clear();
        }
        let a = self.anim.as_ref()?;
        let t = ((now - a.start) / 1000.0).max(0.0) as f32;
        let (x, _) = a.spring.sample(t);
        let (done, kind) = (a.spring.is_done(t), a.kind);
        self.raw = x;
        self.shown = x;
        if !done {
            return None;
        }
        self.anim = None;
        match kind {
            Kind::Reset => {
                self.clear();
                None
            }
            Kind::Release(target) => {
                if target == 0.0 {
                    self.clear();
                    return None;
                }
                // We stay on the neighboring page until the new state arrives.
                self.locked = true;
                self.fallback_at = Some(now + 600.0);
                let dir = if target > 0.0 { -1 } else { 1 };
                let next = cfg.index as i64 + dir as i64;
                if next >= 0 && (next as usize) < cfg.count {
                    Some(dir)
                } else {
                    self.fallback_at = None;
                    self.clear();
                    None
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(index: usize, count: usize) -> SwipeCfg {
        SwipeCfg { width: 200.0, index, count, reduced: false, animate_jumps: false }
    }

    #[test]
    fn a_single_page_ignores_the_gesture() {
        let mut s = Swipe::default();
        assert!(!s.wheel(&cfg(0, 1), 30.0, 0.0, 0.0).consumed);
    }

    #[test]
    fn vertical_scroll_is_left_alone() {
        let mut s = Swipe::default();
        let c = cfg(1, 3);
        assert!(!s.wheel(&c, 1.0, 10.0, 0.0).consumed);
        assert!(!s.wheel(&c, 1.0, 10.0, 10.0).consumed);
        assert_eq!(s.offset(), 0.0);
    }

    #[test]
    fn horizontal_gesture_follows_the_fingers_with_resistance_at_the_edges() {
        let mut s = Swipe::default();
        let c = cfg(1, 3);
        // Fingers moving left (dx > 0) pull the track to the left.
        assert!(s.wheel(&c, 20.0, 0.0, 0.0).consumed);
        assert!((s.offset() - -20.0).abs() < 1e-3);
        // At the first page, pulling to the right resists.
        let mut e = Swipe::default();
        let first = cfg(0, 3);
        e.wheel(&first, -50.0, 0.0, 0.0);
        assert!(e.offset() > 0.0 && e.offset() < 50.0);
    }

    #[test]
    fn a_slow_gesture_springs_back() {
        let mut s = Swipe::default();
        let c = cfg(1, 3);
        s.wheel(&c, 30.0, 0.0, 0.0);
        s.wheel(&c, 10.0, 0.0, 200.0);
        // Silence ends the gesture; a small movement returns to rest.
        assert_eq!(s.frame(&c, 400.0), None);
        let mut t = 400.0;
        while s.needs_frames() && t < 5000.0 {
            t += 16.0;
            assert_eq!(s.frame(&c, t), None);
        }
        assert_eq!(s.offset(), 0.0);
        assert!(!s.swiping());
    }

    #[test]
    fn crossing_the_neighbor_switches_and_swallows_the_momentum() {
        let mut s = Swipe::default();
        let c = cfg(1, 3);
        let mut switched = None;
        let mut t = 0.0;
        for _ in 0..30 {
            s.wheel(&c, 20.0, 0.0, t);
            t += 8.0;
        }
        while s.needs_frames() && t < 5000.0 {
            t += 16.0;
            if let Some(d) = s.frame(&c, t) {
                switched = Some(d);
                break;
            }
        }
        assert_eq!(switched, Some(1));
        // The rest of the momentum is ignored while the state catches up.
        assert!(s.wheel(&c, 5.0, 0.0, t + 1.0).consumed || s.locked);
    }

    #[test]
    fn reduced_motion_switches_after_a_threshold_without_animation() {
        let mut s = Swipe::default();
        let c = SwipeCfg { reduced: true, ..cfg(1, 3) };
        let mut out = s.wheel(&c, 40.0, 0.0, 0.0);
        assert_eq!(out.switch, None);
        out = s.wheel(&c, 50.0, 0.0, 10.0);
        assert_eq!(out.switch, Some(1));
        assert_eq!(s.offset(), 0.0);
    }

    #[test]
    fn page_change_from_elsewhere_slides_when_jumps_are_animated() {
        let mut s = Swipe::default();
        let c = SwipeCfg { animate_jumps: true, ..cfg(2, 3) };
        s.page_changed(&c, Some(0), 0.0);
        // Two pages away: starts one page-width * 2 off and settles at 0.
        assert!((s.offset() - 400.0).abs() < 1e-3);
        let mut t = 0.0;
        while s.needs_frames() && t < 5000.0 {
            t += 16.0;
            s.frame(&c, t);
        }
        assert_eq!(s.offset(), 0.0);
    }
}

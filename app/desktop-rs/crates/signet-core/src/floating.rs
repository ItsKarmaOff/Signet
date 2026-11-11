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

/// What the window manager does with a floating card window (sidebar, compact toolbar).
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

/// Radius of the floating card (Electron `--r-pop`) when nothing better is known.
pub const CARD_RADIUS: f64 = 14.0;

/// Smallest radius a concentric card gets.
const MIN_CARD_RADIUS: f64 = 4.0;

/// Radius of the web cards and of the native page views (Electron `--r-card`, `layout.ts`) when nothing better is known.
pub const WEB_CARD_RADIUS: f64 = 12.0;

/// Smallest radius a web card gets (older macOS has about 10 px window corners: keep the page corners from going tight).
const MIN_WEB_CARD_RADIUS: f64 = 8.0;

/// Radius of a card inset by `inset` in a window whose corners have `window_radius`: concentric with them, down to
/// `min` so a tiny radius never gives square corners. An unknown or absurd value (an SPI that changed meaning, Linux,
/// Windows, fullscreen) gives `fallback`.
pub fn concentric_radius(window_radius: Option<f64>, inset: f64, min: f64, fallback: f64) -> f64 {
    match window_radius {
        Some(r) if r.is_finite() && r > 0.0 && r <= 64.0 => (r - inset).max(min),
        _ => fallback,
    }
}

/// Radius of the floating card inset by `FLOAT_INSET`, concentric with the window (`CARD_RADIUS` when unknown).
pub fn card_radius(window_radius: Option<f64>) -> f64 {
    concentric_radius(window_radius, FLOAT_INSET, MIN_CARD_RADIUS, CARD_RADIUS)
}

/// Radius of the web cards (pages, their GPUI card, the focus ring minus 3 px): inset 6 px like the floating card.
pub fn web_card_radius(window_radius: Option<f64>) -> f64 {
    concentric_radius(window_radius, FLOAT_INSET, MIN_WEB_CARD_RADIUS, WEB_CARD_RADIUS)
}

/// Axis of a floating card's native slide.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SlideAxis {
    #[default]
    X,
    Y,
}

/// Native motion of a floating card (`mac::card_slide`). `offset`: where the parked card sits, in points, window
/// coordinates (top-left origin: negative = left / up). Durations in ms (`--t-fast` 120, `--t` 200, `--t-slow` 320).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CardMotion {
    pub axis: SlideAxis,
    pub offset: f64,
    /// Translate at all (false with reduced motion: fades only).
    pub slide: bool,
    pub enter_ms: u64,
    pub enter_fade_ms: u64,
    pub exit_ms: u64,
}

/// Floating sidebar: slides its own width from its edge (`.float-sidebar`), fade in `--t-fast`, exit `--t`.
pub fn sidebar_motion(width: f64, right: bool, reduced: bool) -> CardMotion {
    CardMotion { axis: SlideAxis::X, offset: if right { width } else { -width }, slide: !reduced, enter_ms: 320, enter_fade_ms: 120, exit_ms: if reduced { 120 } else { 200 } }
}

/// Distance of the compact toolbar's slide (`.float-toolbar` `translateY(-6px)`).
pub const TOOLBAR_SLIDE: f64 = 6.0;

/// Compact mode's floating toolbar: comes down 6 px (spring, `--t-slow`) while fading in (`--t`); exit `--t`
/// (`floatExitMs`: 120 with reduced motion, fades only).
pub fn toolbar_motion(reduced: bool) -> CardMotion {
    CardMotion { axis: SlideAxis::Y, offset: -TOOLBAR_SLIDE, slide: !reduced, enter_ms: 320, enter_fade_ms: 200, exit_ms: if reduced { 120 } else { 200 } }
}

/// The parked offset in the layer's own space: Core Animation's y grows upward in an unflipped superview.
pub fn layer_offset(m: &CardMotion, superview_flipped: bool) -> f64 {
    match m.axis {
        SlideAxis::X => m.offset,
        SlideAxis::Y => {
            if superview_flipped {
                m.offset
            } else {
                -m.offset
            }
        }
    }
}

/// Warm-up of the compact toolbar window once compact mode is on (same as the sidebar).
pub const TOOLBAR_WARM_MS: u64 = SIDEBAR_WARM_MS;
/// Height of the floating toolbar and its inset from the web card's sides (`compact.ts` FLOAT_HEIGHT, `+ 8`).
pub const TOOLBAR_HEIGHT: f64 = 40.0;
const TOOLBAR_SIDE_INSET: f64 = 8.0;

/// Window rectangle of the compact mode's floating toolbar (`compact.ts` `bounds` / `stripBounds`); `area`: the web card area.
pub fn toolbar_rect(w: f64, area: Rect, horizontal: bool) -> Rect {
    if horizontal {
        Rect::new(FLOAT_INSET, FLOAT_INSET, (w - FLOAT_INSET * 2.0).max(200.0), TOOLBAR_HEIGHT)
    } else {
        Rect::new(area.x + TOOLBAR_SIDE_INSET, FLOAT_INSET, (area.width - 2.0 * TOOLBAR_SIDE_INSET).max(200.0), TOOLBAR_HEIGHT)
    }
}

/// The floating card gets a real behind-window blur under a translucent fill (macOS): translucency on, no high
/// contrast, no system "Reduce transparency". Otherwise it keeps the opaque fill (Electron `tokens.css`).
pub fn float_blur_wanted(glass: bool, high_contrast: bool, reduce_transparency: bool) -> bool {
    glass && !high_contrast && !reduce_transparency
}

/// Side of the stretchable mask image of a blurred card with corner `radius` (points): two corners and one
/// stretchable middle point (`capInsets` = radius on every side). A bad radius gives the smallest image.
pub fn blur_mask_side(radius: f64) -> f64 {
    let r = if radius.is_finite() && radius > 0.0 { radius.ceil() } else { 0.0 };
    2.0 * r + 1.0
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
    fn card_radius_follows_the_window() {
        assert_eq!(card_radius(None), 14.0);
        for r in [0.0, -3.0, f64::NAN, f64::INFINITY, 500.0] {
            assert_eq!(card_radius(Some(r)), 14.0, "{r}");
        }
        assert_eq!(card_radius(Some(16.0)), 10.0);
        assert_eq!(card_radius(Some(26.0)), 20.0);
        // A tiny window radius hits the floor instead of going square or negative.
        assert_eq!(card_radius(Some(8.0)), 4.0);
        assert_eq!(card_radius(Some(2.0)), 4.0);
    }

    #[test]
    fn web_card_radius_follows_the_window() {
        assert_eq!(web_card_radius(None), 12.0);
        for r in [0.0, -3.0, f64::NAN, f64::INFINITY, 500.0] {
            assert_eq!(web_card_radius(Some(r)), 12.0, "{r}");
        }
        assert_eq!(web_card_radius(Some(16.0)), 10.0);
        assert_eq!(web_card_radius(Some(26.0)), 20.0);
        // Older macOS (about 10 px window corners) keeps the floor.
        assert_eq!(web_card_radius(Some(10.0)), 8.0);
        assert_eq!(web_card_radius(Some(8.0)), 8.0);
    }

    #[test]
    fn motions() {
        let s = sidebar_motion(240.0, false, false);
        assert_eq!((s.axis, s.offset, s.slide, s.enter_ms, s.enter_fade_ms, s.exit_ms), (SlideAxis::X, -240.0, true, 320, 120, 200));
        assert_eq!(sidebar_motion(240.0, true, false).offset, 240.0);
        let r = sidebar_motion(240.0, false, true);
        assert!(!r.slide);
        assert_eq!(r.exit_ms, 120);
        let t = toolbar_motion(false);
        assert_eq!((t.axis, t.offset, t.slide, t.enter_fade_ms, t.exit_ms), (SlideAxis::Y, -6.0, true, 200, 200));
        assert_eq!(toolbar_motion(true).exit_ms, 120);
        assert!(!toolbar_motion(true).slide);
    }

    #[test]
    fn layer_offsets() {
        assert_eq!(layer_offset(&toolbar_motion(false), false), 6.0);
        assert_eq!(layer_offset(&toolbar_motion(false), true), -6.0);
        assert_eq!(layer_offset(&sidebar_motion(240.0, false, false), false), -240.0);
        assert_eq!(layer_offset(&sidebar_motion(240.0, true, false), true), 240.0);
    }

    #[test]
    fn toolbar_rects() {
        let area = Rect::new(6.0, 6.0, 1188.0, 788.0);
        assert_eq!(toolbar_rect(1200.0, area, false), Rect::new(14.0, 6.0, 1172.0, 40.0));
        assert_eq!(toolbar_rect(1200.0, area, true), Rect::new(6.0, 6.0, 1188.0, 40.0));
        assert_eq!(toolbar_rect(150.0, Rect::new(6.0, 6.0, 138.0, 100.0), false).width, 200.0);
    }

    #[test]
    fn blur_only_with_translucency() {
        assert!(float_blur_wanted(true, false, false));
        assert!(!float_blur_wanted(false, false, false));
        assert!(!float_blur_wanted(true, true, false));
        assert!(!float_blur_wanted(true, false, true));
    }

    #[test]
    fn mask_side() {
        assert_eq!(blur_mask_side(10.0), 21.0);
        assert_eq!(blur_mask_side(9.5), 21.0);
        for r in [0.0, -2.0, f64::NAN, f64::INFINITY] {
            assert_eq!(blur_mask_side(r), 1.0, "{r}");
        }
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

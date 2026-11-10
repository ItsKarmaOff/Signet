//! Motion helpers: port of `renderer/src/lib/spring.ts`, CSS easings, and exit animations.
//!
//! GPUI has no element scale transform, so the "scale 0.96 to 1 + opacity" materialize of the spec is rendered as
//! opacity plus a small vertical slide ([`surface_enter`]). Exits keep the element mounted for
//! [`EXIT_MS`] with [`Presence`], like main's `OVERLAY_EXIT_MS`.

use gpui::{Animation, AnimationExt, App, Context, Global, IntoElement, Styled, px};
use std::time::{Duration, Instant};

/// Exit duration of animated surfaces (`--t-fast`).
pub const EXIT_MS: u64 = 120;

/// Rubber-band resistance beyond a limit: `d` is the overshoot, `dim` the reference dimension.
pub fn rubberband(d: f32, dim: f32, c: f32) -> f32 {
    if d == 0.0 || dim <= 0.0 {
        return 0.0;
    }
    let a = d.abs();
    d.signum() * (a * dim * c) / (dim + c * a)
}

/// Default rubber-band coefficient.
pub const RUBBER_C: f32 = 0.55;

/// Inverse of [`rubberband`].
pub fn unrubberband(r: f32, dim: f32, c: f32) -> f32 {
    let a = r.abs().min(dim * 0.999);
    r.signum() * (a * dim) / (c * (dim - a))
}

/// Distance a fling (px/s) would travel while decelerating like native scrolling.
pub fn project(velocity: f32, rate: f32) -> f32 {
    (velocity / 1000.0) * (rate / (1.0 - rate))
}

/// Default deceleration rate for [`project`].
pub const PROJECT_RATE: f32 = 0.998;

/// Analytic spring (critical by default), exact whatever the time between frames.
#[derive(Clone, Copy, Debug)]
pub struct Spring {
    pub from: f32,
    pub to: f32,
    /// px/s
    pub velocity: f32,
    /// Period of the undamped spring, in seconds.
    pub response: f32,
    /// 1 = critical (no bounce), below 1 = slight bounce.
    pub damping: f32,
}

impl Spring {
    pub fn new(from: f32, to: f32) -> Self {
        Self { from, to, velocity: 0.0, response: 0.35, damping: 1.0 }
    }
    pub fn velocity(mut self, v: f32) -> Self {
        self.velocity = v;
        self
    }
    pub fn response(mut self, r: f32) -> Self {
        self.response = r;
        self
    }
    pub fn damping(mut self, d: f32) -> Self {
        self.damping = d;
        self
    }

    /// Position and velocity at `t` seconds. Returns `(to, 0)` once settled (`|x| < 0.3` and `|v| < 8`).
    pub fn sample(&self, t: f32) -> (f32, f32) {
        let omega = 2.0 * std::f32::consts::PI / self.response;
        let x0 = self.from - self.to;
        let c2 = self.velocity + omega * x0;
        let (x, v) = if self.damping < 1.0 {
            let decay_rate = self.damping * omega;
            let wd = omega * (1.0 - self.damping * self.damping).max(1e-6).sqrt();
            let sb = (self.velocity + decay_rate * x0) / wd;
            let decay = (-decay_rate * t).exp();
            let (cos, sin) = ((wd * t).cos(), (wd * t).sin());
            (decay * (x0 * cos + sb * sin), decay * (-decay_rate * (x0 * cos + sb * sin) + wd * (sb * cos - x0 * sin)))
        } else {
            let decay = (-omega * t).exp();
            ((x0 + c2 * t) * decay, (c2 - omega * (x0 + c2 * t)) * decay)
        };
        if x.abs() < 0.3 && v.abs() < 8.0 { (self.to, 0.0) } else { (self.to + x, v) }
    }

    /// True once settled at time `t`.
    pub fn is_done(&self, t: f32) -> bool {
        self.sample(t).1 == 0.0 && (self.sample(t).0 - self.to).abs() < f32::EPSILON
    }

    /// Easing closure for GPUI animations: maps normalized time over `duration` to progress 0..1 (can overshoot when
    /// `damping < 1`). Uses `from = 0`, `to = 1` scaled so the settle thresholds are in progress units.
    pub fn easing(response: f32, damping: f32, duration: Duration) -> impl Fn(f32) -> f32 {
        let secs = duration.as_secs_f32();
        move |delta| {
            let s = Spring { from: 0.0, to: 1.0, velocity: 0.0, response, damping };
            let t = delta * secs;
            let omega = 2.0 * std::f32::consts::PI / response;
            let x0 = -1.0f32;
            if damping < 1.0 {
                let dr = damping * omega;
                let wd = omega * (1.0 - damping * damping).max(1e-6).sqrt();
                let sb = (dr * x0) / wd;
                1.0 + (-dr * t).exp() * (x0 * (wd * t).cos() + sb * (wd * t).sin())
            } else {
                let c2 = omega * x0;
                let _ = s;
                1.0 + (x0 + c2 * t) * (-omega * t).exp()
            }
        }
    }
}

/// CSS `cubic-bezier(x1, y1, x2, y2)` as an easing function.
pub fn cubic_bezier(x1: f32, y1: f32, x2: f32, y2: f32) -> impl Fn(f32) -> f32 {
    let bez = |a: f32, b: f32, t: f32| {
        let u = 1.0 - t;
        3.0 * u * u * t * a + 3.0 * u * t * t * b + t * t * t
    };
    move |x| {
        if x <= 0.0 {
            return 0.0;
        }
        if x >= 1.0 {
            return 1.0;
        }
        let (mut lo, mut hi) = (0.0f32, 1.0f32);
        let mut t = x;
        for _ in 0..24 {
            let cur = bez(x1, x2, t);
            if (cur - x).abs() < 1e-5 {
                break;
            }
            if cur < x { lo = t } else { hi = t }
            t = (lo + hi) / 2.0;
        }
        bez(y1, y2, t)
    }
}

/// `--ease-out`: cubic-bezier(0.2, 0.8, 0.2, 1).
pub fn ease_out(x: f32) -> f32 {
    cubic_bezier(0.2, 0.8, 0.2, 1.0)(x)
}

/// The stops (time, progress) of [`spring_curve`], piecewise linear.
pub const SPRING_STOPS: [(f32, f32); 8] = [(0.0, 0.0), (0.04, 0.17), (0.10, 0.48), (0.17, 0.72), (0.25, 0.87), (0.34, 0.95), (0.45, 0.985), (1.0, 1.0)];

/// Key frames (key time, value) of a move from `from` to `to` along [`spring_curve`], for a native linear animation.
pub fn slide_keyframes(from: f64, to: f64) -> Vec<(f64, f64)> {
    SPRING_STOPS.iter().map(|&(x, y)| (f64::from(x), from + (to - from) * f64::from(y))).collect()
}

/// `--spring`: the CSS `linear(0, 0.17 4%, 0.48 10%, 0.72 17%, 0.87 25%, 0.95 34%, 0.985 45%, 1)` curve.
pub fn spring_curve(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    for w in SPRING_STOPS.windows(2) {
        let ((x0, y0), (x1, y1)) = (w[0], w[1]);
        if x <= x1 {
            return y0 + (y1 - y0) * (x - x0) / (x1 - x0);
        }
    }
    1.0
}

/// "Reduce animations" preference, stored as a global. Read by [`surface_enter`] and widgets.
#[derive(Clone, Copy, Default)]
pub struct ReducedMotion(pub bool);
impl Global for ReducedMotion {}

pub fn set_reduced_motion(cx: &mut App, on: bool) {
    cx.set_global(ReducedMotion(on));
}

pub fn reduced_motion(cx: &App) -> bool {
    cx.try_global::<ReducedMotion>().is_some_and(|r| r.0)
}

/// Materialize / leave animation of a floating surface (palette, popover, panel): opacity and a small slide.
/// `closing` runs the same path in reverse. With reduced motion it is a plain fade.
pub fn surface_enter<E>(el: E, id: impl Into<gpui::ElementId>, closing: bool, reduced: bool) -> impl IntoElement
where
    E: IntoElement + Styled + 'static,
{
    surface_enter_dir(el, id, closing, reduced, 1.0)
}

/// [`surface_enter`] with a slide direction: `1.0` rises from below (bottom-anchored), `-1.0` drops from above (top-anchored).
pub fn surface_enter_dir<E>(el: E, id: impl Into<gpui::ElementId>, closing: bool, reduced: bool, dir: f32) -> impl IntoElement
where
    E: IntoElement + Styled + 'static,
{
    let slide = if reduced { 0.0 } else { 4.0 * dir };
    el.with_animation(id, Animation::new(Duration::from_millis(EXIT_MS)).with_easing(ease_out), move |el, d| {
        let p = if closing { 1.0 - d } else { d };
        // Slide with margins, never `position` / `top`: the caller's own placement (absolute card anchored by its
        // top or its bottom, or a row in a flex column) must survive. `mt` moves top-anchored and in-flow elements
        // down, the matching negative `mb` moves bottom-anchored ones and gives the space back to the next siblings.
        let off = (1.0 - p) * slide;
        el.opacity(p).mt(px(off)).mb(px(-off))
    })
}

/// Keeps a value mounted for [`EXIT_MS`] after it goes away, so it can fade out (port of `useExit`).
pub struct Presence<T> {
    value: Option<T>,
    closing: bool,
    generation: u64,
    since: Option<Instant>,
}

impl<T: Clone> Default for Presence<T> {
    fn default() -> Self {
        Self { value: None, closing: false, generation: 0, since: None }
    }
}

impl<T: Clone> Presence<T> {
    pub fn new(value: Option<T>) -> Self {
        Self { value, ..Default::default() }
    }
    /// Value to render (the last one while closing).
    pub fn value(&self) -> Option<&T> {
        self.value.as_ref()
    }
    pub fn closing(&self) -> bool {
        self.closing
    }
    /// Sets the wanted value. A `None` starts the exit; call [`Presence::finish`] after [`EXIT_MS`] (see [`Presence::set`]).
    /// Returns true when an exit timer must be started.
    pub fn update(&mut self, next: Option<T>) -> bool {
        self.generation += 1;
        match next {
            Some(v) => {
                self.value = Some(v);
                self.closing = false;
                self.since = None;
                false
            }
            None if self.value.is_some() && !self.closing => {
                self.closing = true;
                self.since = Some(Instant::now());
                true
            }
            None => false,
        }
    }
    /// Unmounts if still closing for the given generation.
    pub fn finish(&mut self, generation: u64) {
        if self.closing && self.generation == generation {
            self.value = None;
            self.closing = false;
        }
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Sets the value from a view and schedules the unmount, notifying the view when done.
    pub fn set<V: 'static>(view: &mut V, get: fn(&mut V) -> &mut Presence<T>, next: Option<T>, cx: &mut Context<V>)
    where
        T: 'static,
    {
        let p = get(view);
        let start = p.update(next);
        let generation = p.generation();
        cx.notify();
        if start {
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(Duration::from_millis(EXIT_MS)).await;
                this.update(cx, |view, cx| {
                    get(view).finish(generation);
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spring_matches_its_stops() {
        for (x, y) in SPRING_STOPS {
            assert!((spring_curve(x) - y).abs() < 1e-6);
        }
        let ((x0, y0), (x1, y1)) = (SPRING_STOPS[2], SPRING_STOPS[3]);
        assert!((spring_curve((x0 + x1) / 2.0) - (y0 + y1) / 2.0).abs() < 1e-6);
    }

    #[test]
    fn slide_keyframes_span_the_move() {
        let k = slide_keyframes(-200.0, 0.0);
        assert_eq!(k.len(), 8);
        assert_eq!(k[0], (0.0, -200.0));
        assert_eq!(k[7], (1.0, 0.0));
        assert!(k.windows(2).all(|w| w[0].0 < w[1].0));
    }

    #[test]
    fn rubberband_roundtrip() {
        let r = rubberband(80.0, 300.0, RUBBER_C);
        assert!(r > 0.0 && r < 80.0);
        assert!((unrubberband(r, 300.0, RUBBER_C) - 80.0).abs() < 0.01);
        assert_eq!(rubberband(0.0, 300.0, RUBBER_C), 0.0);
        assert!(rubberband(-50.0, 300.0, RUBBER_C) < 0.0);
    }

    #[test]
    fn critical_spring_settles_without_overshoot() {
        let s = Spring::new(0.0, 100.0);
        let mut prev = -1.0;
        for i in 0..200 {
            let (x, _) = s.sample(i as f32 * 0.01);
            assert!(x <= 100.0 + 1e-3 && x >= prev - 1e-3);
            prev = x;
        }
        assert_eq!(s.sample(3.0), (100.0, 0.0));
        assert!(s.is_done(3.0));
    }

    #[test]
    fn underdamped_spring_overshoots() {
        let s = Spring::new(0.0, 100.0).damping(0.6);
        let max = (0..100).map(|i| s.sample(i as f32 * 0.01).0).fold(0.0f32, f32::max);
        assert!(max > 100.0);
    }

    #[test]
    fn projection_matches_native_deceleration() {
        assert!((project(1000.0, PROJECT_RATE) - 499.0).abs() < 1.0);
    }

    #[test]
    fn bezier_endpoints_and_monotonic() {
        let f = cubic_bezier(0.2, 0.8, 0.2, 1.0);
        assert_eq!(f(0.0), 0.0);
        assert_eq!(f(1.0), 1.0);
        assert!(f(0.5) > 0.5 && f(0.25) < f(0.5));
    }

    #[test]
    fn spring_curve_stops() {
        assert!((spring_curve(0.04) - 0.17).abs() < 1e-5);
        assert_eq!(spring_curve(1.0), 1.0);
    }

    #[test]
    fn presence_exit() {
        let mut p = Presence::new(Some(1));
        assert!(p.update(None));
        assert!(p.closing() && p.value() == Some(&1));
        let g = p.generation();
        // A new opening cancels the exit.
        p.update(Some(2));
        p.finish(g);
        assert_eq!(p.value(), Some(&2));
        assert!(p.update(None));
        let g = p.generation();
        p.finish(g);
        assert!(p.value().is_none());
    }
}

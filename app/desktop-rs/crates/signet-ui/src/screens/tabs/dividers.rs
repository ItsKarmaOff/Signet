//! Port of `tabs/Dividers.tsx`: resizable split view dividers (8 px grab zones between the panes).
//!
//! The main window puts one [`Dividers`] view over its whole area (it draws nothing but the handles, at the
//! rectangles of `Snapshot.dividers`, in window coordinates).

use std::collections::HashMap;
use std::time::{Duration, Instant};

use gpui::{
    AppContext, Context, DragMoveEvent, InteractiveElement, IntoElement, MouseButton, ParentElement, Render, SharedString, Subscription, StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder, px,
};
use signet_core::types::{Divider, Orientation, Snapshot};

use crate::app_state::{api, ui_state};
use crate::motion::{RUBBER_C, Spring, reduced_motion, rubberband, unrubberband};
use crate::theme::{ThemeExt, space};

/// Minimum size of a pane (same value as `MIN_PANE` in the app).
pub const MIN_PANE: f32 = 200.0;

/// Current lengths of the displayed split view's shares, from the pane rectangles.
pub fn current_lengths(s: &Snapshot, d: &Divider) -> Vec<f32> {
    let vertical = d.orientation == Orientation::Vertical;
    let len = |i: usize| s.panes.get(i).map(|p| if vertical { p.card.width } else { p.card.height } as f32).unwrap_or(0.0);
    let mine = s.dividers.iter().filter(|x| x.split_id == d.split_id).count();
    // Grid: a single horizontal divider for 3 or more panes; shares = [top row, bottom row].
    if !vertical && mine == 1 && s.panes.len() >= 3 {
        return vec![len(0), len(s.panes.len() - 1)];
    }
    (0..s.panes.len()).map(len).collect()
}

/// Result of moving a divider: the new shares, the handle position offset and the overshoot beyond the limits.
#[derive(Debug, PartialEq)]
pub struct Moved {
    /// Shares (sum 1).
    pub sizes: Vec<f64>,
    /// Displacement of the handle inside the limits.
    pub inside: f32,
    /// Rubber-band overshoot (px).
    pub extra: f32,
}

/// Moves the boundary `index` by `delta` px (plus the overshoot `past0` already taken at grab time).
pub fn move_divider(lengths: &[f32], index: usize, delta: f32, past0: f32, reduced: bool) -> Option<Moved> {
    let a = *lengths.get(index)?;
    let b = *lengths.get(index + 1)?;
    let total = a + b;
    // If the two shares cannot respect the minimum, do not move.
    let (lo, hi) = (MIN_PANE, total - MIN_PANE);
    let want = a + delta + past0;
    let next = if total < MIN_PANE * 2.0 { a } else { want.clamp(lo, hi) };
    // The app caps the sizes: only the handle follows the finger beyond that, with resistance.
    let past = if total < MIN_PANE * 2.0 || reduced {
        0.0
    } else if want < lo {
        want - lo
    } else if want > hi {
        want - hi
    } else {
        0.0
    };
    let extra = rubberband(past, 120.0, RUBBER_C);
    let mut l = lengths.to_vec();
    l[index] = next;
    l[index + 1] = total - next;
    let sum: f32 = l.iter().sum();
    Some(Moved { sizes: l.iter().map(|x| (*x / sum) as f64).collect(), inside: next - a, extra })
}

type Key = (String, usize);

#[derive(Clone)]
struct DividerDrag {
    key: Key,
}

struct DragGhost;
impl Render for DragGhost {
    fn render(&mut self, _w: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().size(px(0.0))
    }
}

struct Active {
    key: Key,
    split_id: String,
    index: usize,
    start: f32,
    from: f32,
    lengths: Vec<f32>,
    /// Shares at grab time, restored by Escape.
    initial: Vec<f64>,
    past0: f32,
    rest: f32,
    extra: f32,
    last: (f32, f64, f32),
    sent: Option<Vec<f64>>,
    _esc: Subscription,
}

struct Release {
    spring: Spring,
    start: f64,
    key: Key,
}

pub struct Dividers {
    started: Instant,
    active: Option<Active>,
    release: Option<Release>,
    /// Local position along the axis during the drag (and until the app's state has caught up).
    local: HashMap<Key, (f32, Instant)>,
}

impl Dividers {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let ui = ui_state(cx);
        cx.observe(&ui, |_, _, cx| cx.notify()).detach();
        Dividers { started: Instant::now(), active: None, release: None, local: HashMap::new() }
    }

    fn now_ms(&self) -> f64 {
        self.started.elapsed().as_secs_f64() * 1000.0
    }

    fn begin(&mut self, d: &Divider, pointer: f32, snap: &Snapshot, cx: &mut Context<Self>) {
        let vertical = d.orientation == Orientation::Vertical;
        let axis = if vertical { d.rect.x } else { d.rect.y } as f32;
        let key = (d.split_id.clone(), d.index);
        // Grabbed during the rubber-band: the handle is at `local`, not at the app's clamped position.
        let springing = self.release.take().is_some();
        let shown = if springing { self.local.get(&key).map(|l| l.0).unwrap_or(axis) } else { axis };
        let now = self.now_ms();
        let lengths = current_lengths(snap, d);
        let total: f32 = lengths.iter().sum();
        let initial = lengths.iter().map(|l| if total > 0.0 { (*l / total) as f64 } else { 0.0 }).collect();
        let esc = cx.observe_keystrokes(|this, ev, _w, cx| {
            if ev.keystroke.key == "escape" {
                this.cancel(cx);
            }
        });
        self.active = Some(Active {
            key: key.clone(),
            split_id: d.split_id.clone(),
            index: d.index,
            start: pointer,
            from: axis,
            lengths,
            initial,
            past0: if springing { unrubberband(shown - axis, 120.0, RUBBER_C) } else { 0.0 },
            rest: axis,
            extra: shown - axis,
            last: (shown, now, 0.0),
            sent: None,
            _esc: esc,
        });
        self.local.insert(key, (shown, Instant::now()));
    }

    fn moved(&mut self, pointer: f32, cx: &mut Context<Self>) {
        let reduced = reduced_motion(cx);
        let now = self.now_ms();
        let Some(g) = &mut self.active else { return };
        let Some(m) = move_divider(&g.lengths, g.index, pointer - g.start, g.past0, reduced) else { return };
        g.rest = g.from + m.inside;
        g.extra = m.extra;
        let pos = g.rest + m.extra;
        let dt = now - g.last.1;
        if dt > 0.0 {
            g.last = (pos, now, g.last.2 * 0.5 + ((pos - g.last.0) / dt as f32) * 1000.0 * 0.5);
        }
        g.sent = Some(m.sizes.clone());
        api(cx).set_split_sizes(&g.split_id, &m.sizes, true);
        self.local.insert(g.key.clone(), (pos, Instant::now()));
        cx.notify();
    }

    /// Escape during a drag: the shares of the grab are restored and the rest of the gesture is ignored.
    fn cancel(&mut self, cx: &mut Context<Self>) {
        let Some(g) = self.active.take() else { return };
        if g.sent.is_some() {
            api(cx).set_split_sizes(&g.split_id, &g.initial, false);
        }
        self.local.remove(&g.key);
        cx.notify();
    }

    /// End of the drag: the sizes are saved, the handle springs back from its overshoot, then hands control back.
    fn finish(&mut self, cx: &mut Context<Self>) {
        let Some(g) = self.active.take() else { return };
        if let Some(sizes) = &g.sent {
            api(cx).set_split_sizes(&g.split_id, sizes, false);
        }
        if g.extra.abs() < 0.5 {
            return;
        }
        let now = self.now_ms();
        let idle = now - g.last.1 > 100.0;
        let spring = Spring::new(g.rest + g.extra, g.rest).velocity(if idle { 0.0 } else { g.last.2 });
        self.release = Some(Release { spring, start: now, key: g.key });
        cx.notify();
    }
}

impl Render for Dividers {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.signet().clone();
        let snap = ui_state(cx).read(cx).snapshot.clone();
        if self.active.is_some() && !cx.has_active_drag() {
            self.finish(cx);
        }
        if let Some(r) = &self.release {
            let ts = ((self.now_ms() - r.start) / 1000.0) as f32;
            let (x, _) = r.spring.sample(ts);
            let key = r.key.clone();
            if r.spring.is_done(ts) {
                self.release = None;
                self.local.remove(&key);
            } else {
                self.local.insert(key, (x, Instant::now()));
                window.request_animation_frame();
            }
        }
        // The app's state has caught up with the local position: switch back to it.
        let idle = self.active.is_none() && self.release.is_none();
        if idle {
            self.local.retain(|(sid, ix), (pos, at)| {
                let Some(d) = snap.dividers.iter().find(|d| &d.split_id == sid && d.index == *ix) else { return false };
                let axis = if d.orientation == Orientation::Vertical { d.rect.x } else { d.rect.y } as f32;
                (axis - *pos).abs() > 1.0 && at.elapsed() < Duration::from_millis(400)
            });
        }
        let this = cx.weak_entity();
        let mut layer = div().absolute().size_full();
        for d in &snap.dividers {
            let key: Key = (d.split_id.clone(), d.index);
            let vertical = d.orientation == Orientation::Vertical;
            let local = self.local.get(&key).map(|l| l.0);
            let (mut x, mut y) = (d.rect.x as f32, d.rect.y as f32);
            if let Some(p) = local {
                if vertical { x = p } else { y = p }
            }
            let is_active = self.active.as_ref().is_some_and(|a| a.key == key) || self.release.as_ref().is_some_and(|r| r.key == key);
            let group = SharedString::from(format!("divider-{}-{}", d.split_id, d.index));
            let (d1, d2) = (d.clone(), d.split_id.clone());
            let (this_down, this_move) = (this.clone(), this.clone());
            let key_move = key.clone();
            let snap_down = snap.clone();
            let accent = t.accent;
            let handle = div()
                .rounded(px(2.0))
                .bg(accent)
                .opacity(if is_active { 1.0 } else { 0.0 })
                .group_hover(group.clone(), |s| s.opacity(0.45))
                .when(vertical, |h| h.w(px(3.0)).h(space::S7))
                .when(!vertical, |h| h.w(space::S7).h(px(3.0)));
            layer = layer.child(
                div()
                    .id(group.clone())
                    .group(group)
                    .absolute()
                    .left(px(x))
                    .top(px(y))
                    .w(px(d.rect.width as f32))
                    .h(px(d.rect.height as f32))
                    .flex()
                    .items_center()
                    .justify_center()
                    .when(vertical, |e| e.cursor_col_resize())
                    .when(!vertical, |e| e.cursor_row_resize())
                    .on_mouse_down(MouseButton::Left, move |e, _w, cx| {
                        let pointer = if vertical { f32::from(e.position.x) } else { f32::from(e.position.y) };
                        if e.click_count >= 2 {
                            api(cx).set_split_sizes(&d2, &[], false);
                            return;
                        }
                        let d = d1.clone();
                        let snap = snap_down.clone();
                        this_down.update(cx, |s, cx| {
                            s.begin(&d, pointer, &snap, cx);
                            cx.notify();
                        })
                        .ok();
                    })
                    .on_drag(DividerDrag { key: key.clone() }, |_, _, _, cx| cx.new(|_| DragGhost))
                    .on_drag_move::<DividerDrag>(move |e: &DragMoveEvent<DividerDrag>, _w, cx| {
                        if e.drag(cx).key != key_move {
                            return;
                        }
                        let pointer = if vertical { f32::from(e.event.position.x) } else { f32::from(e.event.position.y) };
                        this_move.update(cx, |s, cx| s.moved(pointer, cx)).ok();
                    })
                    .child(handle),
            );
        }
        layer
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use signet_core::types::{Pane, Rect};

    fn pane(w: f64, h: f64) -> Pane {
        Pane { tab_id: "t".into(), rect: Rect::new(0.0, 0.0, w, h), card: Rect::new(0.0, 0.0, w, h), focused: false, devtools: None, responsive: None }
    }
    fn divider(split: &str, o: Orientation) -> Divider {
        Divider { split_id: split.into(), index: 0, orientation: o, rect: Rect::new(0.0, 0.0, 8.0, 8.0) }
    }

    #[test]
    fn lengths_follow_the_pane_cards_along_the_divider_axis() {
        let s = Snapshot { panes: vec![pane(300.0, 500.0), pane(500.0, 500.0)], dividers: vec![divider("s", Orientation::Vertical)], ..Default::default() };
        assert_eq!(current_lengths(&s, &s.dividers[0]), vec![300.0, 500.0]);
    }

    #[test]
    fn grid_with_one_horizontal_divider_uses_the_two_rows() {
        let s = Snapshot { panes: vec![pane(400.0, 250.0), pane(400.0, 250.0), pane(800.0, 300.0)], dividers: vec![divider("s", Orientation::Horizontal)], ..Default::default() };
        assert_eq!(current_lengths(&s, &s.dividers[0]), vec![250.0, 300.0]);
    }

    #[test]
    fn moving_within_the_limits_shifts_the_shares() {
        let m = move_divider(&[400.0, 400.0], 0, 100.0, 0.0, false).unwrap();
        assert_eq!(m.inside, 100.0);
        assert_eq!(m.extra, 0.0);
        assert!((m.sizes[0] - 0.625).abs() < 1e-6 && (m.sizes[1] - 0.375).abs() < 1e-6);
    }

    #[test]
    fn the_minimum_pane_size_is_kept_and_the_handle_resists_beyond_it() {
        let m = move_divider(&[400.0, 400.0], 0, 500.0, 0.0, false).unwrap();
        // First share capped at 600 (800 - 200); the handle still goes a little further.
        assert!((m.sizes[0] - 0.75).abs() < 1e-6);
        assert!(m.extra > 0.0 && m.extra < 300.0);
        let r = move_divider(&[400.0, 400.0], 0, 500.0, 0.0, true).unwrap();
        assert_eq!(r.extra, 0.0);
    }

    #[test]
    fn too_small_to_respect_the_minimum_does_not_move() {
        let m = move_divider(&[150.0, 150.0], 0, 50.0, 0.0, false).unwrap();
        assert_eq!(m.inside, 0.0);
        assert_eq!(m.extra, 0.0);
    }

    #[test]
    fn missing_boundary_gives_nothing() {
        assert!(move_divider(&[400.0], 0, 10.0, 0.0, false).is_none());
    }
}

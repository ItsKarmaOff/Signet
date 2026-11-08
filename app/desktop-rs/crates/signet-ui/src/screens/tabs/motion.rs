//! Motion of the sidebar rows (ports of `lib/flip.ts`, the active pill, the folder body unfolding):
//!
//! - [`Flip`]: rows glide from their previous place to the new one when the list structure changes. GPUI has no
//!   transform on elements, so a wrapper element shifts its child during prepaint (`Window::with_element_offset`),
//!   by an offset that a spring brings back to zero. The offset is applied in the same frame as the layout change, so
//!   there is no flash at the new place.
//! - [`PillSlide`]: the active-row background that slides from the previous active row to the new one.
//! - [`Unfold`]: a folder or group body whose height animates (and whose chevron turns).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Instant;

use gpui::{
    AnyElement, App, Bounds, Element, ElementId, GlobalElementId, InspectorElementId, IntoElement, LayoutId, Pixels, Point, ScrollHandle, Window, point, px,
};

use crate::motion::spring_curve;
use crate::theme::duration;

/// Spring-like duration shared by the pill and the row glide (`--t-slow`).
pub const GLIDE: std::time::Duration = duration::SLOW;

// ---------------------------------------------------------------------------------------------------------------
// FLIP

struct Slot {
    seen_epoch: u64,
    last_frame: u64,
    /// Place of the element in the previous frame, without the offsets of the ancestors and the scroll.
    last: Option<Point<Pixels>>,
    from: Point<Pixels>,
    start: Option<Instant>,
}

struct FlipInner {
    epoch: Cell<u64>,
    frame: Cell<u64>,
    scroll: ScrollHandle,
    reduced: Cell<bool>,
    /// Sum of the offsets of the FLIP wrappers around the element being prepainted.
    ancestors: Cell<Point<Pixels>>,
    slots: RefCell<HashMap<String, Slot>>,
}

/// List FLIP shared by the wrappers of one list container. Cloning shares the state.
#[derive(Clone)]
pub struct Flip(Rc<FlipInner>);

impl Flip {
    pub fn new(scroll: ScrollHandle) -> Self {
        Flip(Rc::new(FlipInner {
            epoch: Cell::new(0),
            frame: Cell::new(0),
            scroll,
            reduced: Cell::new(false),
            ancestors: Cell::new(Point::default()),
            slots: RefCell::new(HashMap::new()),
        }))
    }

    /// Call once per render, before the wrappers are built.
    pub fn begin_frame(&self, reduced: bool) {
        self.0.frame.set(self.0.frame.get() + 1);
        self.0.reduced.set(reduced);
    }

    /// The list structure changed: the wrappers compare their place with the previous frame's.
    pub fn bump(&self) {
        self.0.epoch.set(self.0.epoch.get() + 1);
    }

    /// Forgets every place (space change: not animated).
    pub fn reset(&self) {
        self.0.slots.borrow_mut().clear();
    }

    /// Wraps `child` so that it glides to its new place. `both_axes`: the grid of favorites also moves sideways.
    pub fn wrap(&self, key: impl Into<String>, both_axes: bool, child: AnyElement) -> AnyElement {
        FlipEl { flip: self.clone(), key: key.into(), both_axes, child }.into_any_element()
    }
}

struct FlipEl {
    flip: Flip,
    key: String,
    both_axes: bool,
    child: AnyElement,
}

impl IntoElement for FlipEl {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

fn offset_of(slot: &Slot) -> Point<Pixels> {
    let Some(start) = slot.start else { return Point::default() };
    let k = start.elapsed().as_secs_f32() / GLIDE.as_secs_f32();
    if k >= 1.0 {
        return Point::default();
    }
    let rest = 1.0 - spring_curve(k);
    point(slot.from.x * rest, slot.from.y * rest)
}

impl Element for FlipEl {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(&mut self, _id: Option<&GlobalElementId>, _inspector: Option<&InspectorElementId>, window: &mut Window, cx: &mut App) -> (LayoutId, ()) {
        (self.child.request_layout(window, cx), ())
    }

    fn prepaint(&mut self, _id: Option<&GlobalElementId>, _inspector: Option<&InspectorElementId>, bounds: Bounds<Pixels>, _state: &mut (), window: &mut Window, cx: &mut App) {
        let inner = &self.flip.0;
        let anc = inner.ancestors.get();
        let scroll = inner.scroll.offset();
        let mut base = point(bounds.origin.x - anc.x - scroll.x, bounds.origin.y - anc.y - scroll.y);
        if !self.both_axes {
            base.x = px(0.0);
        }
        let (epoch, frame) = (inner.epoch.get(), inner.frame.get());
        let off = {
            let mut slots = inner.slots.borrow_mut();
            let slot = slots.entry(self.key.clone()).or_insert(Slot { seen_epoch: epoch, last_frame: frame, last: None, from: Point::default(), start: None });
            let fresh = slot.last_frame + 1 < frame;
            if fresh {
                // Not drawn for a while (folder closed, other space): no animation from a stale place.
                slot.last = None;
                slot.start = None;
            }
            if slot.seen_epoch != epoch {
                slot.seen_epoch = epoch;
                if let (Some(prev), false) = (slot.last, inner.reduced.get()) {
                    let cur = offset_of(slot);
                    let d = point(prev.x - base.x, prev.y - base.y);
                    if d.x.abs() > px(0.5) || d.y.abs() > px(0.5) {
                        slot.from = point(cur.x + d.x, cur.y + d.y);
                        slot.start = Some(Instant::now());
                    }
                }
            }
            slot.last = Some(base);
            slot.last_frame = frame;
            let off = offset_of(slot);
            if slot.start.is_some_and(|s| s.elapsed() < GLIDE) {
                window.request_animation_frame();
            } else {
                slot.start = None;
            }
            off
        };
        inner.ancestors.set(point(anc.x + off.x, anc.y + off.y));
        window.with_element_offset(off, |window| {
            self.child.prepaint(window, cx);
        });
        inner.ancestors.set(anc);
    }

    fn paint(&mut self, _id: Option<&GlobalElementId>, _inspector: Option<&InspectorElementId>, _bounds: Bounds<Pixels>, _state: &mut (), _prepaint: &mut (), window: &mut Window, cx: &mut App) {
        self.child.paint(window, cx);
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Active pill

/// How the active row's background reaches its place.
#[derive(Clone, Debug)]
pub enum PillKind {
    /// From the previous active row (window coordinates of both rows when the tab changed).
    Slide { from: Bounds<Pixels>, to: Bounds<Pixels> },
    /// Another list or no known place: fades in.
    Fade,
}

/// Active-pill animation in progress. The row that draws it (`host`) is the upper one of the two, so that the text of
/// the rows crossed stays above the pill.
#[derive(Clone, Debug)]
pub struct PillSlide {
    /// Row that becomes active: it does not draw its own background while the pill runs.
    pub to_id: String,
    /// Row that draws the pill.
    pub host_id: String,
    pub kind: PillKind,
    pub generation: u64,
    pub start: Instant,
}

impl PillSlide {
    pub fn running(&self) -> bool {
        self.start.elapsed() < GLIDE
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Folder body

/// A folder or group body that is opening or closing.
#[derive(Clone, Copy, Debug)]
pub struct Unfold {
    pub opening: bool,
    pub start: Instant,
    /// Height of the body when open (last measure, or an estimate).
    pub height: f32,
}

impl Unfold {
    pub fn running(&self) -> bool {
        self.start.elapsed() < crate::theme::duration::NORMAL
    }
}

/// Everything the rows need to animate, built by the sidebar for one render (`None` in the tab strip).
#[derive(Clone)]
pub struct RowMotion {
    pub flip: Flip,
    pub pill: Option<Rc<PillSlide>>,
    pub unfold: Rc<HashMap<String, Unfold>>,
}

impl RowMotion {
    pub fn unfold_of(&self, id: &str) -> Option<Unfold> {
        self.unfold.get(id).copied().filter(Unfold::running)
    }
}

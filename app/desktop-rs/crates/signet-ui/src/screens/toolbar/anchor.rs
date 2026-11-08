//! Window-coordinate bounds of an element, for the api calls that anchor a popover under a button
//! (`getBoundingClientRect` in the React bars).

use super::util::rect_of;
use gpui::{Bounds, Pixels, Styled, canvas};
use signet_core::types::Rect;
use std::cell::Cell;
use std::rc::Rc;

/// Shared cell filled at every paint with the bounds of the element it is attached to.
#[derive(Clone, Default)]
pub struct Anchor(Rc<Cell<Bounds<Pixels>>>);

impl Anchor {
    /// An invisible element to place (as a child) inside the element to measure, which must be `relative()`.
    pub fn track(&self) -> impl gpui::IntoElement {
        let cell = self.0.clone();
        canvas(move |bounds, _, _| cell.set(bounds), |_, _, _, _| {}).absolute().size_full()
    }

    pub fn bounds(&self) -> Bounds<Pixels> {
        self.0.get()
    }

    /// Last painted bounds in the api's coordinates.
    pub fn rect(&self) -> Rect {
        rect_of(self.0.get())
    }
}

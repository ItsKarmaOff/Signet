//! Edge fades of scrolling areas (the CSS `mask-image` of `.scroll`, `.ts-days` and `.space-strip`). GPUI has no mask, so
//! a gradient from the background color to transparent is laid over the edge.

use gpui::{Div, Hsla, Styled, div, linear_color_stop, linear_gradient, px};

/// Side of the area that fades.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FadeEdge {
    Top,
    Bottom,
    Left,
    Right,
}

/// An absolutely positioned gradient over one edge of the parent (which must be `relative`), `size` px deep.
/// `None` when there is nothing to fade (size under half a pixel).
pub fn edge_fade(edge: FadeEdge, size: f32, color: Hsla) -> Option<Div> {
    if size < 0.5 {
        return None;
    }
    let clear = Hsla { a: 0.0, ..color };
    // `linear_gradient` angle: 0 points up, clockwise. The gradient runs from the edge inward.
    let angle = match edge {
        FadeEdge::Top => 180.0,
        FadeEdge::Bottom => 0.0,
        FadeEdge::Left => 90.0,
        FadeEdge::Right => 270.0,
    };
    let el = div().absolute().bg(linear_gradient(angle, linear_color_stop(color, 0.0), linear_color_stop(clear, 1.0)));
    Some(match edge {
        FadeEdge::Top => el.top(px(0.0)).left(px(0.0)).right(px(0.0)).h(px(size)),
        FadeEdge::Bottom => el.bottom(px(0.0)).left(px(0.0)).right(px(0.0)).h(px(size)),
        FadeEdge::Left => el.left(px(0.0)).top(px(0.0)).bottom(px(0.0)).w(px(size)),
        FadeEdge::Right => el.right(px(0.0)).top(px(0.0)).bottom(px(0.0)).w(px(size)),
    })
}

/// Fades of a vertical scroller: `offset` is the (negative) scroll position, `max` the scrollable distance. The fade
/// grows over the first `depth` px of scrolling, like the 4% / 96% ramp of the CSS.
pub fn vertical_fades(offset: f32, max: f32, depth: f32, color: Hsla) -> [Option<Div>; 2] {
    let top = (-offset).clamp(0.0, depth);
    let bottom = (max + offset).clamp(0.0, depth);
    [edge_fade(FadeEdge::Top, top, color), edge_fade(FadeEdge::Bottom, bottom, color)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fades_follow_the_scroll_position() {
        let c = gpui::black();
        let [t, b] = vertical_fades(0.0, 100.0, 12.0, c);
        assert!(t.is_none() && b.is_some());
        let [t, b] = vertical_fades(-100.0, 100.0, 12.0, c);
        assert!(t.is_some() && b.is_none());
        let [t, b] = vertical_fades(-50.0, 100.0, 12.0, c);
        assert!(t.is_some() && b.is_some());
        let [t, b] = vertical_fades(0.0, 0.0, 12.0, c);
        assert!(t.is_none() && b.is_none());
    }
}

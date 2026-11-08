use crate::theme::{ThemeExt, radius, space};
use gpui::{App, Div, Styled, div, px};

/// Rounded, translucent, shadowed container for popovers and menus (`--r-pop`, `--glass-view`, `--shadow-pop`).
///
/// GPUI has no backdrop blur: the fill is the high-alpha `glass_view` color. Position it with
/// `gpui_component::popover::Popover` (use `.appearance(false)` so it does not add its own chrome) or `anchored()`.
pub fn popover_surface(cx: &App) -> Div {
    let t = cx.signet();
    div()
        .flex()
        .flex_col()
        .rounded(radius::POP)
        .bg(t.glass_view)
        .border_1()
        .border_color(t.border)
        .shadow(t.shadow_pop.clone())
        .text_color(t.text)
        .text_size(t.type_scale.base)
        .overflow_hidden()
}

/// [`popover_surface`] with the standard inner padding.
pub fn popover_content(cx: &App) -> Div {
    popover_surface(cx).p(space::S2).min_w(px(160.0))
}

//! Peek frame (port of `overlay/PeekFrame.tsx`): scrim, card shadow and buttons. The Peek web view is a native view the
//! app places on the card rectangle, which this surface leaves empty.

use super::common::lucide;
use crate::app_state::{api, tr};
use crate::theme::{ThemeExt, radius, space, weight};
use gpui::{AnyElement, App, InteractiveElement, IntoElement, MouseButton, ParentElement, StatefulInteractiveElement, Styled, div, px};
use signet_core::types::Rect;

/// Height of the bar above the card.
pub const BAR_H: f32 = 28.0;

pub fn peek_frame(card: Rect, title: &str, url: &str, cx: &App) -> AnyElement {
    let t = cx.signet().clone();
    let bar_top = card.y as f32 - 36.0;
    let round = |id: &'static str, icon: &str, size: f32, tip: String| {
        let hover = t.surface_2;
        div()
            .id(id)
            .occlude()
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(px(BAR_H))
            .rounded_full()
            .bg(t.surface)
            .text_color(t.text)
            .border(px(0.5))
            .border_color(t.border)
            .shadow(t.shadow_knob.clone())
            .cursor_pointer()
            .hover(move |s| s.bg(hover))
            .tooltip(move |window, cx| gpui_component::tooltip::Tooltip::new(tip.clone()).build(window, cx))
            .child(lucide(icon).size(px(size)))
    };
    div()
        .absolute()
        .size_full()
        .bg(t.scrim_peek)
        .on_mouse_down(MouseButton::Left, |_, _, cx| api(cx).close_overlay(false))
        .child(div().absolute().left(px(card.x as f32)).top(px(card.y as f32)).w(px(card.width as f32)).h(px(card.height as f32)).rounded(radius::CARD).bg(t.surface).shadow(t.shadow_peek.clone()))
        .child(
            div()
                .absolute()
                .left(px(card.x as f32))
                .top(px(bar_top))
                .w(px(card.width as f32))
                .h(px(BAR_H))
                .flex()
                .items_center()
                .gap(space::S2)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .text_color(t.on_accent_fixed())
                        .font_weight(weight::LABEL)
                        .child(if title.is_empty() { url.to_string() } else { title.to_string() }),
                )
                .child(round("peek-open", "maximize-2", 14.0, tr("peek.openInTab")).on_click(|_, _, cx| api(cx).expand_peek()))
                .child(round("peek-close", "x", 15.0, tr("common.close")).on_click(|_, _, cx| api(cx).close_overlay(false))),
        )
        .into_any_element()
}

trait FixedWhite {
    fn on_accent_fixed(&self) -> gpui::Hsla;
}
impl FixedWhite for crate::theme::SignetTheme {
    /// The title sits on the dark scrim in both schemes.
    fn on_accent_fixed(&self) -> gpui::Hsla {
        gpui::white()
    }
}

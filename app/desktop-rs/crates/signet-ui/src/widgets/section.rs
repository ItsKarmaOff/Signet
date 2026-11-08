use crate::theme::{ThemeExt, space, weight};
use gpui::{AnyElement, App, IntoElement, ParentElement, RenderOnce, SharedString, Styled, Window, div, prelude::FluentBuilder, px};

/// Small caption header of a group of rows, with optional trailing actions.
#[derive(IntoElement)]
pub struct SectionHeader {
    title: SharedString,
    trailing: Option<AnyElement>,
}

impl SectionHeader {
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self { title: title.into(), trailing: None }
    }
    pub fn trailing(mut self, el: impl IntoElement) -> Self {
        self.trailing = Some(el.into_any_element());
        self
    }
}

impl RenderOnce for SectionHeader {
    fn render(self, _w: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = cx.signet();
        div()
            .flex()
            .items_center()
            .gap(space::S2)
            .px(space::S3)
            .pt(space::S2)
            .pb(space::S1)
            .text_size(t.type_scale.cap)
            .font_weight(weight::STRONG)
            .text_color(t.text_2)
            .child(div().flex_1().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().child(self.title))
            .when_some(self.trailing, |d, el| d.child(div().flex_none().child(el)))
    }
}

/// Hairline separator (0.5 px look: 1 px at low alpha).
#[derive(IntoElement)]
pub struct Separator {
    vertical: bool,
}

impl Separator {
    pub fn horizontal() -> Self {
        Self { vertical: false }
    }
    pub fn vertical() -> Self {
        Self { vertical: true }
    }
}

impl RenderOnce for Separator {
    fn render(self, _w: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = cx.signet();
        let d = div().flex_none().bg(t.border);
        if self.vertical { d.w(px(1.0)).h_full() } else { d.h(px(1.0)).w_full() }
    }
}

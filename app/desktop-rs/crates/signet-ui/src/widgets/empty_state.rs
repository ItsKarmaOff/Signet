use crate::icon::IconName;
use crate::theme::{ThemeExt, space, weight};
use gpui::{AnyElement, App, IntoElement, ParentElement, RenderOnce, SharedString, Styled, Window, div, prelude::FluentBuilder, px};
use gpui_component::Icon;

/// Centered placeholder: icon, title, hint and an optional action.
#[derive(IntoElement)]
pub struct EmptyState {
    icon: Option<IconName>,
    title: SharedString,
    hint: Option<SharedString>,
    action: Option<AnyElement>,
}

impl EmptyState {
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self { icon: None, title: title.into(), hint: None, action: None }
    }
    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }
    pub fn hint(mut self, hint: impl Into<SharedString>) -> Self {
        self.hint = Some(hint.into());
        self
    }
    pub fn action(mut self, el: impl IntoElement) -> Self {
        self.action = Some(el.into_any_element());
        self
    }
}

impl RenderOnce for EmptyState {
    fn render(self, _w: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = cx.signet();
        div()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(space::S2)
            .p(space::S6)
            .text_center()
            .text_color(t.text_2)
            .children(self.icon.map(|i| Icon::from(i).size(px(28.0)).text_color(t.text_3)))
            .child(div().text_size(t.type_scale.md).font_weight(weight::LABEL).text_color(t.text).child(self.title))
            .when_some(self.hint, |d, h| d.child(div().text_size(t.type_scale.sub).child(h)))
            .when_some(self.action, |d, a| d.child(div().mt(space::S2).child(a)))
    }
}

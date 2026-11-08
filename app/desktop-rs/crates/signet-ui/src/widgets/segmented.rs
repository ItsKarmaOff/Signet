use crate::icon::IconName;
use crate::theme::{ThemeExt, radius, space, weight};
use gpui::{AnyElement, App, ElementId, InteractiveElement, IntoElement, ParentElement, RenderOnce, SharedString, StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder, px};
use gpui_component::Icon;
use std::rc::Rc;

struct Item {
    label: Option<SharedString>,
    icon: Option<IconName>,
    tint: Option<gpui::Hsla>,
}

/// Segmented control (`.seg-ctl`): a tray with the selected segment raised.
///
/// ```ignore
/// Segmented::new("scheme").item("Light").item("Dark").selected(0).on_change(|ix, _, cx| { /* ... */ })
/// ```
#[derive(IntoElement)]
pub struct Segmented {
    id: ElementId,
    items: Vec<Item>,
    selected: usize,
    on_change: Option<Rc<dyn Fn(usize, &mut Window, &mut App)>>,
    height: f32,
}

impl Segmented {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self { id: id.into(), items: vec![], selected: 0, on_change: None, height: 24.0 }
    }
    pub fn item(mut self, label: impl Into<SharedString>) -> Self {
        self.items.push(Item { label: Some(label.into()), icon: None, tint: None });
        self
    }
    pub fn icon_item(mut self, icon: IconName, label: impl Into<SharedString>) -> Self {
        self.items.push(Item { label: Some(label.into()), icon: Some(icon), tint: None });
        self
    }
    /// Item whose label takes a color when selected (permission allow / deny).
    pub fn tinted_item(mut self, label: impl Into<SharedString>, color: gpui::Hsla) -> Self {
        self.items.push(Item { label: Some(label.into()), icon: None, tint: Some(color) });
        self
    }
    pub fn selected(mut self, ix: usize) -> Self {
        self.selected = ix;
        self
    }
    pub fn height(mut self, h: f32) -> Self {
        self.height = h;
        self
    }
    pub fn on_change(mut self, f: impl Fn(usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(f));
        self
    }
}

impl RenderOnce for Segmented {
    fn render(self, _w: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = cx.signet().clone();
        let on_change = self.on_change.clone();
        let h = px(self.height);
        let id = self.id.clone();
        div()
            .id(self.id)
            .flex()
            .p(space::HALF)
            .rounded(radius::ROW)
            .bg(t.surface_2)
            .children(self.items.into_iter().enumerate().map(|(ix, item)| {
                let on = ix == self.selected;
                let cb = on_change.clone();
                let fg = if on { item.tint.unwrap_or(t.text) } else { t.text_2 };
                let content: AnyElement = div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(space::S1)
                    .children(item.icon.map(|i| Icon::from(i).size(px(13.0))))
                    .children(item.label)
                    .into_any_element();
                div()
                    .id(ElementId::NamedInteger(format!("{id}").into(), ix as u64))
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .h(h)
                    .px(space::S2)
                    .rounded(radius::ROW - space::HALF)
                    .text_size(t.type_scale.sub)
                    .text_color(fg)
                    .cursor_pointer()
                    .when(on, |d| d.bg(t.active).shadow(t.active_shadow.clone()).font_weight(weight::LABEL))
                    .when(!on, |d| d.hover(move |s| s.text_color(t.text)))
                    .when_some(cb, |d, f| d.on_click(move |_, w, cx| f(ix, w, cx)))
                    .child(content)
            }))
    }
}

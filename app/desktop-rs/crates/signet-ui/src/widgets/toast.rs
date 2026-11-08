use crate::motion::surface_enter;
use crate::theme::{ThemeExt, radius, space, weight};
use gpui::{App, ClickEvent, ElementId, InteractiveElement, IntoElement, ParentElement, RenderOnce, SharedString, StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder};
use std::rc::Rc;

/// Announcement with an optional action ("N tabs archived", "Undo"). Place it with `absolute()` above the bottom bar
/// (left / right 6 px, bottom 44 px) and keep it mounted while `closing` with [`crate::motion::Presence`].
#[derive(IntoElement)]
pub struct Toast {
    id: ElementId,
    message: SharedString,
    action: Option<(SharedString, Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>)>,
    closing: bool,
}

impl Toast {
    pub fn new(id: impl Into<ElementId>, message: impl Into<SharedString>) -> Self {
        Self { id: id.into(), message: message.into(), action: None, closing: false }
    }
    pub fn action(mut self, label: impl Into<SharedString>, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.action = Some((label.into(), Rc::new(f)));
        self
    }
    pub fn closing(mut self, closing: bool) -> Self {
        self.closing = closing;
        self
    }
}

impl RenderOnce for Toast {
    fn render(self, _w: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = cx.signet().clone();
        let reduced = crate::motion::reduced_motion(cx);
        let aid = self.id.clone();
        let body = div()
            .flex()
            .items_center()
            .gap(space::S2)
            .py(space::S2)
            .pl(space::S3)
            .pr(space::S2)
            .rounded(radius::POP)
            .bg(t.glass_view)
            .border_1()
            .border_color(t.border)
            .shadow(t.shadow_pop.clone())
            .text_size(t.type_scale.sub)
            .text_color(t.text)
            .child(div().flex_1().min_w_0().child(self.message))
            .when_some(self.action, |d, (label, f)| {
                d.child(
                    div()
                        .id(ElementId::from((aid, "action")))
                        .flex_none()
                        .px(space::S2)
                        .py(space::S1)
                        .rounded(radius::CONTROL)
                        .text_color(t.accent)
                        .font_weight(weight::STRONG)
                        .cursor_pointer()
                        .hover(move |s| s.bg(t.sel))
                        .on_click(move |e, w, cx| f(e, w, cx))
                        .child(label),
                )
            });
        surface_enter(body, self.id, self.closing, reduced)
    }
}

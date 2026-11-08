use crate::theme::{ThemeExt, radius, space};
use gpui::{App, ClickEvent, ElementId, FocusHandle, InteractiveElement, IntoElement, ParentElement, RenderOnce, SharedString, StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder};
use std::rc::Rc;

type Cb = Rc<dyn Fn(&mut Window, &mut App)>;

/// In-place confirmation of a destructive action: a message naming the object and consequence, then Cancel
/// (default focus) and the action button in danger style. Escape cancels. Port of `ui/InlineConfirm.tsx`.
///
/// Pass the `FocusHandle` of the cancel button to focus it on first render (`window.focus(&handle)`).
#[derive(IntoElement)]
pub struct InlineConfirm {
    id: ElementId,
    message: SharedString,
    confirm_label: SharedString,
    cancel_label: SharedString,
    on_confirm: Option<Cb>,
    on_cancel: Option<Cb>,
    cancel_focus: Option<FocusHandle>,
}

impl InlineConfirm {
    pub fn new(id: impl Into<ElementId>, message: impl Into<SharedString>, confirm_label: impl Into<SharedString>, cancel_label: impl Into<SharedString>) -> Self {
        Self { id: id.into(), message: message.into(), confirm_label: confirm_label.into(), cancel_label: cancel_label.into(), on_confirm: None, on_cancel: None, cancel_focus: None }
    }
    pub fn on_confirm(mut self, f: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_confirm = Some(Rc::new(f));
        self
    }
    pub fn on_cancel(mut self, f: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_cancel = Some(Rc::new(f));
        self
    }
    pub fn cancel_focus(mut self, h: &FocusHandle) -> Self {
        self.cancel_focus = Some(h.clone());
        self
    }
}

impl RenderOnce for InlineConfirm {
    fn render(self, _w: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = cx.signet().clone();
        let (confirm, cancel) = (self.on_confirm.clone(), self.on_cancel.clone());
        let cancel_for_key = self.on_cancel.clone();
        let btn = |id: ElementId| {
            div()
                .id(id)
                .flex_none()
                .flex()
                .items_center()
                .h(gpui::px(28.0))
                .px(space::S3)
                .rounded(radius::CONTROL)
                .whitespace_nowrap()
                .cursor_pointer()
        };
        div()
            .id(self.id.clone())
            .flex()
            .flex_wrap()
            .items_center()
            .gap(space::S2)
            .text_size(t.type_scale.base)
            .on_key_down(move |ev, w, cx| {
                if ev.keystroke.key == "escape" {
                    cx.stop_propagation();
                    if let Some(f) = &cancel_for_key {
                        f(w, cx);
                    }
                }
            })
            .child(div().text_size(t.type_scale.sub).text_color(t.text_2).child(self.message))
            .child(
                btn(ElementId::from((self.id.clone(), "cancel")))
                    .bg(t.ctl)
                    .border_1()
                    .border_color(t.ctl_ring)
                    .text_color(t.text)
                    .hover(move |s| s.opacity(0.9))
                    .when_some(self.cancel_focus.as_ref(), |d, h| d.track_focus(h))
                    .on_click(move |_: &ClickEvent, w, cx| {
                        if let Some(f) = &cancel {
                            f(w, cx)
                        }
                    })
                    .child(self.cancel_label),
            )
            .child(
                btn(ElementId::from((self.id, "confirm")))
                    .bg(t.danger)
                    .text_color(t.on_accent)
                    .hover(move |s| s.opacity(0.9))
                    .on_click(move |_: &ClickEvent, w, cx| {
                        if let Some(f) = &confirm {
                            f(w, cx)
                        }
                    })
                    .child(self.confirm_label),
            )
    }
}

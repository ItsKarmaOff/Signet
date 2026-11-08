use crate::theme::{ThemeExt, radius, space};
use gpui::{App, ClickEvent, Div, SharedString, Styled, Window, div, px};
use gpui_component::{WindowExt, button::ButtonVariant};
use std::rc::Rc;

/// Content of a confirmation dialog. Strings are already translated.
pub struct ConfirmDialog {
    pub title: SharedString,
    pub message: SharedString,
    pub confirm: SharedString,
    pub cancel: SharedString,
    /// Destructive action: the confirm button is red.
    pub danger: bool,
    pub on_confirm: Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>,
}

/// Opens a modal confirmation (gpui-component alert dialog, themed). The window must have a `Root`.
pub fn confirm_dialog(window: &mut Window, cx: &mut App, d: ConfirmDialog) {
    let ConfirmDialog { title, message, confirm, cancel, danger, on_confirm } = d;
    window.open_alert_dialog(cx, move |alert, _, _| {
        let on_confirm = on_confirm.clone();
        alert
            .title(title.clone())
            .description(message.clone())
            .ok_text(confirm.clone())
            .cancel_text(cancel.clone())
            .ok_variant(if danger { ButtonVariant::Danger } else { ButtonVariant::Primary })
            .show_cancel(true)
            .on_ok(move |e, w, cx| {
                on_confirm(e, w, cx);
                true
            })
    });
}

/// Container for custom modal surfaces (command bar, forms): `--r-pop`, opaque-ish fill, `--shadow-modal`.
/// Center it over a `scrim` backdrop yourself.
pub fn modal_surface(cx: &App) -> Div {
    let t = cx.signet();
    div()
        .flex()
        .flex_col()
        .rounded(radius::POP)
        .bg(t.glass_view)
        .border_1()
        .border_color(t.border)
        .shadow(t.shadow_modal.clone())
        .text_color(t.text)
        .text_size(t.type_scale.base)
        .p(space::S4)
        .min_w(px(320.0))
        .overflow_hidden()
}

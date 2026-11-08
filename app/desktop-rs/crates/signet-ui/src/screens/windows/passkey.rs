//! Port of `windows/Passkey.tsx` + `styles/passkey.css` (+ the `.welcome` layout it reuses): route `#passkey`, the QR code
//! window for a passkey from a phone. Escape or Cancel gives up (`cancel_passkey_prompt`).

use super::common::{drag_area, st_btn};
use super::qr::{QrGrid, parse_qr_svg, qr_element};
use crate::app_state::{api, tr, tr_args, ui_state};
use crate::theme::{ThemeExt, radius, space, weight};
use gpui::{AppContext, Context, Entity, FocusHandle, Focusable, InteractiveElement, IntoElement, ParentElement, Render, StatefulInteractiveElement, Styled, Window, div, px};
use signet_core::api::UiEvent;
use signet_core::types::{PasskeyPromptState, PasskeyStatus, PasskeyType};

pub struct Passkey {
    prompt: Option<PasskeyPromptState>,
    qr: Option<QrGrid>,
    focus: FocusHandle,
}

/// Key of the status line: the plain hint while waiting (or when scanning is not possible), otherwise what is going on.
pub fn hint_key(status: PasskeyStatus) -> String {
    match status {
        PasskeyStatus::Waiting | PasskeyStatus::Unsupported => "passkey.hint".into(),
        PasskeyStatus::Off => "passkey.status.off".into(),
        PasskeyStatus::NoAdapter => "passkey.status.no-adapter".into(),
        PasskeyStatus::Denied => "passkey.status.denied".into(),
        PasskeyStatus::Error => "passkey.status.error".into(),
        PasskeyStatus::Connecting => "passkey.status.connecting".into(),
        PasskeyStatus::Confirm => "passkey.status.confirm".into(),
        PasskeyStatus::Failed => "passkey.status.failed".into(),
    }
}

impl Passkey {
    pub fn new(window: &mut Window, cx: &mut gpui::App) -> Entity<Self> {
        let view = cx.new(|cx: &mut Context<Self>| {
            // Subscribe first: a status pushed while the prompt is fetched is not lost.
            cx.subscribe(&ui_state(cx), |this, _, ev: &UiEvent, cx| {
                if let UiEvent::PasskeyStatus(status) = ev {
                    if let Some(p) = this.prompt.as_mut() {
                        p.status = *status;
                    }
                    cx.notify();
                }
            })
            .detach();
            let prompt = api(cx).get_passkey_prompt();
            let qr = prompt.as_ref().and_then(|p| parse_qr_svg(&p.qr));
            Self { prompt, qr, focus: cx.focus_handle() }
        });
        let focus = view.read(cx).focus.clone();
        window.focus(&focus, cx);
        view
    }

    fn cancel(cx: &mut gpui::App) {
        api(cx).cancel_passkey_prompt();
    }
}

impl Focusable for Passkey {
    fn focus_handle(&self, _cx: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Passkey {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.signet().clone();
        let kind = self.prompt.as_ref().map(|p| p.kind).unwrap_or(PasskeyType::Get);
        let status = self.prompt.as_ref().map(|p| p.status).unwrap_or(PasskeyStatus::Waiting);
        let kind_key = if kind == PasskeyType::Get { "get" } else { "create" };
        let plain = matches!(status, PasskeyStatus::Waiting | PasskeyStatus::Unsupported);
        let (hint_color, hint_weight) = if plain {
            (t.text_2, weight::BODY)
        } else if matches!(status, PasskeyStatus::Connecting | PasskeyStatus::Confirm) {
            (t.text, weight::STRONG)
        } else {
            (t.danger, weight::BODY)
        };
        let text = self.prompt.as_ref().map(|p| tr_args(&format!("passkey.{kind_key}Text"), &[("site", &p.rp_id)])).unwrap_or_default();

        let qr_box = div().flex_none().size(px(232.0)).rounded(radius::ROW).bg(gpui::white()).overflow_hidden().children(self.qr.clone().map(|g| qr_element(g, 232.0)));

        div()
            .id("passkey")
            .track_focus(&self.focus)
            .on_key_down(|ev, _, cx| {
                let k = ev.keystroke.key.as_str();
                if k == "escape" || k == "enter" || k == "space" {
                    cx.stop_propagation();
                    Self::cancel(cx);
                }
            })
            .flex()
            .flex_col()
            .size_full()
            .bg(t.bg)
            .text_color(t.text)
            .text_size(t.type_scale.base)
            .child(drag_area(div().flex_none().h(px(44.0))))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .items_center()
                    .px(space::S8)
                    .pt(space::S4)
                    .text_center()
                    .child(div().text_size(t.type_scale.title).font_weight(weight::HERO).line_height(gpui::relative(1.1)).child(tr(&format!("passkey.{kind_key}Title"))))
                    .child(div().max_w(px(420.0)).min_h(t.type_scale.base * 2.9).mt(space::S3).mb(space::S4).text_color(t.text_2).line_height(gpui::relative(1.45)).child(text))
                    .child(qr_box)
                    .child(div().max_w(px(300.0)).mt(space::S3).mb(space::S5).text_size(t.type_scale.sub).line_height(gpui::relative(1.35)).text_color(hint_color).font_weight(hint_weight).child(tr(&hint_key(status))))
                    .child(st_btn("cancel", &t).child(tr("passkey.cancel")).on_click(|_, _, cx| Self::cancel(cx))),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hint_follows_status() {
        assert_eq!(hint_key(PasskeyStatus::Waiting), "passkey.hint");
        assert_eq!(hint_key(PasskeyStatus::Unsupported), "passkey.hint");
        assert_eq!(hint_key(PasskeyStatus::NoAdapter), "passkey.status.no-adapter");
        assert_eq!(hint_key(PasskeyStatus::Confirm), "passkey.status.confirm");
    }
}

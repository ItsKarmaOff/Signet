//! Password prompt and saved accounts list (port of `overlay/PasswordPrompt.tsx`).

use super::common::{arrow_up, host_of, lucide, panel, small_btn};
use crate::app_state::{api, tr};
use crate::theme::{ThemeExt, radius, space, weight};
use gpui::{
    AppContext, ClipboardItem, Context, Entity, InteractiveElement, IntoElement, ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder, px,
};
use signet_core::types::{AutofillKind, LoginRef, Rect};

pub const WIDTH: f32 = 360.0;
/// Shadow margin around the card when the overlay covers only it (= `PW_MARGIN` of the app's overlay placement).
pub const MARGIN: f32 = 24.0;

/// Card placed under the key button: right edge aligned with it, arrow pointing at its center.
fn under_anchor(anchor: Rect, window_w: f32) -> (f32, f32, f32) {
    let left = (anchor.x as f32 + anchor.width as f32 + 6.0 - WIDTH).clamp(8.0, (window_w - WIDTH - 8.0).max(8.0));
    let arrow = ((anchor.x + anchor.width / 2.0) as f32 - left).clamp(14.0, WIDTH - 14.0);
    (left, (anchor.y + anchor.height) as f32 + 8.0, arrow)
}

fn head(icon: &str, title: String, sub: String, cx: &gpui::App) -> impl IntoElement {
    let t = cx.signet();
    div()
        .flex()
        .items_center()
        .gap(space::S3)
        .pt(space::S2)
        .px(space::S2)
        .pb(space::S3)
        .child(lucide(icon).size(px(18.0)).text_color(t.text_2))
        .child(
            div()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(1.0))
                .child(div().overflow_hidden().text_ellipsis().whitespace_nowrap().font_weight(weight::STRONG).child(title))
                .child(div().text_size(t.type_scale.sub).text_color(t.text_2).child(sub)),
        )
}

pub struct PasswordPrompt {
    token: String,
    origin: String,
    username: String,
    update: bool,
    kind: Option<AutofillKind>,
    anchor: Option<Rect>,
}

impl PasswordPrompt {
    #[allow(clippy::too_many_arguments)]
    pub fn new(token: String, origin: String, username: String, update: bool, kind: Option<AutofillKind>, anchor: Option<Rect>, cx: &mut gpui::App) -> Entity<Self> {
        cx.new(|_| Self { token, origin, username, update, kind, anchor })
    }
}

impl Render for PasswordPrompt {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.signet().clone();
        let (ww, _) = super::common::viewport(window);
        let placed = self.anchor.map(|a| under_anchor(a, ww));
        let (left, top) = placed.map(|p| (p.0, p.1)).unwrap_or((MARGIN, MARGIN));
        let titles = match self.kind {
            Some(AutofillKind::Card) => ["password.saveCardTitle", "password.updateCardTitle"],
            Some(AutofillKind::Address) => ["password.saveAddressTitle", "password.updateAddressTitle"],
            None => ["password.saveTitle", "password.updateTitle"],
        };
        let icon = match self.kind {
            Some(AutofillKind::Card) => "credit-card",
            Some(AutofillKind::Address) => "map-pin",
            None => "key-round",
        };
        let user = if self.username.is_empty() && self.kind.is_none() { tr("password.noUsername") } else { self.username.clone() };
        let (tok1, tok2, tok3) = (self.token.clone(), self.token.clone(), self.token.clone());
        let answer = |token: String, a: signet_core::api::PasswordAnswer| move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut gpui::App| api(cx).answer_password_prompt(&token, a);
        use signet_core::api::PasswordAnswer as A;
        panel(cx)
            .id("pw-prompt")
            .absolute()
            .left(px(left))
            .top(px(top))
            .w(px(WIDTH))
            .p(px(6.0))
            .when_some(placed, |d, p| d.child(arrow_up(cx, p.2)))
            .child(head(icon, tr(titles[usize::from(self.update)]), host_of(&self.origin), cx))
            .child(div().pt(space::S1).px(space::S2).pb(space::S3).overflow_hidden().text_ellipsis().whitespace_nowrap().text_color(t.text_2).child(user))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(space::S2)
                    .pt(space::S2)
                    .px(space::S1)
                    .pb(space::HALF)
                    .border_t_1()
                    .border_color(t.border)
                    .child(small_btn("pw-never", cx).px(space::S2).child(tr("password.never")).on_click(answer(tok1, A::Never)))
                    .child(div().flex_1())
                    .child(small_btn("pw-later", cx).px(space::S2).child(tr("password.notNow")).on_click(answer(tok2, A::Dismiss)))
                    .child(small_btn("pw-save", cx).px(space::S2).bg(t.accent).text_color(t.on_accent).child(if self.update { tr("password.update") } else { tr("password.save") }).on_click(answer(tok3, A::Save))),
            )
    }
}

pub struct PasswordList {
    origin: String,
    logins: Vec<LoginRef>,
    anchor: Rect,
    copied: Option<String>,
}

impl PasswordList {
    pub fn new(origin: String, logins: Vec<LoginRef>, anchor: Rect, cx: &mut gpui::App) -> Entity<Self> {
        cx.new(|_| Self { origin, logins, anchor, copied: None })
    }
}

impl Render for PasswordList {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.signet().clone();
        let (ww, _) = super::common::viewport(window);
        let (left, top, arrow) = under_anchor(self.anchor, ww);
        let me = cx.entity();
        let rows = self.logins.iter().map(|l| {
            let (me, id, name) = (me.clone(), l.id.clone(), l.username.clone());
            let copied = self.copied.as_deref() == Some(l.id.as_str());
            div()
                .id(SharedString::from(format!("login-{}", l.id)))
                .flex()
                .items_center()
                .gap(space::S2)
                .min_h(px(32.0))
                .pl(space::S2)
                .pr(space::S1)
                .rounded(radius::ROW)
                .child(div().flex_1().min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap().child(if l.username.is_empty() { tr("password.noUsername") } else { l.username.clone() }))
                .when(!l.username.is_empty(), |d| {
                    d.child(
                        small_btn(SharedString::from(format!("copy-{}", l.id)), cx)
                            .child(lucide(if copied { "check" } else { "copy" }).size(px(13.0)))
                            .child(if copied { tr("password.copied") } else { tr("password.copyUsernameShort") })
                            .on_click(move |_, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(name.clone()));
                                me.update(cx, |this, cx| {
                                    this.copied = Some(id.clone());
                                    cx.notify();
                                })
                            }),
                    )
                })
        });
        panel(cx)
            .id("pw-list")
            .absolute()
            .left(px(left))
            .top(px(top))
            .w(px(WIDTH))
            .p(px(6.0))
            .child(arrow_up(cx, arrow))
            .child(head("key-round", tr("password.listTitle"), host_of(&self.origin), cx))
            .child(div().flex().flex_col().gap(space::HALF).px(space::S1).pb(space::S2).children(rows))
            .child(
                div().flex().items_center().gap(space::S2).pt(space::S2).px(space::S1).pb(space::HALF).border_t_1().border_color(t.border).child(
                    small_btn("pw-manage", cx).child(lucide("settings").size(px(13.0))).child(tr("password.manage")).on_click(|_, _, cx| {
                        api(cx).open_settings(Some("passwords"));
                        api(cx).close_overlay(false);
                    }),
                ),
            )
    }
}
